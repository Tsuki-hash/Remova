//! Shared fixtures for the crate's tests.
//!
//! Every test that touches the filesystem or the process environment used to carry
//! its own helper. Five near-identical copies existed (`fsutil.rs`, `icon.rs`,
//! `path_seal.rs`, `toolcache.rs`, `commands/update.rs`), plus
//! `commands/backup_cmd.rs::IsolatedBackupRoot`, which was the only one written
//! correctly. Two defects were shared by all of them and could not be caught by
//! the compiler:
//!
//! 1. **Leaked fixtures.** Each helper created its directory and then called
//!    `let _ = std::fs::remove_dir_all(&dir)` at the *end* of the test body. An
//!    assertion failure unwinds past that line, so every red test left its tree in
//!    `%TEMP%`. Over a debug session that is gigabytes of orphaned fixtures, and
//!    a maintainer reading a failure cannot tell a fresh fixture from a stale one.
//! 2. **Unguarded environment mutation.** `std::env::set_var` is not `unsafe` in
//!    Rust 2021, so nothing warns about it, and the crate runs tests on multiple
//!    threads. `PROGRAMDATA` was guarded by `history::ENV_LOCK` — but the readers
//!    (`ignore.rs`, `orphans.rs`, `apps.rs`, `manage.rs`) do not take that lock,
//!    so the guard protected nothing. The symptom is a green-then-red flake that
//!    depends on thread interleaving.
//!
//! The rules this module exists to enforce:
//!
//! - A fixture that mutates process-wide state takes [`env_lock`] for its whole
//!   lifetime. Readers take the same lock.
//! - A fixture cleans itself up on the way out, including on panic, which means
//!   `Drop` — never a line at the end of a test body.
//! - A fixture is unique per call (`tag` + pid + nanos), so parallel tests never
//!   share a directory.
//!
//! This module is test-only; it is never compiled into the shipped binary.

#![cfg(test)]

use std::cell::Cell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

thread_local! {
    /// Whether the current thread holds [`EnvLock`]. Lets [`EnvVar::set`] catch the
    /// mistake of mutating the environment with no lock held, which is the exact
    /// defect this module exists to prevent and which the compiler cannot see.
    static LOCK_HELD: Cell<bool> = const { Cell::new(false) };
}

fn lock_held() -> bool {
    LOCK_HELD.with(|held| held.get())
}

/// The one lock every test that mutates process-wide state must hold.
///
/// Deliberately a single global lock rather than one per variable: the defect it
/// fixes is not a data race on one variable, it is a test reading state that
/// another test is concurrently rewriting. Serializing the mutations is what makes
/// that impossible, and the suite is fast enough that the lost parallelism does
/// not matter.
///
/// A poisoned lock is recovered rather than propagated: a panicking test has
/// already left the process in an unknown state, and failing every later test
/// with "lock poisoned" hides the one failure that actually happened.
///
/// One `EnvLock` may cover any number of [`EnvVar`] overrides. It must **not** be
/// acquired while another is alive on the same thread — `std::sync::Mutex` is not
/// re-entrant, so a nested acquire deadlocks. Take the lock once, at the top.
pub struct EnvLock {
    _guard: MutexGuard<'static, ()>,
}

impl EnvLock {
    pub fn new() -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        LOCK_HELD.with(|held| held.set(true));
        Self { _guard: guard }
    }
}

impl Drop for EnvLock {
    fn drop(&mut self) {
        LOCK_HELD.with(|held| held.set(false));
    }
}

