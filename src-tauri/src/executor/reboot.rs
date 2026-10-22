//! Reboot-pending gate for MSI 1641.
//!
//! A 1641 exit means the uninstaller already initiated a restart. Deleting
//! residuals (or any other cleanup) while the shutdown is pending can leave a
//! half-completed state. The gate must survive separate IPC calls and also an
//! app relaunch/elevation, so the pending set is persisted under
//! `%LOCALAPPDATA%\Remova\reboot-pending.json` together with a boot signature.
//! A real reboot changes the boot signature and the stale record is discarded.
//!
//! In unit tests the gate is thread-local and never touches disk; persistence
//! helpers are tested directly with a temp path.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub(crate) const INITIATED_MESSAGE: &str =
    "official uninstaller initiated reboot; residual cleanup skipped";
pub(crate) const PENDING_MESSAGE: &str = "system reboot pending; residual cleanup postponed";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct PendingState {
    boot_epoch: u64,
    uptime_ms: u64,
    #[serde(default)]
    app_keys: Vec<String>,
}

#[cfg(test)]
thread_local! {
    static TEST_PENDING: std::cell::RefCell<HashSet<String>> =
        std::cell::RefCell::new(HashSet::new());
}

#[cfg(not(test))]
fn memory() -> &'static Mutex<HashSet<String>> {
    static PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashSet::new()))
}

#[cfg(not(test))]
fn state_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("Remova")
            .join("reboot-pending.json"),
    )
}

#[cfg(not(test))]
fn uptime_ms() -> u64 {
    #[cfg(windows)]
    {
        unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
    }
    #[cfg(not(windows))]
    {
        0
    }
}

#[cfg(not(test))]
fn boot_signature() -> (u64, u64) {
    let uptime = uptime_ms();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (now.saturating_sub(uptime / 1000), uptime)
}

fn state_is_stale_with(
    stored_epoch: u64,
    stored_uptime: u64,
    current_epoch: u64,
    current_uptime: u64,
) -> bool {
    // Uptime going backwards is the strongest reboot signal; the epoch check
    // also catches a reboot after a clock adjustment.
    current_uptime.saturating_add(120_000) < stored_uptime
        || current_epoch.abs_diff(stored_epoch) > 600
}

fn load_state_from(path: &Path) -> Option<PendingState> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_state_to(path: &Path, state: &PendingState) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    crate::fsutil::write_bytes_atomic(path, &bytes)
}

#[cfg(not(test))]
fn hydrate_from_disk() -> bool {
    let Some(path) = state_path() else {
        return false;
    };
    let Some(state) = load_state_from(&path) else {
        return false;
    };
    let (epoch, uptime) = boot_signature();
    if state_is_stale_with(state.boot_epoch, state.uptime_ms, epoch, uptime) {
        let _ = std::fs::remove_file(&path);
        return false;
    }
    if state.app_keys.is_empty() {
        return false;
    }
    if let Ok(mut set) = memory().lock() {
        set.extend(state.app_keys);
    }
    true
}

#[cfg(not(test))]
fn persist() {
    #[cfg(not(test))]
    {
        let Some(path) = state_path() else {
            return;
        };
        let Ok(set) = memory().lock() else {
            return;
        };
        let (epoch, uptime) = boot_signature();
        let mut keys: Vec<String> = set.iter().cloned().collect();
        keys.sort();
        let state = PendingState {
            boot_epoch: epoch,
            uptime_ms: uptime,
            app_keys: keys,
        };
        if let Err(e) = save_state_to(&path, &state) {
            // The in-memory gate still protects the current process.
            eprintln!("could not persist reboot-pending state: {e}");
        }
    }
}

pub(crate) fn mark_pending(app_key: &str) {
    if app_key.is_empty() {
        return;
    }
    #[cfg(test)]
    {
        // Thread-local gate: test processes must not see each other's 1641s.
        TEST_PENDING.with(|pending| {
            pending.borrow_mut().insert(app_key.to_string());
        });
    }
    #[cfg(not(test))]
    {
        if let Ok(mut set) = memory().lock() {
            set.insert(app_key.to_string());
        }
        persist();
    }
}

/// Any 1641 in this session blocks every cleanup until a reboot, not just the
/// app that reported it: a pending shutdown makes any deletion unsafe.
pub(crate) fn any_pending() -> bool {
    #[cfg(test)]
    {
        TEST_PENDING.with(|pending| !pending.borrow().is_empty())
    }
    #[cfg(not(test))]
    {
        if memory().lock().map(|set| !set.is_empty()).unwrap_or(false) {
            return true;
        }
        hydrate_from_disk()
    }
}

#[cfg(test)]
pub(crate) fn is_pending(app_key: &str) -> bool {
    TEST_PENDING.with(|pending| pending.borrow().contains(app_key))
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    TEST_PENDING.with(|pending| pending.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn pending_gate_blocks_every_cleanup_not_only_the_marked_app() {
        let _g = guard();
        reset_for_test();
        mark_pending("source\u{0}key\u{0}AppA");
        assert!(any_pending(), "a 1641 must be global, not app-key scoped");
        assert!(is_pending("source\u{0}key\u{0}AppA"));
        assert!(!is_pending("source\u{0}key\u{0}AppB"));
        reset_for_test();
        assert!(!any_pending());
    }

    #[test]
    fn boot_signature_change_discards_the_pending_record() {
        assert!(state_is_stale_with(1_000, 500_000, 1_000, 10_000));
        assert!(state_is_stale_with(1_000, 500_000, 5_000, 500_100));
        assert!(!state_is_stale_with(1_000, 10_000, 1_020, 20_000));
    }

    #[test]
    fn pending_state_round_trips_through_the_state_file() {
        let tmp = crate::test_support::TempDir::new("reboot-state");
        let path = tmp.path().join("reboot-pending.json");
        let state = PendingState {
            boot_epoch: 42,
            uptime_ms: 900_000,
            app_keys: vec!["a".into(), "b".into()],
        };
        save_state_to(&path, &state).unwrap();
        assert_eq!(load_state_from(&path), Some(state));
    }
}
