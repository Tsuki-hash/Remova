//! Task-local cancellation for read-only association scans; never used by deletion.
use serde::Serialize;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Serialize)]
pub struct Progress {
    pub stage: String,
    pub found: usize,
}

struct Task {
    id: u32,
    scope: Option<&'static str>,
    cancelled: AtomicBool,
    committed: AtomicBool,
    started: bool,
    progress: Mutex<Progress>,
}

static NEXT: AtomicU32 = AtomicU32::new(1);
fn latest() -> &'static Mutex<BTreeMap<&'static str, u32>> {
    static LATEST: OnceLock<Mutex<BTreeMap<&'static str, u32>>> = OnceLock::new();
    LATEST.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn tasks() -> &'static Mutex<BTreeMap<u32, Arc<Task>>> {
    static TASKS: OnceLock<Mutex<BTreeMap<u32, Arc<Task>>>> = OnceLock::new();
    TASKS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
thread_local! {
    static CURRENT: RefCell<Option<Arc<Task>>> = const { RefCell::new(None) };
}

pub fn begin() -> Result<u32, String> {
    let mut tasks = tasks().lock().map_err(|_| "scan:state")?;
    if tasks.len() >= 8 {
        return Err("scan:busy".into());
    }
    let id = NEXT
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_add(1))
        .map_err(|_| "scan:state")?;
    tasks.insert(
        id,
        Arc::new(Task {
            id,
            scope: None,
            cancelled: AtomicBool::new(false),
            committed: AtomicBool::new(false),
            started: false,
            progress: Mutex::new(Progress {
                stage: "preparing".into(),
                found: 0,
            }),
        }),
    );
    Ok(id)
}

pub fn cancel(id: u32) -> Result<bool, String> {
    let mut tasks = tasks().lock().map_err(|_| "scan:state")?;
    if let Some(task) = tasks.get(&id) {
        if task.committed.load(Ordering::SeqCst) {
            return Ok(false);
        }
        task.cancelled.store(true, Ordering::SeqCst);
        if !task.started {
            tasks.remove(&id);
        }
        return Ok(true);
    }
    Ok(false)
}

pub fn progress(id: u32) -> Option<Progress> {
    let tasks = tasks().lock().ok()?;
    tasks.get(&id)?.progress.lock().ok().map(|p| p.clone())
}

/// New scans supersede only their own source. IDs also reject out-of-order workers.
pub fn run_scoped<T>(id: u32, scope: &'static str, work: impl FnOnce() -> T) -> Result<T, String> {
    {
        let mut tasks = tasks().lock().map_err(|_| "scan:state")?;
        let mut latest = latest().lock().map_err(|_| "scan:state")?;
        if latest.get(scope).is_some_and(|previous| *previous > id) {
            tasks.remove(&id);
            return Err("scan:cancelled".into());
        }
        let task = tasks.get_mut(&id).ok_or("scan:cancelled")?;
        Arc::get_mut(task).ok_or("scan:busy")?.scope = Some(scope);
        tasks.retain(|other, task| {
            if *other < id && task.scope == Some(scope) {
                if !task.committed.load(Ordering::SeqCst) {
                    task.cancelled.store(true, Ordering::SeqCst);
                }
                task.started
            } else {
                true
            }
        });
        latest.insert(scope, id);
    }
    run(id, work)
}

/// Publish a scan allow-list atomically with cancellation/supersession checks.
/// Unmanaged callers retain their existing behavior; no new paths are authorized.
pub fn publish(work: impl FnOnce()) {
    CURRENT.with(|current| {
        let current = current.borrow();
        let Some(task) = current.as_ref() else {
            work();
            return;
        };
        let Ok(_tasks) = tasks().lock() else {
            task.cancelled.store(true, Ordering::SeqCst);
            return;
        };
        let Ok(latest) = latest().lock() else {
            task.cancelled.store(true, Ordering::SeqCst);
            return;
        };
        if task.cancelled.load(Ordering::SeqCst)
            || task
                .scope
                .is_some_and(|scope| latest.get(scope) != Some(&task.id))
        {
            task.cancelled.store(true, Ordering::SeqCst);
            return;
        }
        work();
    });
}

/// A task can be claimed once. Pending cancellation also rejects a late worker.
pub struct CommitPermit(());

/// Final session metadata commit is serialized with cancellation. Never used by deletion.
pub fn commit<T>(work: impl FnOnce(&CommitPermit) -> Result<T, String>) -> Result<T, String> {
    let task = CURRENT.with(|current| current.borrow().clone());
    let Some(task) = task else {
        return work(&CommitPermit(()));
    };
    let _tasks = tasks().lock().map_err(|_| "scan:state")?;
    let latest = latest().lock().map_err(|_| "scan:state")?;
    if task.cancelled.load(Ordering::SeqCst)
        || task
            .scope
            .is_some_and(|scope| latest.get(scope) != Some(&task.id))
    {
        return Err("scan:cancelled".into());
    }
    let result = work(&CommitPermit(()));
    if result.is_ok() {
        task.committed.store(true, Ordering::SeqCst);
    }
    result
}

