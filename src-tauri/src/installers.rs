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

fn push_file_item(
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    path: &Path,
    bucket: &str,
) {
    if out.len() >= SPECIALTY_RESULT_CAP {
        return;
    }
    // Non-UTF-8 never becomes a delete candidate (fail-closed).
    let Some(p) = crate::fsutil::path_utf8(path) else {
        return;
    };
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
        user_library: crate::safety::is_user_library_path(&p)
            && !crate::safety::is_user_data_path(&p),
        size_kb: None,
        bucket: Some(bucket.into()),
    });
    scanned.insert(p);
}

fn push_dir_item(
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    path: &Path,
    bucket: &str,
) {
    if out.len() >= SPECIALTY_RESULT_CAP {
        return;
    }
    let Some(p) = crate::fsutil::path_utf8(path) else {
        return;
    };
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
        user_library: crate::safety::is_user_library_path(&p)
            && !crate::safety::is_user_data_path(&p),
        size_kb: None,
        bucket: Some(bucket.into()),
    });
    scanned.insert(p);
}

fn scan_downloads(out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>) {
    if !crate::scan_task::checkpoint("files", out.len()) {
        return;
    }
    let Some(dir) = user_downloads() else { return };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    for ent in rd.flatten().take(400) {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return;
        }
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

fn is_updater_dir_name(name: &str) -> bool {
    name.to_ascii_lowercase()
        .strip_suffix("-updater")
        .is_some_and(|prefix| !prefix.is_empty())
}

fn scan_updater_dirs(out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>) {
    let Some(local) = user_local_appdata() else {
        return;
    };
    scan_updater_dirs_from(&local, out, scanned);
}

fn scan_updater_dirs_from(local: &Path, out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>) {
    if !crate::scan_task::checkpoint("caches", out.len()) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(local) else {
        return;
    };
    for vendor in rd.flatten().take(200) {
        if !crate::scan_task::checkpoint("caches", out.len()) {
            return;
        }
        let vpath = vendor.path();
        if !vpath.is_dir() {
            continue;
        }

        if vendor.file_name().to_str().is_some_and(is_updater_dir_name) {
            push_dir_item(out, scanned, &vpath, "updater_cache");
            if out.len() >= SPECIALTY_RESULT_CAP {
                return;
            }
        }

        for leaf in [
            "updater",
            "updates",
            "packages",
            "installcache",
            "squirreltemp",
        ] {
            if crate::scan_task::cancelled() {
                return;
            }
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
    if !crate::scan_task::checkpoint("sizes", out.len()) {
        return out;
    }
    fill_item_sizes(&mut out);
    if !crate::scan_task::checkpoint("finalizing", out.len()) {
        return out;
    }
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
    fn updater_dir_name_matches_known_suffix_only() {
        for name in [
            "perfectworldarena-updater",
            "@deepseek-aidsh-desktop-updater",
            "obsidian-UPDATER",
            "token-monitor-UpDaTeR",
        ] {
            assert!(is_updater_dir_name(name), "{name}");
        }
        for name in [
            "updater",
            "-updater",
            "ordinary-directory",
            "updater-backup",
            "product-updater-extra",
        ] {
            assert!(!is_updater_dir_name(name), "{name}");
        }
    }

    #[test]
    fn updater_scan_finds_suffix_roots_and_existing_cache_leaves() {
        let _allow = crate::scan_allow::test_lock();
        let root = std::env::temp_dir().join(format!(
            "remova-installer-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        for name in [
            "perfectworldarena-updater",
            "@deepseek-aidsh-desktop-updater",
            "obsidian-UPDATER",
            "token-monitor-UpDaTeR",
        ] {
            let dir = root.join(name);
            std::fs::create_dir_all(dir.join("payload")).expect("create updater fixture");
            std::fs::write(dir.join("payload").join("package.nupkg"), b"fixture")
                .expect("write updater fixture");
        }
        let vendor = root.join("Application Data");
        for leaf in [
            "updater",
            "updates",
            "packages",
            "installcache",
            "squirreltemp",
        ] {
            std::fs::create_dir_all(vendor.join(leaf)).expect("create cache leaf");
        }
        std::fs::create_dir(root.join("ordinary-directory")).expect("create ordinary fixture");
        std::fs::write(root.join("token-monitor-updater.txt"), b"not a directory")
            .expect("create non-directory fixture");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        scan_updater_dirs_from(&root, &mut out, &mut scanned);

        let paths = out
            .iter()
            .map(|item| item.path.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(out.len(), 9);
        assert_eq!(paths.len(), out.len());
        for name in [
            "perfectworldarena-updater",
            "@deepseek-aidsh-desktop-updater",
            "obsidian-UPDATER",
            "token-monitor-UpDaTeR",
        ] {
            let path = crate::fsutil::path_utf8(&root.join(name)).expect("utf8 fixture path");
            let item = out
                .iter()
                .find(|item| item.path == path)
                .expect("suffix root");
            assert_eq!(item.kind, ItemKind::Dir);
            assert_eq!(item.bucket.as_deref(), Some("updater_cache"));
            assert_eq!(item.confidence, Confidence::Suspected);
            assert_eq!(item.risk, RiskLevel::Medium);
            assert_eq!(item.evidence[0].code, "updater_cache");
        }
        assert!(out.iter().all(|item| !item.path.ends_with("package.nupkg")));
        assert!(out
            .iter()
            .all(|item| !item.path.ends_with("ordinary-directory")));

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    #[test]
    fn scan_runs_without_panic() {
        let _allow = crate::scan_allow::test_lock();
        let items = scan_installer_caches();
        // May be empty on CI; must not panic and must stay under cap.
        assert!(items.len() <= SPECIALTY_RESULT_CAP);
    }
}
