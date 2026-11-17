//! A cancelled or abandoned cleanup request cannot affect another operation.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_REQUESTS: usize = 8;
const RESERVATION_TTL: Duration = Duration::from_secs(60);
const BUSY: &str = "too many cleanup requests";

struct Request {
    reserved_at: Option<Instant>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct Requests {
    last_id: u32,
    entries: BTreeMap<u32, Request>,
}

impl Requests {
    fn expire_unclaimed(&mut self, now: Instant) {
        self.entries.retain(|_, request| {
            request
                .reserved_at
                .is_none_or(|reserved| now.duration_since(reserved) < RESERVATION_TTL)
        });
    }
}

static REQUESTS: Mutex<Requests> = Mutex::new(Requests {
    last_id: 0,
    entries: BTreeMap::new(),
});

pub(crate) fn begin() -> Result<u32, String> {
    let mut requests = REQUESTS.lock().map_err(|_| BUSY)?;
    let now = Instant::now();
    requests.expire_unclaimed(now);
    if requests.entries.len() >= MAX_REQUESTS {
        return Err(BUSY.into());
    }
    let id = requests.last_id.checked_add(1).ok_or(BUSY)?;
    requests.last_id = id;
    requests.entries.insert(
        id,
        Request {
            reserved_at: Some(now),
            cancelled: Arc::new(AtomicBool::new(false)),
        },
    );
    Ok(id)
}

pub(crate) fn cancel(id: u32) -> bool {
    let Ok(mut requests) = REQUESTS.lock() else {
        return false;
    };
    let Some(request) = requests.entries.get(&id) else {
        return false;
    };
    if request.reserved_at.is_some() {
        requests.entries.remove(&id);
    } else {
        request.cancelled.store(true, Ordering::Release);
    }
    true
}

pub(crate) struct CleanupRequest {
    id: u32,
    cancelled: Arc<AtomicBool>,
}

impl CleanupRequest {
    pub(crate) fn claim(id: u32) -> Result<Self, String> {
        let mut requests = REQUESTS.lock().map_err(|_| BUSY)?;
        requests.expire_unclaimed(Instant::now());
        let request = requests
            .entries
            .get_mut(&id)
            .ok_or("cleanup request cancelled or expired")?;
        if request.reserved_at.take().is_none() {
            return Err("cleanup request already started".into());
        }
        Ok(Self {
            id,
            cancelled: Arc::clone(&request.cancelled),
        })
    }

    pub(crate) fn run<T>(self, work: impl FnOnce() -> T) -> T {
        super::with_operation_cancellation(Arc::clone(&self.cancelled), work)
    }
}

impl Drop for CleanupRequest {
    fn drop(&mut self) {
        if let Ok(mut requests) = REQUESTS.lock() {
            requests.entries.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_only_reaches_the_claimed_cleanup_id() {
        let one = begin().unwrap();
        let two = begin().unwrap();
        let first = CleanupRequest::claim(one).unwrap();
        let second = CleanupRequest::claim(two).unwrap();
        assert!(CleanupRequest::claim(one).is_err());
        first.run(|| {
            assert!(std::thread::spawn(move || cancel(one)).join().unwrap());
            assert!(super::super::cleanup_cancelled());
            assert!(!std::thread::spawn(super::super::cleanup_cancelled)
                .join()
                .unwrap());
            second.run(|| assert!(!super::super::cleanup_cancelled()));
            assert!(super::super::cleanup_cancelled());
        });
        assert!(!cancel(one));
        assert!(!cancel(two));
        assert!(!super::super::cleanup_cancelled());
        let abandoned = begin().unwrap();
        assert!(cancel(abandoned));
        assert!(CleanupRequest::claim(abandoned).is_err());
    }

    #[test]
    fn a_cancelled_cleanup_leaves_an_unrelated_lock_holder_alone() {
        let _guard = super::super::CLEANUP_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let id = begin().unwrap();
        let request = CleanupRequest::claim(id).unwrap();
        let (entered, waiting) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            request.run(|| {
                entered.send(()).unwrap();
                super::super::wait_for_cleanup_lock(super::super::cleanup_cancelled).is_none()
            })
        });
        waiting.recv().unwrap();
        assert!(cancel(id));
        assert!(worker.join().unwrap());
        assert!(!cancel(id));
        assert!(!super::super::cleanup_cancelled());
    }
}
