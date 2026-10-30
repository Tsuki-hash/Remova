//! System-trace scans (M4) — evaluation-gated, read-only.
//!
//! The MuiCache-shaped class passed the evaluation and is implemented here:
//! ConsentStore `NonPackaged` entries name an executable path as the value
//! name, so an orphan (exe gone, local fixed drive, not a resource string)
//! can be matched precisely, firewall-style. The deletion gate stays closed
//! this cycle per plan — read-only reporting only.
//!
//! Evaluation record for the classes that did NOT pass (kept for the next
//! review, not implementable offline):
//! - Tile: value layout is UWP-package based (`SystemAppData\<pkg>`), no
//!   precise path/value mapping to a desktop app's exe — fails gate 2.
//! - RADAR: structure is not verifiable offline on Windows 10 vs 11 — fails
//!   the "verify the structure per OS" gate; re-evaluate with real machines.
//! - JumpList: files are keyed by AppID hashes; attributing a JumpList file
//!   to one app requires LNK parsing and is not precise — stays out entirely
//!   (user history, not residue; see plan §5).

use serde::{Deserialize, Serialize};

const CONSENT_STORE_ROOT: &str =
    r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";
const RESULT_CAP: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentOrphan {
    /// Capability family (calendars, microphone, …).
    pub capability: String,
    /// Full registry key holding the orphaned value.
    pub key: String,
    /// The value name — the executable path whose file is gone.
    pub exe_path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceScanResult {
    pub consent_orphans: Vec<ConsentOrphan>,
    #[serde(default)]
    pub omitted: u32,
    #[serde(default)]
    pub completeness: crate::scan_contract::ScanCompleteness,
}

/// Pure orphan test, shared with the (future, gated) delete path.
fn is_consent_orphan(exe_path: &str) -> bool {
    let p = exe_path.trim().trim_matches('"');
    if p.is_empty() || p.contains('%') || p.starts_with('@') {
        return false;
    }
    if !crate::fsutil::is_local_drive_path(p) {
        return false;
    }
    std::path::Path::new(p)
        .try_exists()
        .map(|exists| !exists)
        .unwrap_or(false)
}

/// Orphaned ConsentStore `NonPackaged` entries, read-only.
pub fn scan_consent_orphans() -> TraceScanResult {
    let mut out = TraceScanResult::default();
    let capabilities = crate::regscan::list_subkeys(CONSENT_STORE_ROOT);
    if crate::scan_task::cancelled() {
        out.completeness = crate::scan_contract::ScanCompleteness::cancelled(0);
        return out;
    }
    out.completeness.roots_scanned = 1;
    for capability in capabilities {
        if crate::scan_task::cancelled() {
            out.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
            break;
        }
        let non_packaged = format!("{CONSENT_STORE_ROOT}\\{capability}\\NonPackaged");
        let (values, _subkeys) = match crate::regscan::snapshot_names(&non_packaged) {
            Ok(pair) => pair,
            Err(e) if e == "scan:cancelled" => {
                out.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
                break;
            }
            Err(_) => {
                // Capabilities without a NonPackaged child are normal; an
                // unexpected access/enumeration error is incomplete evidence.
                out.completeness.status = crate::scan_contract::ScanStatus::Partial;
                out.completeness.failed_roots.push(non_packaged);
                continue;
            }
        };
        for value_name in values {
            if out.consent_orphans.len() >= RESULT_CAP {
                out.omitted += 1;
                break;
            }
            if !is_consent_orphan(&value_name) {
                continue;
            }
            out.consent_orphans.push(ConsentOrphan {
                capability: capability.clone(),
                key: non_packaged.clone(),
                exe_path: value_name.trim().trim_matches('"').to_string(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orphan_test_rejects_resources_env_and_remote_paths() {
        assert!(is_consent_orphan(r"C:\Tools\gone.exe"));
        assert!(
            !is_consent_orphan(r"C:\Windows\explorer.exe"),
            "exists → not orphan"
        );
        assert!(!is_consent_orphan("@{pkg}"));
        assert!(!is_consent_orphan(r"%ProgramFiles%\x.exe"));
        assert!(!is_consent_orphan(r"\\server\share\gone.exe"));
        assert!(!is_consent_orphan(""));
    }
}
