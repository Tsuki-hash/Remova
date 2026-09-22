//! Uninstall command parsing, dry-run preview and the real cleanup pipeline (Phase 2).
//!
//! Split by lifecycle stage: `uninstall` resolves and launches the vendor uninstaller,
//! `preview` is the no-write dry-run, `pipeline` is backup → uninstall → delete. All three are
//! re-exported here so callers keep using `crate::executor::*` unchanged.

use serde::{Deserialize, Serialize};

mod pipeline;
mod preview;
mod uninstall;

pub use pipeline::*;
pub use preview::*;
pub use uninstall::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub app_name: String,
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

/// Lightweight error alias until thiserror is fully adopted (ARCH-5).
pub type ExecResult<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCleanupOptions {
    pub dry_run: bool,
    pub skip_official_uninstall: bool,
    pub backup_enabled: bool,
    /// Create a Windows restore point (tests set false to avoid real side effects).
    #[serde(default = "default_true")]
    pub restore_point: bool,
    /// uninstall | orphan | monitor | copilot — default uninstall (S-R4-07).
    #[serde(default)]
    pub cleanup_source: Option<String>,
}

pub fn cleanup_source_from_opts(opts: &FullCleanupOptions) -> crate::policy::CleanupSource {
    match opts.cleanup_source.as_deref() {
        Some("orphan") => crate::policy::CleanupSource::Orphan,
        Some("monitor") => crate::policy::CleanupSource::Monitor,
        Some("copilot") => crate::policy::CleanupSource::Copilot,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCleanupReport {
    pub app_name: String,
    pub dry_run: bool,
    pub backup_dir: String,
    pub uninstall_ok: bool,
    pub uninstall_message: String,
    pub deleted: u32,
    pub failed: u32,
    pub skipped: u32,
    /// Reboot-delayed deletes (not counted in `deleted`) — S-R4-13.
    #[serde(default)]
    pub delayed: u32,
    pub aborted: bool,
    pub restore_point_ok: bool,
    pub restore_point_msg: String,
    pub errors: Vec<String>,
    pub item_details: Vec<ItemDetail>,
}
