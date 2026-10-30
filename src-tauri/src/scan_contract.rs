//! Shared completeness contract for bounded read-only scans.
//!
//! An empty item list is not enough to prove a scan was complete: a root may
//! have been unreadable, a cap may have dropped rows, or cancellation may have
//! stopped the walk. These fields travel with scanner results so the UI and
//! exported evidence can distinguish those cases.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    #[default]
    Complete,
    Partial,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanCompleteness {
    pub status: ScanStatus,
    /// Number of roots whose enumeration completed or produced a result.
    #[serde(default)]
    pub roots_scanned: u32,
    /// Root labels that could not be read. Never expose an empty result as clean
    /// when this list is non-empty.
    #[serde(default)]
    pub failed_roots: Vec<String>,
}

impl ScanCompleteness {
    pub fn complete() -> Self {
        Self {
            status: ScanStatus::Complete,
            roots_scanned: 0,
            failed_roots: Vec::new(),
        }
    }

    pub fn partial(roots_scanned: u32, failed_roots: Vec<String>) -> Self {
        Self {
            status: ScanStatus::Partial,
            roots_scanned,
            failed_roots,
        }
    }

    pub fn cancelled(roots_scanned: u32) -> Self {
        Self {
            status: ScanStatus::Cancelled,
            roots_scanned,
            failed_roots: Vec::new(),
        }
    }

    pub fn failed(root: String) -> Self {
        Self {
            status: ScanStatus::Failed,
            roots_scanned: 0,
            failed_roots: vec![root],
        }
    }
}
