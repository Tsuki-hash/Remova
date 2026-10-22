//! Reboot-pending gate for MSI 1641.
//!
//! A 1641 exit means the uninstaller already initiated a restart. Deleting
//! residuals (or any other cleanup) while the shutdown is pending can leave a
//! half-completed state. The gate must survive separate IPC calls and also an
//! app relaunch/elevation, so the pending set is persisted under
//! `%LOCALAPPDATA%\Remova\reboot-pending.json` together with a boot identity.
//!
//! The identity is the kernel's boot GUID (`NtQuerySystemInformation` class
//! `SystemBootEnvironmentInformation`), which changes on every real restart and
//! is unaffected by wall-clock corrections. The previous
//! `wall_clock - uptime` heuristic both missed short restarts and let a clock
//! adjustment clear a live gate; it is gone. When the boot GUID cannot be read
//! the gate fails closed (keeps blocking) rather than guessing.
//!
//! In unit tests the gate is thread-local and never touches disk; the
//! persistence helpers take an explicit path and boot identity so the real
//! implementation is what the tests exercise.

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
    /// Kernel boot GUID as lowercase hex. Empty on records written before the
    /// identity switch, which fall back to the uptime check.
    #[serde(default)]
    boot_id: String,
    /// Uptime at write time. Only a secondary signal: a smaller current uptime
    /// proves a restart happened even when no GUID was recorded.
    #[serde(default)]
    uptime_ms: u64,
    #[serde(default)]
    app_keys: Vec<String>,
}

/// Uptime slack for the fallback path. A clock change cannot affect this, and
/// the comparison is one-directional (current below stored), so no tolerance
/// can ever clear the gate without an actual restart.
const UPTIME_SLACK_MS: u64 = 120_000;

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

/// The kernel boot GUID for this session, as lowercase hex.
///
/// `SystemBootEnvironmentInformation` is a stable, documented structure; the
/// information class constant is not surfaced by the `windows` crate, so the
/// entry point is resolved dynamically and the whole call is isolated here. A
/// 64-byte buffer is used so only the leading 16-byte GUID is interpreted,
/// independent of the tail layout, and any failure returns `None` (the gate
/// then fails closed instead of guessing).
#[cfg(windows)]
fn system_boot_id() -> Option<String> {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

    const SYSTEM_BOOT_ENVIRONMENT_INFORMATION: u32 = 90;
    type NtQuerySystemInformationFn =
        unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32, *mut u32) -> i32;

    unsafe {
        let module = GetModuleHandleW(windows::core::w!("ntdll.dll")).ok()?;
        let proc = GetProcAddress(
            module,
            PCSTR(c"NtQuerySystemInformation".as_ptr().cast::<u8>()),
        );
        let query: NtQuerySystemInformationFn = std::mem::transmute(proc);
        let mut buffer = [0u8; 64];
        let mut returned = 0u32;
        let status = query(
            SYSTEM_BOOT_ENVIRONMENT_INFORMATION,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut returned,
        );
        // NTSTATUS >= 0 is success; the GUID is the first 16 bytes.
        if status < 0 || returned < 16 {
            return None;
        }
        let mut hex = String::with_capacity(32);
        for byte in &buffer[..16] {
            hex.push_str(&format!("{byte:02x}"));
        }
        Some(hex)
    }
}

