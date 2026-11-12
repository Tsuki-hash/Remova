use super::{copy_phase, MigrationCopyReport};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(super) const CANCELLED: &str = "migrate:cancelled";
const COPY_BUSY: &str = "migrate:copy_busy";
const MAX_OPERATIONS: usize = 8;
const CLAIM_TIMEOUT: Duration = Duration::from_secs(60);

struct Request {
    paths: Option<(String, String)>,
    cancelled: Arc<AtomicBool>,
    reserved_at: Instant,
}

#[derive(Default)]
struct Requests {
    last_id: u32,
    active: BTreeMap<u32, Request>,
}

impl Requests {
    fn expire_unclaimed(&mut self, now: Instant) {
        self.active.retain(|_, request| {
            request.paths.is_none() || now.duration_since(request.reserved_at) < CLAIM_TIMEOUT
        });
    }

    fn reserve(&mut self, source: String, target: String, now: Instant) -> Result<u32, String> {
        self.expire_unclaimed(now);
        if self.active.len() >= MAX_OPERATIONS {
            return Err(COPY_BUSY.into());
        }
        let id = self.last_id.checked_add(1).ok_or(COPY_BUSY)?;
        self.last_id = id;
        self.active.insert(
            id,
            Request {
                paths: Some((source, target)),
                cancelled: Arc::new(AtomicBool::new(false)),
                reserved_at: now,
            },
        );
        Ok(id)
    }
}

fn requests() -> &'static Mutex<Requests> {
    static REQUESTS: OnceLock<Mutex<Requests>> = OnceLock::new();
    REQUESTS.get_or_init(|| Mutex::new(Requests::default()))
}

pub(crate) fn begin(source: String, target: String) -> Result<u32, String> {
    requests()
        .lock()
        .map_err(|_| COPY_BUSY)?
        .reserve(source, target, Instant::now())
}

pub(crate) fn cancel(id: u32) -> Result<(), String> {
    let mut requests = requests().lock().map_err(|_| COPY_BUSY)?;
    if let Some(request) = requests.active.get(&id) {
        if request.paths.is_some() {
            requests.active.remove(&id);
        } else {
            request.cancelled.store(true, Ordering::Release);
        }
    }
    Ok(())
}

pub(crate) struct Operation {
    id: u32,
    source: String,
    target: String,
    cancelled: Arc<AtomicBool>,
}

impl Operation {
    pub(crate) fn claim(id: u32) -> Result<Self, String> {
        let mut requests = requests().lock().map_err(|_| COPY_BUSY)?;
        requests.expire_unclaimed(Instant::now());
        let request = requests.active.get_mut(&id).ok_or(CANCELLED)?;
        let (source, target) = request.paths.take().ok_or(COPY_BUSY)?;
        Ok(Self {
            id,
            source,
            target,
            cancelled: Arc::clone(&request.cancelled),
        })
    }

    pub(crate) fn run(self) -> Result<MigrationCopyReport, String> {
        let _scope = CancellationScope(CURRENT.replace(Some(Arc::clone(&self.cancelled))));
        copy_phase(&self.source, &self.target)
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        if let Ok(mut requests) = requests().lock() {
            requests.active.remove(&self.id);
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

struct CancellationScope(Option<Arc<AtomicBool>>);

impl Drop for CancellationScope {
    fn drop(&mut self) {
        CURRENT.replace(self.0.take());
    }
}

pub(super) fn cancelled() -> bool {
    CURRENT.with_borrow(|flag| {
        flag.as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    })
}

#[cfg(windows)]
pub(super) fn flag() -> Option<Arc<AtomicBool>> {
    CURRENT.with_borrow(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancel_before_the_worker_claims_its_request_prevents_start() {
        let id = begin("source".into(), "target".into()).unwrap();
        cancel(id).unwrap();
        assert_eq!(Operation::claim(id).err().as_deref(), Some(CANCELLED));
        assert!(!cancelled());
    }

    #[test]
    fn abandoned_reservations_expire_without_affecting_claimed_copies() {
        let mut requests = Requests::default();
        let now = Instant::now();
        let reserve =
            |requests: &mut Requests, now| requests.reserve("source".into(), "target".into(), now);
        let claimed = reserve(&mut requests, now).unwrap();
        requests.active.get_mut(&claimed).unwrap().paths.take();
        for _ in 1..MAX_OPERATIONS {
            reserve(&mut requests, now).unwrap();
        }
        assert_eq!(reserve(&mut requests, now).unwrap_err(), COPY_BUSY);

        let new = reserve(&mut requests, now + CLAIM_TIMEOUT).unwrap();
        assert_eq!(
            requests.active.keys().copied().collect::<Vec<_>>(),
            [claimed, new]
        );
        assert!(new > MAX_OPERATIONS as u32);
    }

    #[cfg(windows)]
    #[test]
    fn cancelling_a_queued_copy_keeps_the_other_copy_and_its_files() {
        use std::sync::mpsc;

        let fixture = crate::test_support::TempDir::new("migration_copy_requests");
        let first_source = fixture.subdir("first-source");
        let first_target = fixture.join("first-target");
        let second_source = fixture.subdir("second-source");
        let second_target = fixture.join("second-target");
        std::fs::write(first_source.join("data"), b"first").unwrap();
        std::fs::write(second_source.join("data"), b"second").unwrap();
        let begin_pair = |source: &std::path::Path, target: &std::path::Path| {
            begin(source.display().to_string(), target.display().to_string()).unwrap()
        };
        let first_id = begin_pair(&first_source, &first_target);
        let second_id = begin_pair(&second_source, &second_target);
        let first = Operation::claim(first_id).unwrap();
        let second = Operation::claim(second_id).unwrap();
        assert_eq!(
            Operation::claim(second_id).err().as_deref(),
            Some(COPY_BUSY)
        );

        let lock = crate::executor::CLEANUP_LOCK.lock().unwrap();
        let first_worker = std::thread::spawn(move || first.run());
        let (done, result) = mpsc::channel();
        let second_worker = std::thread::spawn(move || done.send(second.run()).unwrap());
        cancel(second_id).unwrap();
        let queued = result.recv_timeout(Duration::from_secs(5));
        drop(lock);
        assert_eq!(queued.unwrap().unwrap_err(), CANCELLED);
        second_worker.join().unwrap();
        assert!(first_worker.join().unwrap().is_ok());
        assert_eq!(std::fs::read(first_target.join("data")).unwrap(), b"first");
        assert_eq!(
            std::fs::read(second_source.join("data")).unwrap(),
            b"second"
        );
        assert!(!second_target.exists());

        let retry_id = begin_pair(&second_source, &second_target);
        cancel(first_id).unwrap();
        cancel(second_id).unwrap();
        Operation::claim(retry_id).unwrap().run().unwrap();
        assert_eq!(
            std::fs::read(second_target.join("data")).unwrap(),
            b"second"
        );
    }
}
