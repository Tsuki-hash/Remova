//! Uninstall command parsing, dry-run preview and the real cleanup pipeline (Phase 2).
//!
//! Split by lifecycle stage: `uninstall` resolves and launches the vendor uninstaller,
//! `preview` is the no-write dry-run, `pipeline` is backup → uninstall → delete. These stages are
//! re-exported here so callers keep using `crate::executor::*` unchanged.

use serde::{Deserialize, Serialize};

#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub(crate) static CLEANUP_LOCK: Mutex<()> = Mutex::new(());
#[cfg(not(test))]
static CLEANUP_CANCELLED: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
thread_local! {
    /// Thread-local in tests so one test's cancellation cannot abort a parallel
    /// cleanup fixture. Production uses the process-wide flag (the cancel IPC
    /// arrives on a different thread).
    static TEST_CLEANUP_CANCELLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Start a new cleanup/restore transaction. Cancellation only aborts at a
/// phase boundary, so the product is never left mid-delete.
pub fn begin_cleanup_control() {
    #[cfg(test)]
    {
        TEST_CLEANUP_CANCELLED.with(|v| v.set(false));
    }
    #[cfg(not(test))]
    {
        CLEANUP_CANCELLED.store(false, Ordering::SeqCst);
    }
}

pub fn cancel_cleanup_operation() {
    #[cfg(test)]
    {
        TEST_CLEANUP_CANCELLED.with(|v| v.set(true));
    }
    #[cfg(not(test))]
    {
        CLEANUP_CANCELLED.store(true, Ordering::SeqCst);
    }
}

pub(crate) fn cleanup_cancelled() -> bool {
    #[cfg(test)]
    {
        TEST_CLEANUP_CANCELLED.with(|v| v.get())
    }
    #[cfg(not(test))]
    {
        CLEANUP_CANCELLED.load(Ordering::SeqCst)
    }
}

mod pipeline;
mod preview;
pub(crate) mod reboot;
mod uninstall;

pub use pipeline::*;
pub use preview::*;
pub use uninstall::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub app_name: String,
    /// Stable scan identity; empty for legacy/synthetic dry runs.
    #[serde(default)]
    pub app_key: String,
    pub dry_run: bool,
    pub uninstall_command: Vec<String>,
    pub uninstall_message: String,
    pub deleted_planned: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
    pub item_details: Vec<ItemDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDetail {
    pub path: String,
    pub kind: String,
    pub status: String,
    pub message: String,
}

/// Lightweight error alias until thiserror is fully adopted.
pub type ExecResult<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCleanupOptions {
    pub dry_run: bool,
    pub skip_official_uninstall: bool,
    pub backup_enabled: bool,
    /// Create a Windows restore point (tests set false to avoid real side effects).
    #[serde(default = "default_true")]
    pub restore_point: bool,
    /// uninstall | orphan | monitor | copilot | installer | toolcache — default uninstall.
    #[serde(default)]
    pub cleanup_source: Option<String>,
}

pub fn cleanup_source_from_opts(opts: &FullCleanupOptions) -> crate::policy::CleanupSource {
    match opts.cleanup_source.as_deref() {
        Some("orphan") => crate::policy::CleanupSource::Orphan,
        Some("monitor") => crate::policy::CleanupSource::Monitor,
        Some("copilot") => crate::policy::CleanupSource::Copilot,
        Some("installer") => crate::policy::CleanupSource::Installer,
        Some("toolcache") => crate::policy::CleanupSource::ToolCache,
        _ => crate::policy::CleanupSource::Uninstall,
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialUninstallResult {
    pub ok: bool,
    pub message: String,
    pub had_command: bool,
    /// The uninstaller reported that it already initiated a reboot (MSI 1641).
    #[serde(default)]
    pub reboot_initiated: bool,
    /// False when the 1641 gate state could not be written to disk. The
    /// in-process gate is still active, but a relaunch would lose it.
    #[serde(default = "default_true")]
    pub reboot_gate_persisted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCleanupReport {
    pub app_name: String,
    /// Stable scan identity, so same-name apps never cross-pair in the UI.
    #[serde(default)]
    pub app_key: String,
    pub dry_run: bool,
    pub backup_dir: String,
    pub uninstall_ok: bool,
    pub uninstall_message: String,
    pub deleted: u32,
    pub failed: u32,
    pub skipped: u32,
    /// Reboot-delayed deletes (not counted in `deleted`).
    #[serde(default)]
    pub delayed: u32,
    /// Sum of `size_kb` for rows this run actually deleted (M5).
    #[serde(default)]
    pub reclaimed_kb: u64,
    /// Sum of `size_kb` for selected rows the gate refused (keep-class).
    #[serde(default)]
    pub kept_kb: u64,
    /// Sum of `size_kb` for deletes scheduled on reboot (not in `deleted`).
    #[serde(default)]
    pub delayed_kb: u64,
    pub aborted: bool,
    /// MSI 1641 / global reboot gate: the uninstall succeeded but deletion was
    /// postponed until the machine restarts.
    #[serde(default)]
    pub reboot_pending: bool,
    /// False when the 1641 gate could not be written to disk, so a relaunch
    /// would lose it. The batch summary must not hide this warning.
    #[serde(default = "default_true")]
    pub reboot_gate_persisted: bool,
    pub restore_point_ok: bool,
    pub restore_point_msg: String,
    pub errors: Vec<String>,
    pub item_details: Vec<ItemDetail>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI reads these over IPC, so the *serialized* shape is the contract.
    /// A field-level assertion on the Rust source cannot catch
    /// `#[serde(skip_serializing)]`, a rename, or a value that never leaves
    /// `false`; this serializes the real structs instead.
    #[test]
    fn report_and_uninstall_results_serialize_the_fields_the_ui_reads() {
        let report = FullCleanupReport {
            app_name: "Fixture".into(),
            app_key: "k".into(),
            dry_run: false,
            backup_dir: String::new(),
            uninstall_ok: true,
            uninstall_message: "ok".into(),
            deleted: 1,
            failed: 2,
            skipped: 3,
            delayed: 4,
            reclaimed_kb: 5,
            kept_kb: 6,
            delayed_kb: 7,
            aborted: true,
            reboot_pending: true,
            reboot_gate_persisted: true,
            restore_point_ok: false,
            restore_point_msg: "msg".into(),
            errors: vec!["e".into()],
            item_details: vec![ItemDetail {
                path: "p".into(),
                kind: "file".into(),
                status: "failed".into(),
                message: "m".into(),
            }],
        };
        let json = serde_json::to_value(&report).unwrap();
        let obj = json.as_object().unwrap();
        // Presence and type of every field the frontend consumes.
        assert_eq!(obj["reboot_pending"], serde_json::json!(true));
        assert_eq!(obj["reboot_gate_persisted"], serde_json::json!(true));
        assert_eq!(obj["aborted"], serde_json::json!(true));
        assert_eq!(obj["uninstall_ok"], serde_json::json!(true));
        assert_eq!(obj["uninstall_message"], serde_json::json!("ok"));
        assert_eq!(obj["deleted"], serde_json::json!(1));
        assert_eq!(obj["failed"], serde_json::json!(2));
        assert_eq!(obj["skipped"], serde_json::json!(3));
        assert_eq!(obj["delayed"], serde_json::json!(4));
        assert_eq!(obj["reclaimed_kb"], serde_json::json!(5));
        assert_eq!(obj["kept_kb"], serde_json::json!(6));
        assert_eq!(obj["delayed_kb"], serde_json::json!(7));
        assert_eq!(obj["app_key"], serde_json::json!("k"));
        assert!(obj["item_details"].is_array());
        assert!(obj["errors"].is_array());

        // `reboot_pending` must survive a round trip, not just serialization.
        let back: FullCleanupReport = serde_json::from_value(json).unwrap();
        assert!(back.reboot_pending && back.aborted && back.uninstall_ok);

        let uninstall = OfficialUninstallResult {
            ok: true,
            message: "m".into(),
            had_command: true,
            reboot_initiated: true,
            reboot_gate_persisted: true,
        };
        let ujson = serde_json::to_value(&uninstall).unwrap();
        assert_eq!(ujson["reboot_initiated"], serde_json::json!(true));
        assert_eq!(ujson["had_command"], serde_json::json!(true));
        let uback: OfficialUninstallResult = serde_json::from_value(ujson).unwrap();
        assert!(uback.reboot_initiated && uback.had_command && uback.ok);

        // A legacy payload without the newer optional fields still parses, so an
        // older report cannot break the UI.
        let legacy: FullCleanupReport = serde_json::from_value(serde_json::json!({
            "app_name": "L", "dry_run": false, "backup_dir": "", "uninstall_ok": false,
            "uninstall_message": "", "deleted": 0, "failed": 0, "skipped": 0,
            "aborted": false, "restore_point_ok": false, "restore_point_msg": "",
            "errors": [], "item_details": []
        }))
        .unwrap();
        assert!(!legacy.reboot_pending);
        assert_eq!(legacy.delayed, 0);
    }

    #[test]
    fn a_new_cleanup_control_clears_a_stale_cancel() {
        cancel_cleanup_operation();
        assert!(cleanup_cancelled());
        begin_cleanup_control();
        assert!(!cleanup_cancelled());
    }
}