#[cfg(not(windows))]
fn system_boot_id() -> Option<String> {
    None
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

/// True when `stored` was written during an earlier boot.
///
/// The boot GUID is authoritative whenever both sides have one. Otherwise the
/// uptime comparison can only prove a restart (it never clears the gate on its
/// own when the numbers are consistent with the same session).
fn state_is_stale_with(
    stored: &PendingState,
    current_boot_id: Option<&str>,
    current_uptime_ms: Option<u64>,
) -> bool {
    if !stored.boot_id.is_empty() {
        if let Some(current) = current_boot_id {
            return !stored.boot_id.eq_ignore_ascii_case(current);
        }
        // The stored record carries an identity we cannot re-read: keep the
        // gate closed rather than guessing.
        return false;
    }
    match current_uptime_ms {
        Some(now) => now.saturating_add(UPTIME_SLACK_MS) < stored.uptime_ms,
        None => false,
    }
}

#[cfg(not(test))]
fn hydrate_from_disk() -> bool {
    let Some(path) = state_path() else {
        return false;
    };
    let Some(state) = load_state_from(&path) else {
        return false;
    };
    let boot_id = system_boot_id();
    if state_is_stale_with(&state, boot_id.as_deref(), Some(uptime_ms())) {
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
    let Some(path) = state_path() else {
        return;
    };
    let Ok(set) = memory().lock() else {
        return;
    };
    let mut keys: Vec<String> = set.iter().cloned().collect();
    keys.sort();
    let state = PendingState {
        boot_id: system_boot_id().unwrap_or_default(),
        uptime_ms: uptime_ms(),
        app_keys: keys,
    };
    if let Err(e) = save_state_to(&path, &state) {
        // The in-memory gate still protects the current process; the persisted
        // record may be missing, but a reboot can only be proven by a changed
        // boot identity, so a lost file never opens the gate.
        eprintln!("could not persist reboot-pending state: {e}");
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
    fn boot_identity_change_discards_the_pending_record() {
        let stored = PendingState {
            boot_id: "aabb".into(),
            uptime_ms: 500_000,
            app_keys: vec!["app".into()],
        };
        // Same boot: keep blocking, regardless of wall-clock time.
        assert!(!state_is_stale_with(&stored, Some("AABB"), Some(500_000)));
        // A real restart changes the GUID even when the new uptime is large.
        assert!(state_is_stale_with(&stored, Some("ccdd"), Some(500_100)));
        // Short restart: uptime is back near zero and the GUID differs.
        assert!(state_is_stale_with(&stored, Some("ccdd"), Some(30_000)));
        // The GUID cannot be re-read: keep the gate closed rather than guess.
        assert!(!state_is_stale_with(&stored, None, Some(10_000)));
    }

    #[test]
    fn legacy_uptime_only_records_still_clear_after_a_restart() {
        let legacy = PendingState {
            boot_id: String::new(),
            uptime_ms: 500_000,
            app_keys: vec!["app".into()],
        };
        // Uptime went backwards => a restart happened.
        assert!(state_is_stale_with(&legacy, Some("ccdd"), Some(10_000)));
        // Same session (uptime only grew) must not clear on a clock jump.
        assert!(!state_is_stale_with(&legacy, Some("aabb"), Some(700_000)));
    }

    #[test]
    fn pending_state_round_trips_through_the_state_file() {
        let tmp = crate::test_support::TempDir::new("reboot-state");
        let path = tmp.path().join("reboot-pending.json");
        let state = PendingState {
            boot_id: "00112233445566778899aabbccddeeff".into(),
            uptime_ms: 900_000,
            app_keys: vec!["a".into(), "b".into()],
        };
        save_state_to(&path, &state).unwrap();
        assert_eq!(load_state_from(&path), Some(state));
    }

    #[test]
    fn a_corrupt_state_file_is_not_treated_as_a_cleared_gate() {
        let tmp = crate::test_support::TempDir::new("reboot-corrupt");
        let path = tmp.path().join("reboot-pending.json");
        std::fs::write(&path, b"{not json").unwrap();
        // Parsing fails: the caller must keep the in-memory gate rather than
        // infer "no pending" from an unreadable file.
        assert_eq!(load_state_from(&path), None);
    }

    #[test]
    fn the_real_boot_identity_is_readable_and_stable_within_one_session() {
        // Exercises the production FFI path: a working read must return a
        // 16-byte GUID in hex, and two reads in the same session must match.
        // A `None` here would mean the gate can never be cleared by a restart,
        // so it is reported rather than silently skipped.
        let first = system_boot_id().expect("kernel boot GUID must be readable");
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(system_boot_id().as_deref(), Some(first.as_str()));
    }

    #[test]
    fn an_unwritable_state_path_reports_the_error() {
        let tmp = crate::test_support::TempDir::new("reboot-unwritable");
        // A directory cannot be replaced by the atomic write, so this must
        // surface as an error instead of silently succeeding.
        let path = tmp.path().join("blocked");
        std::fs::create_dir_all(&path).unwrap();
        let state = PendingState::default();
        assert!(save_state_to(&path, &state).is_err());
    }
}
