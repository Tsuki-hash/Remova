//! Portable-app candidates (M4, read-only).
//!
//! A firewall rule names an executable that still exists, yet no installed
//! application claims that path — the shape of a portable app running from
//! an arbitrary folder. Cross-references two existing sources only; no new
//! probing beyond an existence check. Nothing here deletes or writes.

use serde::{Deserialize, Serialize};
use std::path::Path;

const RESULT_CAP: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableCandidate {
    /// Executable path from the firewall rule (exists on disk).
    pub exe_path: String,
    /// Firewall rule display name that revealed the path.
    pub rule_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableScanResult {
    pub candidates: Vec<PortableCandidate>,
    #[serde(default)]
    pub omitted: u32,
}

/// Pure cross-reference: the exe lives outside every install root. Paths are
/// compared case-insensitively at component boundaries.
fn claimed_by_install(exe_path: &str, install_roots: &[String]) -> bool {
    let exe = exe_path.to_lowercase();
    install_roots.iter().any(|root| {
        let r = root
            .trim()
            .to_lowercase()
            .trim_end_matches('\\')
            .to_string();
        !r.is_empty() && (exe.starts_with(&format!("{r}\\")) || exe == r)
    })
}

/// Scan firewall rules for live executables no install record claims.
pub fn scan_portable_candidates() -> PortableScanResult {
    let apps = crate::apps::scan_installed_apps();
    let roots: Vec<String> = apps
        .iter()
        .map(|a| a.install_location.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let mut out = PortableScanResult::default();
    for (value_name, raw) in crate::regscan::list_values(crate::firewall::FIREWALL_RULES_KEY) {
        if out.candidates.len() >= RESULT_CAP {
            out.omitted += 1;
            break;
        }
        // Reuse the firewall parser for the App= field.
        let Some(rule) = crate::firewall::parse_rule_public(&value_name, &raw) else {
            continue;
        };
        let exe = rule.app_path;
        if exe.is_empty() {
            continue;
        }
        if !crate::fsutil::is_local_drive_path(&exe) {
            continue;
        }
        if !Path::new(&exe).is_file() {
            continue;
        }
        if claimed_by_install(&exe, &roots) {
            continue;
        }
        out.candidates.push(PortableCandidate {
            exe_path: exe,
            rule_name: rule.rule_name,
        });
    }
    out.candidates.sort_by_key(|c| c.exe_path.to_lowercase());
    out.candidates
        .dedup_by(|a, b| a.exe_path.eq_ignore_ascii_case(&b.exe_path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_matching_respects_boundaries() {
        let roots = vec![r"C:\Program Files\App".to_string()];
        assert!(claimed_by_install(
            r"C:\Program Files\App\bin\x.exe",
            &roots
        ));
        assert!(!claimed_by_install(
            r"C:\Program Files\Appx\bin\x.exe",
            &roots
        ));
        assert!(!claimed_by_install(r"D:\Portables\x.exe", &roots));
        // Some install records point straight at the exe; equality claims it.
        assert!(claimed_by_install(r"C:\Program Files\App", &roots));
    }
}