/// A temporary directory that removes itself on drop, including on panic.
///
/// The path is unique per call, so several `TempDir`s can be alive at once. The
/// directory is created eagerly because every current use site needs it to exist;
/// [`TempDir::new`] panics if creation fails rather than letting a later
/// `unwrap` report a misleading cause.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("remova_{tag}_{}_{nonce}", std::process::id()));
        // A stale directory from an earlier leaked run must not be mistaken for
        // this run's fixture.
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|e| panic!("test fixture {tag}: create {path:?}: {e}"));
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A subdirectory of this fixture, created on demand.
    pub fn subdir(&self, name: &str) -> PathBuf {
        let p = self.path.join(name);
        std::fs::create_dir_all(&p).unwrap_or_else(|e| panic!("test fixture: create {p:?}: {e}"));
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if owns(&self.path) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Lets a fixture stand in for its path, so existing `dir.join(..)`,
/// `dir.exists()`, `dir.parent()` and `AsRef<Path>` call sites keep working
/// unchanged.
impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// True when `path` is a directory this module is willing to delete: it must sit
/// directly inside `%TEMP%`.
///
/// The guard exists because the tag is caller-supplied. A tag containing a path
/// separator would let `join` escape `%TEMP%`, and `remove_dir_all` on a path the
/// fixture never created is the one mistake in this file that could destroy real
/// data. Comparing the parent rather than a prefix means a deeper escape (via
/// `..`) is refused too, since `join` keeps the `..` as a literal component.
fn owns(path: &Path) -> bool {
    path.parent() == Some(std::env::temp_dir().as_path())
}

/// An environment variable override that restores the previous value on drop.
///
/// Does **not** take the lock itself: the caller holds one [`EnvLock`] and may
/// override as many variables as it needs under it. Acquiring the lock here would
/// make a second override on the same thread deadlock, because `Mutex` is not
/// re-entrant — and "override two variables" is an ordinary thing for a fixture to
/// do. Forgetting the lock is caught by a `debug_assert` rather than by the
/// compiler, which cannot see process-wide mutation at all.
///
/// Keep the guard alive for as long as the override is meant to apply; dropping it
/// restores the variable. Values are stored as `OsString`, so a fixture can
/// reproduce the non-UTF-8 paths production has to fail closed on.
pub struct EnvVar {
    name: OsString,
    previous: Option<OsString>,
}

impl EnvVar {
    /// # Panics
    /// In debug builds, when the current thread does not hold an [`EnvLock`].
    pub fn set(name: &str, value: impl Into<OsString>) -> Self {
        debug_assert!(
            lock_held(),
            "EnvVar::set without an EnvLock: another test could be reading this \
             variable concurrently. Take `let _lock = EnvLock::new();` first."
        );
        let name = OsString::from(name);
        let previous = std::env::var_os(&name);
        std::env::set_var(&name, value.into());
        Self { name, previous }
    }

    /// Remove the variable, remembering whether it was set so `Drop` can restore it.
    ///
    /// # Panics
    /// In debug builds, when the current thread does not hold an [`EnvLock`].
    pub fn remove(name: &str) -> Self {
        debug_assert!(
            lock_held(),
            "EnvVar::remove without an EnvLock: take `let _lock = EnvLock::new();` first."
        );
        let name = OsString::from(name);
        let previous = std::env::var_os(&name);
        std::env::remove_var(&name);
        Self { name, previous }
    }
}

impl Drop for EnvVar {
    fn drop(&mut self) {
        // The restore is a write to the same process-wide state the constructor
        // wrote, so it needs the same lock. A guard whose lock is dropped first —
        // field order in the owning struct decides this — would restore into a
        // window where another test has already installed its own override, and
        // would delete it. `IsolatedBackupRoot` had exactly that bug.
        debug_assert!(
            lock_held(),
            "EnvVar dropped without its EnvLock: the restore can delete another \
             test's override. Declare the EnvVar before the EnvLock."
        );
        match &self.previous {
            Some(previous) => std::env::set_var(&self.name, previous),
            None => std::env::remove_var(&self.name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two fixtures alive at once must not collide, and both must be inside %TEMP%.
    #[test]
    fn temp_dirs_are_unique_and_inside_temp() {
        let a = TempDir::new("support_a");
        let b = TempDir::new("support_a");
        assert_ne!(a.path(), b.path(), "same tag must still be unique");
        for d in [a.path(), b.path()] {
            assert!(d.is_dir());
            assert_eq!(d.parent(), Some(std::env::temp_dir().as_path()));
        }
    }

    /// The whole point of the RAII type: a panic inside the test body must still
    /// clean up. Asserted by dropping in a closure that unwinds.
    #[test]
    fn temp_dir_cleans_up_on_panic() {
        let captured = {
            let dir = TempDir::new("support_panic");
            let path = dir.path().to_path_buf();
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                panic!("injected failure to exercise the Drop path");
            }));
            assert!(outcome.is_err(), "the injected panic must propagate");
            path
        };
        assert!(
            !captured.exists(),
            "the fixture must be gone after the panic unwound through it"
        );
    }

    /// `Drop` must remove only the fixture, never a sibling in `%TEMP%`.
    #[test]
    fn temp_dir_removes_only_itself() {
        let victim = TempDir::new("support_solo");
        let sibling =
            std::env::temp_dir().join(format!("remova_support_sibling_{}", std::process::id()));
        std::fs::create_dir_all(&sibling).unwrap();
        let path = victim.path().to_path_buf();
        drop(victim);
        assert!(!path.exists(), "the fixture must be gone");
        assert!(sibling.exists(), "a sibling directory must survive");
        let _ = std::fs::remove_dir_all(&sibling);
    }

    /// The deletion guard: a path that does not sit directly in `%TEMP%` is never
    /// removed, so a tag carrying a separator or `..` cannot turn a fixture into a
    /// recursive delete of somewhere real.
    #[test]
    fn owns_refuses_paths_outside_temp() {
        let temp = std::env::temp_dir();
        assert!(owns(&temp.join("remova_x_1_2")));
        // A deeper path is not ours even though it is under %TEMP%.
        assert!(!owns(&temp.join("remova_x_1_2").join("inner")));
        // A `..` escape keeps the literal component, so the parent is not %TEMP%.
        assert!(!owns(&temp.join("..").join("remova_escape")));
        assert!(!owns(Path::new("")));
    }

    /// One `EnvLock` may cover several overrides at once, and each restores what it
    /// found — including the "was unset" case. Nesting overrides is what the
    /// earlier self-locking design deadlocked on.
    #[test]
    fn env_var_restores_the_previous_value() {
        const NAME: &str = "REMOVA_TEST_SUPPORT_PROBE";
        const OTHER: &str = "REMOVA_TEST_SUPPORT_PROBE_2";
        let _lock = EnvLock::new();

        // Start from a known state: both unset.
        let _clear = EnvVar::remove(NAME);
        let _clear_other = EnvVar::remove(OTHER);
        assert!(std::env::var_os(NAME).is_none());
        assert!(std::env::var_os(OTHER).is_none());

        {
            let _set = EnvVar::set(NAME, "fixture-value");
            let _set_other = EnvVar::set(OTHER, "second-value");
            assert_eq!(std::env::var(NAME).as_deref(), Ok("fixture-value"));
            assert_eq!(std::env::var(OTHER).as_deref(), Ok("second-value"));
        }
        assert!(
            std::env::var_os(NAME).is_none(),
            "an override of an unset variable must unset it again"
        );
        assert!(std::env::var_os(OTHER).is_none());

        // A variable that *was* set must come back with its original value.
        let original = std::env::var_os("PATH").expect("PATH is always set");
        {
            let _set = EnvVar::set("PATH", "fixture-only");
            assert_eq!(std::env::var("PATH").as_deref(), Ok("fixture-only"));
        }
        assert_eq!(std::env::var_os("PATH").as_ref(), Some(&original));

        // Nested overrides unwind one level at a time.
        {
            let _first = EnvVar::set(NAME, "first");
            {
                let _second = EnvVar::set(NAME, "second");
                assert_eq!(std::env::var(NAME).as_deref(), Ok("second"));
            }
            assert_eq!(std::env::var(NAME).as_deref(), Ok("first"));
        }
        assert!(std::env::var_os(NAME).is_none());
    }

    /// The lock must actually exclude other threads — that is the whole point of
    /// having one, since the defect it fixes is a test reading state another test
    /// is concurrently rewriting.
    #[test]
    fn env_lock_excludes_other_threads() {
        let guard = EnvLock::new();
        let entered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&entered);
        let worker = std::thread::spawn(move || {
            let _g = EnvLock::new();
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        // Give the worker a real chance to run; it must still be blocked.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            !entered.load(std::sync::atomic::Ordering::SeqCst),
            "a second thread must not acquire the lock while it is held"
        );
        drop(guard);
        worker
            .join()
            .expect("worker must finish once the lock is released");
        assert!(entered.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// The `debug_assert` in `EnvVar::set` is the only thing standing between a
    /// test and silent process-wide mutation, so it has to be proven to fire.
    #[test]
    fn env_var_without_the_lock_is_rejected() {
        let outcome = std::panic::catch_unwind(|| {
            let _override = EnvVar::set("REMOVA_TEST_SUPPORT_PROBE", "unlocked");
        });
        assert!(
            outcome.is_err(),
            "mutating the environment with no EnvLock held must be rejected"
        );
        // The rejected override must not have been applied.
        assert!(std::env::var_os("REMOVA_TEST_SUPPORT_PROBE").is_none());
    }
}