pub fn run<T>(id: u32, work: impl FnOnce() -> T) -> Result<T, String> {
    let task = {
        let mut tasks = tasks().lock().map_err(|_| "scan:state")?;
        let task = tasks.get_mut(&id).ok_or("scan:cancelled")?;
        let task_mut = Arc::get_mut(task).ok_or("scan:busy")?;
        if task_mut.started {
            return Err("scan:busy".into());
        }
        task_mut.started = true;
        task.clone()
    };
    CURRENT.with(|c| *c.borrow_mut() = Some(task.clone()));
    struct Guard(u32);
    impl Drop for Guard {
        fn drop(&mut self) {
            CURRENT.with(|c| *c.borrow_mut() = None);
            if let Ok(mut tasks) = tasks().lock() {
                tasks.remove(&self.0);
            }
        }
    }
    let _guard = Guard(id);
    if task.cancelled.load(Ordering::SeqCst) {
        return Err("scan:cancelled".into());
    }
    let result = work();
    if task.cancelled.load(Ordering::SeqCst) {
        Err("scan:cancelled".into())
    } else {
        Ok(result)
    }
}

pub fn cancelled() -> bool {
    CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|t| t.cancelled.load(Ordering::SeqCst))
    })
}

pub fn checkpoint(stage: &str, found: usize) -> bool {
    CURRENT.with(|c| {
        if let Some(task) = c.borrow().as_ref() {
            if let Ok(mut p) = task.progress.lock() {
                *p = Progress {
                    stage: stage.into(),
                    found,
                };
            }
        }
    });
    !cancelled()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_commit_and_cancel_have_a_single_winner() {
        let id = begin().unwrap();
        assert_eq!(
            run(id, || {
                commit(|_| Ok(())).unwrap();
                assert!(!cancel(id).unwrap());
                "committed"
            }),
            Ok("committed")
        );
        let id = begin().unwrap();
        assert_eq!(
            run(id, || {
                assert!(cancel(id).unwrap());
                assert_eq!(
                    commit::<()>(|_| panic!("committed after cancellation")),
                    Err("scan:cancelled".into())
                );
            }),
            Err("scan:cancelled".into())
        );
    }

    #[test]
    fn cancelled_scan_cannot_publish_authorization() {
        let id = begin().unwrap();
        let result = run(id, || {
            cancel(id).unwrap();
            publish(|| panic!("published partial scan"));
        });
        assert_eq!(result, Err("scan:cancelled".into()));
    }

    #[test]
    fn newer_scope_scan_blocks_older_publication_but_other_scopes_stay_valid() {
        let old = begin().unwrap();
        let result = run_scoped(old, "test-scoped-publication", || {
            std::thread::spawn(|| {
                let new = begin().unwrap();
                run_scoped(new, "test-scoped-publication", || {
                    let mut published = false;
                    publish(|| published = true);
                    assert!(published);
                })
                .unwrap();
            })
            .join()
            .unwrap();
            publish(|| panic!("late old scan overwrote authorization"));
        });
        assert_eq!(result, Err("scan:cancelled".into()));
        let other = begin().unwrap();
        run_scoped(other, "test-independent-scope", || {
            let mut published = false;
            publish(|| published = true);
            assert!(published);
        })
        .unwrap();
    }

    #[test]
    fn pending_cancel_rejects_late_worker_without_running_it() {
        let id = begin().unwrap();
        cancel(id).unwrap();
        assert_eq!(
            run(id, || panic!("cancelled worker ran")),
            Err::<(), _>("scan:cancelled".into())
        );
        assert!(progress(id).is_none());
    }

    #[test]
    fn cancellation_is_local_and_partial_result_is_rejected() {
        let a = begin().unwrap();
        let b = begin().unwrap();
        assert_eq!(
            run(a, || {
                assert!(checkpoint("files", 3));
                assert_eq!(progress(a).unwrap().found, 3);
                cancel(a).unwrap();
                assert!(!checkpoint("registry", 3));
                "partial"
            }),
            Err("scan:cancelled".into())
        );
        assert!(!cancelled());
        assert_eq!(
            run(b, || {
                assert!(!cancelled());
                "complete"
            }),
            Ok("complete")
        );
        assert!(progress(a).is_none());
        assert!(progress(b).is_none());
    }

    #[test]
    fn panic_releases_task_and_thread_context() {
        let id = begin().unwrap();
        assert!(std::panic::catch_unwind(|| run(id, || panic!("worker"))).is_err());
        assert!(!cancelled());
        assert!(progress(id).is_none());
    }
}
