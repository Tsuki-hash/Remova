//! Installer / updater-cache scan (进阶) — packages and update blobs, not app leftover dirs.
//! Distinct from orphan leftovers: objects are installers and updater caches.

use crate::constants::SPECIALTY_RESULT_CAP;
use crate::scan_allow::{self, AllowScope};
use crate::scanner::{
    fill_item_buckets, fill_item_sizes, CleanupItem, Confidence, Evidence, ItemKind, RiskLevel,
    SCORE_CONFIRMED, SCORE_SUSPECTED_MIN,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Installer package extensions. `.zip` / bare `.exe` are too broad for personal Downloads.
const PKG_EXTS: &[&str] = &[".msi", ".msix", ".appx", ".appxbundle"];

fn user_downloads() -> Option<PathBuf> {
    let profile = std::env::var_os("USERPROFILE")?;
    Some(PathBuf::from(profile).join("Downloads"))
}

fn user_local_appdata() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local))
}

fn looks_like_setup_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("setup")
        || n.contains("install")
        || n.contains("update")
        || n.contains("redist")
        || n.contains("vcredist")
        || n.contains("dotnet")
        || n.contains("jdk")
        || n.contains("node-v")
}

/// Dedicated package extensions, or `.exe` only when the name looks like an installer.
fn ext_is_pkg(name: &str) -> bool {
    let lower = name.to_lowercase();
    if PKG_EXTS.iter().any(|e| lower.ends_with(e)) {
        return true;
    }
    lower.ends_with(".exe") && looks_like_setup_name(&lower)
}

fn push_file_item(out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>, path: &Path, bucket: &str) {
    if out.len() >= SPECIALTY_RESULT_CAP {
        return;
    }
    let p = path.to_string_lossy().to_string();
    let risk = if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("msi"))
        .unwrap_or(false)
    {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };
    let mut evidence = vec![Evidence {
        code: "installer_package".into(),
        label: "installer".into(),
        weight: 40,
        detail: bucket.into(),
    }];
    if looks_like_setup_name(&p) {
        evidence.push(Evidence {
            code: "installer_setup_name".into(),
            label: "setup-like".into(),
            weight: 10,
            detail: String::new(),
        });
    }
    out.push(CleanupItem {
        path: p.clone(),
        kind: ItemKind::File,
        score: SCORE_CONFIRMED,
        confidence: Confidence::Confirmed,
        risk,
        reason: "installer package".into(),
        evidence,
        shared: false,
        user_data: crate::safety::is_user_data_path(&p),
        user_library: crate::safety::is_user_library_path(&p) && !crate::safety::is_user_data_path(&p),
        size_kb: None,
        bucket: Some(bucket.into()),
    });
    scanned.insert(p);
}

fn push_dir_item(out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>, path: &Path, bucket: &str) {
    if out.len() >= SPECIALTY_RESULT_CAP {
        return;
    }
    let p = path.to_string_lossy().to_string();
    out.push(CleanupItem {
        path: p.clone(),
        kind: ItemKind::Dir,
        score: SCORE_SUSPECTED_MIN + 20,
        confidence: Confidence::Suspected,
        risk: RiskLevel::Medium,
        reason: "updater cache".into(),
        evidence: vec![Evidence {
            code: "updater_cache".into(),
            label: "updater".into(),
            weight: 35,
            detail: bucket.into(),
        }],
        shared: false,
        user_data: crate::safety::is_user_data_path(&p),
        user_library: crate::safety::is_user_library_path(&p) && !crate::safety::is_user_data_path(&p),
        size_kb: None,
        bucket: Some(bucket.into()),
    });
    scanned.insert(p);
}

fn scan_downloads(out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>) {
    let Some(dir) = user_downloads() else { return };
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    for ent in rd.flatten().take(400) {
        let p = ent.path();
        if !p.is_file() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if !ext_is_pkg(&name) {
            continue;
        }
        // Skip tiny files — likely not full installers.
        if let Ok(meta) = ent.metadata() {
            if meta.len() < 8 * 1024 * 1024 && !looks_like_setup_name(&name) {
                continue;
            }
        }
        push_file_item(out, scanned, &p, "download_pkg");
        if out.len() >= SPECIALTY_RESULT_CAP {
            return;
        }
    }
}

fn scan_updater_dirs(out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>) {
    let Some(local) = user_local_appdata() else { return };
    let Ok(rd) = std::fs::read_dir(&local) else { return };
    for vendor in rd.flatten().take(200) {
        let vpath = vendor.path();
        if !vpath.is_dir() {
            continue;
        }
        for leaf in ["updater", "updates", "packages", "installcache", "squirreltemp"] {
            let cand = vpath.join(leaf);
            if cand.is_dir() {
                push_dir_item(out, scanned, &cand, "updater_cache");
                if out.len() >= SPECIALTY_RESULT_CAP {
                    return;
                }
            }
        }
    }
}

/// Scan installer packages + updater caches. Results feed the installer allow-list.
pub fn scan_installer_caches() -> Vec<CleanupItem> {
    let mut out = Vec::new();
    let mut scanned = HashSet::new();
    scan_downloads(&mut out, &mut scanned);
    scan_updater_dirs(&mut out, &mut scanned);
    fill_item_sizes(&mut out);
    fill_item_buckets(&mut out, "");
    scan_allow::remember(AllowScope::Installer, &scanned);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_is_pkg_matches_installers() {
        assert!(ext_is_pkg("App.Setup.MSI"));
        assert!(ext_is_pkg("node-v20.msi"));
        assert!(!ext_is_pkg("notes.txt"));
        // bare exe / zip must not match — personal Downloads
        assert!(!ext_is_pkg("photos.zip"));
        assert!(!ext_is_pkg("game.exe"));
        assert!(ext_is_pkg("vcredist_x64.exe"));
        assert!(ext_is_pkg("App-Setup.exe"));
    }

    #[test]
    fn scan_runs_without_panic() {
        let items = scan_installer_caches();
        // May be empty on CI; must not panic and must stay under cap.
        assert!(items.len() <= SPECIALTY_RESULT_CAP);
    }
}
