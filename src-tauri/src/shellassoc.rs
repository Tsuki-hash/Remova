//! Shell-association residue (M4E.5, read-only) — the classes an official
//! uninstaller commonly leaves behind, all keyed by a full executable path so
//! attribution is precise:
//!
//! - `HKCU\Software\Classes\Applications\<exe>.exe` (Open-with ghosts)
//! - URL-protocol keys whose `shell\open\command` points at a missing exe
//! - `AppCompatFlags\Compatibility Assistant\Store` values named by full path
//!
//! Nothing here deletes: the gate stays closed this cycle, exactly like the
//! M4 system-trace scans. Deletion would need per-class policy evaluation
//! (a URL protocol can be re-registered by a still-installed product).

use serde::{Deserialize, Serialize};

const RESULT_CAP: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellAssocOrphan {
    /// `applications` | `url_protocol` | `compat_store`
    pub class: String,
    /// Registry path of the key (value data path for compat_store).
    pub key: String,
    /// The executable path whose file is gone.
    pub exe_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShellAssocScanResult {
    pub orphans: Vec<ShellAssocOrphan>,
    #[serde(default)]
    pub omitted: u32,
    #[serde(default)]
    pub completeness: crate::scan_contract::ScanCompleteness,
}

/// The exe path an `open command` string refers to, judged only for local
/// fixed drives (a UNC probe from an elevated process is an SMB hazard).
fn orphan_command_exe(command: &str) -> Option<String> {
    let exe = crate::cliresidue::command_exe_path(command)?;
    let exe = exe.trim_matches('"').trim().to_string();
    if exe.is_empty() || exe.contains('%') || !crate::fsutil::is_local_drive_path(&exe) {
        return None;
    }
    std::path::Path::new(&exe)
        .try_exists()
        .ok()
        .filter(|exists| !exists)
        .map(|_| exe)
}

fn orphan_value_path(value_name: &str) -> Option<String> {
    let p = value_name.trim().trim_matches('"');
    if p.is_empty() || p.contains('%') || !crate::fsutil::is_local_drive_path(p) {
        return None;
    }
    std::path::Path::new(p)
        .try_exists()
        .ok()
        .filter(|exists| !exists)
        .map(|_| p.to_string())
}

/// `HKCU\Software\Classes\Applications\<exe>.exe` — the `shell\open\command`
/// default value names the full path of the registered exe.
fn scan_applications(out: &mut Vec<ShellAssocOrphan>, failed: &mut Vec<String>) {
    const ROOT: &str = r"HKCU\Software\Classes\Applications";
    for key in crate::regscan::list_subkeys(ROOT) {
        let command_key = format!("{ROOT}\\{key}\\shell\\open\\command");
        let Some(command) = crate::regscan::read_string(&command_key, "") else {
            continue;
        };
        if let Some(exe) = orphan_command_exe(&command) {
            if out.len() >= RESULT_CAP {
                break;
            }
            out.push(ShellAssocOrphan {
                class: "applications".into(),
                key: command_key,
                exe_path: exe,
            });
        }
    }
    let _ = failed;
}

/// `HKCU\Software\Classes\<proto>` with a `URL Protocol` value; the open
/// command's exe decides orphan-ness.
fn scan_url_protocols(out: &mut Vec<ShellAssocOrphan>, failed: &mut Vec<String>) {
    const ROOT: &str = r"HKCU\Software\Classes";
    for key in crate::regscan::list_subkeys(ROOT) {
        let proto_key = format!("{ROOT}\\{key}");
        // Cheap pre-filter: only keys that carry the URL Protocol marker.
        if crate::regscan::read_string(&proto_key, "URL Protocol").is_none() {
            continue;
        }
        let command_key = format!("{proto_key}\\shell\\open\\command");
        let Some(command) = crate::regscan::read_string(&command_key, "") else {
            continue;
        };
        if let Some(exe) = orphan_command_exe(&command) {
            if out.len() >= RESULT_CAP {
                break;
            }
            out.push(ShellAssocOrphan {
                class: "url_protocol".into(),
                key: command_key,
                exe_path: exe,
            });
        }
    }
    let _ = failed;
}

/// `AppCompatFlags\Compatibility Assistant\Store` — value NAMES are full exe
/// paths with empty data.
fn scan_compat_store(out: &mut Vec<ShellAssocOrphan>, failed: &mut Vec<String>) {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store";
    let (_, values) = match crate::regscan::snapshot_names(KEY) {
        Ok(pair) => pair,
        Err(e) if e == "scan:cancelled" => return,
        Err(_) => {
            failed.push(KEY.into());
            return;
        }
    };
    for value_name in values {
        if out.len() >= RESULT_CAP {
            break;
        }
        if let Some(path) = orphan_value_path(&value_name) {
            out.push(ShellAssocOrphan {
                class: "compat_store".into(),
                key: KEY.into(),
                exe_path: path,
            });
        }
    }
}

pub fn scan_shell_assoc_orphans() -> ShellAssocScanResult {
    let mut result = ShellAssocScanResult::default();
    if crate::scan_task::cancelled() {
        result.completeness = crate::scan_contract::ScanCompleteness::cancelled(0);
        return result;
    }
    let mut failed = Vec::new();
    result.completeness.roots_scanned = 3;
    scan_applications(&mut result.orphans, &mut failed);
    if crate::scan_task::cancelled() {
        result.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
        return result;
    }
    scan_url_protocols(&mut result.orphans, &mut failed);
    if crate::scan_task::cancelled() {
        result.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
        return result;
    }
    scan_compat_store(&mut result.orphans, &mut failed);
    if failed.is_empty() {
        result.completeness.status = crate::scan_contract::ScanStatus::Complete;
    } else {
        result.completeness.status = crate::scan_contract::ScanStatus::Partial;
        result.completeness.failed_roots = failed;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_orphan_judgement_rejects_env_unc_and_existing() {
        assert_eq!(
            orphan_command_exe(r#""C:\Gone\app.exe" "%*""#),
            Some(r"C:\Gone\app.exe".into())
        );
        assert_eq!(orphan_command_exe(r#""C:\Windows\explorer.exe""#), None);
        assert_eq!(orphan_command_exe(r#""\\server\share\app.exe""#), None);
        assert_eq!(orphan_command_exe(r#""%ProgramFiles%\x.exe""#), None);
        assert_eq!(orphan_command_exe(""), None);
    }

    #[test]
    fn value_orphan_judgement_needs_a_local_missing_path() {
        assert_eq!(
            orphan_value_path(r"C:\Gone\app.exe"),
            Some(r"C:\Gone\app.exe".into())
        );
        assert_eq!(orphan_value_path(r"C:\Windows\explorer.exe"), None);
        assert_eq!(orphan_value_path("argon"), None);
    }
}
