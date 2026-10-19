//! Task-local cancellation for read-only association scans; never used by deletion.
use serde::Serialize;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Serialize)]
pub struct Progress {
    pub stage: String,
    pub found: usize,
}

struct Task {
    id: u32,
    scope: Option<&'static str>,
    /// `Arc` so a worker thread can hold a live view of the flag: the
    /// thread-local `CURRENT` is not visible to a thread spawned inside a scan.
    cancelled: Arc<AtomicBool>,
    committed: AtomicBool,
    started: bool,
    progress: Mutex<Progress>,
    /// Last (stage pointer, stage length, found) published to `progress`.
    /// `checkpoint` is called per root and per item and almost always repeats
    /// the previous call; short-circuiting here avoids a `String` allocation and
    /// a mutex write on the hot path.
    last_stage_ptr: AtomicUsize,
    last_stage_len: AtomicUsize,
    last_found: AtomicUsize,
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
    // `fetch_update` is deprecated (renamed `try_update`), and the rename is not
    // available on every toolchain this crate still builds with — so use the
    // compare_exchange primitive directly. Semantics are unchanged: the previous
    // value is handed out and overflow is an error rather than a wrapped id.
    let id = loop {
        let current = NEXT.load(Ordering::SeqCst);
        match current.checked_add(1) {
            Some(next) => {
                if NEXT
                    .compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    break current;
                }
            }
            None => return Err("scan:state".into()),
        }
    };
    tasks.insert(
        id,
        Arc::new(Task {
            id,
            scope: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            committed: AtomicBool::new(false),
            started: false,
            progress: Mutex::new(Progress {
                stage: "preparing".into(),
                found: 0,
            }),
            last_stage_ptr: AtomicUsize::new(0),
            last_stage_len: AtomicUsize::new(0),
            last_found: AtomicUsize::new(usize::MAX),
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

/// True when this thread is running inside a scan task.
///
/// Size walks use it to tell a batch scan (background) from a direct user request
/// (foreground) when they contend for the global walk slots.
pub fn has_task() -> bool {
    CURRENT.with(|c| c.borrow().is_some())
}

/// A live view of the current task's cancellation flag.
///
/// A thread spawned inside a scan has no thread-local `CURRENT`, so without one
/// of these it can neither observe cancellation nor be counted as part of the
/// scan that spawned it.
#[derive(Clone)]
pub struct CancelWatch(Arc<AtomicBool>);

impl CancelWatch {
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The current task's cancellation flag, or a watch that is never cancelled when
/// this thread runs outside a scan.
pub fn cancel_watch() -> CancelWatch {
    CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .map(|t| CancelWatch(Arc::clone(&t.cancelled)))
            .unwrap_or(CancelWatch(Arc::new(AtomicBool::new(false))))
    })
}

pub fn checkpoint(stage: &str, found: usize) -> bool {
    CURRENT.with(|c| {
        if let Some(task) = c.borrow().as_ref() {
            // Stage names are `&'static str` literals at every call site, so the
            // pointer identifies the stage. A dynamically built equal string
            // merely misses the short-circuit — it is never mistaken for a
            // different stage.
            let ptr = stage.as_ptr() as usize;
            let len = stage.len();
            let unchanged = task.last_stage_ptr.load(Ordering::Relaxed) == ptr
                && task.last_stage_len.load(Ordering::Relaxed) == len
                && task.last_found.load(Ordering::Relaxed) == found;
            if !unchanged {
                if let Ok(mut p) = task.progress.lock() {
                    *p = Progress {
                        stage: stage.into(),
                        found,
                    };
                }
                task.last_stage_ptr.store(ptr, Ordering::Relaxed);
                task.last_stage_len.store(len, Ordering::Relaxed);
                task.last_found.store(found, Ordering::Relaxed);
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

    /// `checkpoint` is called per root and per item. Repeating the previous
    /// (stage, found) must not lose the published progress, and a change must
    /// still land — the short-circuit is a cache, not a filter.
    #[test]
    fn repeated_checkpoint_keeps_progress_and_still_reports_changes() {
        let id = begin().unwrap();
        run(id, || {
            assert!(checkpoint("files", 3));
            assert_eq!(progress(id).unwrap().found, 3);
            // Same stage and count: still observable, still truthy.
            for _ in 0..5 {
                assert!(checkpoint("files", 3));
            }
            assert_eq!(progress(id).unwrap().found, 3);
            assert_eq!(progress(id).unwrap().stage, "files");
            // A new count publishes; a new stage publishes.
            assert!(checkpoint("files", 4));
            assert_eq!(progress(id).unwrap().found, 4);
            assert!(checkpoint("registry", 4));
            assert_eq!(progress(id).unwrap().stage, "registry");
            assert_eq!(progress(id).unwrap().found, 4);
            // Going back to a lower count is a change too.
            assert!(checkpoint("registry", 1));
            assert_eq!(progress(id).unwrap().found, 1);
        })
        .unwrap();
    }

    /// A worker spawned inside a scan has no thread-local task context. It must
    /// still see the scan's cancellation through the watch, or a cancelled scan
    /// keeps a size walk running on a thread the scan no longer controls.
    #[test]
    fn cancel_watch_lets_a_worker_thread_observe_scan_cancellation() {
        let id = begin().unwrap();
        let result = run(id, || {
            let watch = cancel_watch();
            assert!(!watch.cancelled());
            let worker = std::thread::spawn(move || {
                // No thread-local task here — only the watch.
                assert!(!has_task());
                assert!(!cancelled());
                watch.cancelled()
            });
            assert!(!worker.join().unwrap());
            cancel(id).unwrap();
            let watch = cancel_watch();
            let worker = std::thread::spawn(move || watch.cancelled());
            assert!(worker.join().unwrap(), "worker must see the cancellation");
        });
        assert_eq!(result, Err("scan:cancelled".into()));
    }
}
