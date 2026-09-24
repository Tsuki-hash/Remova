//! Orphan leftover directory scan (P2-2) 鈥?no matching installed app.

use crate::apps::InstalledApp;
use crate::scanner::{CleanupItem, Confidence, Evidence, ItemKind, RiskLevel, SCORE_SUSPECTED_MIN};

fn slug_tokens(name: &str) -> Vec<String> {
    crate::scanner::slugify(name)
}

/// Heuristic stats for orphan candidate dirs (feeds judgment evidence).
fn dir_shape(p: &std::path::Path) -> (usize, bool, bool) {
    let Ok(rd) = std::fs::read_dir(p) else {
        return (0, false, false);
    };
    let mut files = 0;
    let mut has_exe = false;
    let mut has_config = false;
    for e in rd.flatten().take(40) {
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n.ends_with(".exe") || n.ends_with(".msi") {
            has_exe = true;
        }
        if n.ends_with(".dll")
            || n.ends_with(".dat")
            || n.ends_with(".db")
            || n.ends_with(".json")
            || n.ends_with(".ini")
            || n.ends_with(".xml")
        {
            has_config = true;
        }
        if e.path().is_file() {
            files += 1;
        }
    }
    (files, has_exe, has_config)
}

/// Heuristic: directory looks like an app leftover if it has an exe or many files.
fn looks_like_app_dir(p: &std::path::Path) -> bool {
    let (files, has_exe, _) = dir_shape(p);
    has_exe || files >= 3
}

fn scan_root_label(path: &str) -> Option<&'static str> {
    let p = path.replace('/', "\\").to_lowercase();
    if p.contains("\\program files (x86)\\") {
        return Some("Program Files (x86)");
    }
    if p.contains("\\program files\\") {
        return Some("Program Files");
    }
    if p.contains("\\appdata\\local\\") || p.contains("\\appdata\\roaming\\") {
        return Some("AppData");
    }
    if p.contains("\\programdata\\") {
        return Some("ProgramData");
    }
    None
}

fn last_write_age_days(p: &std::path::Path) -> Option<i64> {
    let meta = std::fs::metadata(p).ok()?;
    let modified = meta.modified().ok()?;
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .ok()?
        .as_secs()
        / 86_400;
    Some(age as i64)
}

/// Path prefix with segment boundary (REV-BE-06): `C:\App` must not own `C:\AppEvil`.
fn path_same_or_under(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches('\\');
    let b = b.trim_end_matches('\\');
    if a == b {
        return true;
    }
    match (a.strip_prefix(b), b.strip_prefix(a)) {
        (Some(rest), _) | (_, Some(rest)) => rest.starts_with('\\'),
        (None, None) => false,
    }
}

fn match_installed(installed: &[InstalledApp], dir: &std::path::Path) -> bool {
    let d = dir.to_string_lossy().replace('/', "\\").to_lowercase();
    let leaf = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for app in installed {
        let loc = app.install_location.replace('/', "\\").to_lowercase();
        if !loc.is_empty() && path_same_or_under(&d, &loc) {
            return true;
        }
        if leaf.is_empty() {
            continue;
        }
        let tokens = slug_tokens(&app.name);
        if tokens
            .iter()
            .any(|t| t.len() >= 4 && leaf.contains(t.as_str()))
        {
            return true;
        }
    }
    false
}

pub fn scan_orphans(installed: &[InstalledApp]) -> Vec<CleanupItem> {
    let mut roots: Vec<std::path::PathBuf> = vec![];
    for env in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
        "PROGRAMDATA",
    ] {
        if let Ok(v) = std::env::var(env) {
            roots.push(std::path::PathBuf::from(v));
        }
    }
    let mut out = Vec::new();
    let mut scanned: std::collections::HashSet<String> = std::collections::HashSet::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            // skip obvious system / installer cache folders (FUNC-8)
            if matches!(
                name.to_lowercase().as_str(),
                "windows"
                    | "windowsapps"
                    | "common files"
                    | "microsoft"
                    | "reference assemblies"
                    | "package cache"
                    | "installshield installation information"
                    | "windows kits"
                    | "internet explorer"
                    | "windows defender"
                    | "windows security"
                    | "windows powershell"
                    | "dotnet"
                    | "msbuild"
                    | "nuget"
            ) {
                continue;
            }
            if match_installed(installed, &p) {
                continue;
            }
            // AR-04: honor ignore path rules for orphan candidates.
            if crate::ignore::should_skip_leftover_path(
                &crate::ignore::load(),
                &p.to_string_lossy(),
            ) {
                continue;
            }
            if !looks_like_app_dir(&p) {
                continue;
            }
            let score = SCORE_SUSPECTED_MIN + 10;
            let (files, has_exe, has_config) = dir_shape(&p);
            let mut evidence = vec![Evidence {
                code: "orphan_no_owner".into(),
                label: "No matching uninstall entry".into(),
                weight: score,
                detail: format!("folder `{name}` not matched to installed software"),
            }];
            if has_exe {
                evidence.push(Evidence {
                    code: "orphan_has_exe".into(),
                    label: "Contains executable".into(),
                    weight: 10,
                    detail: "found .exe/.msi under folder".into(),
                });
            } else if files >= 3 {
                evidence.push(Evidence {
                    code: "orphan_many_files".into(),
                    label: "Multi-file app-like folder".into(),
                    weight: 5,
                    detail: format!("{files} files in top level"),
                });
            }
            if has_config {
                evidence.push(Evidence {
                    code: "orphan_has_config".into(),
                    label: "Contains config/data files".into(),
                    weight: 4,
                    detail: "dll/dat/db/json/ini/xml present".into(),
                });
            }
            if let Some(root) = scan_root_label(&p.to_string_lossy()) {
                evidence.push(Evidence {
                    code: "orphan_root".into(),
                    label: "Install-root location".into(),
                    weight: 2,
                    detail: root.into(),
                });
            }
            if let Some(days) = last_write_age_days(&p) {
                evidence.push(Evidence {
                    code: "orphan_mtime".into(),
                    label: "Last write age".into(),
                    weight: 0,
                    detail: format!("{days} day(s) ago"),
                });
            }
            // Orphans stay Suspected/Medium 鈥?never auto-select; user must confirm.
            let path_str = p.to_string_lossy().to_string();
            scanned.insert(path_str.clone());
            out.push(CleanupItem {
                path: path_str,
                kind: ItemKind::Dir,
                score,
                confidence: Confidence::Suspected,
                risk: RiskLevel::Medium,
                reason: "Orphan app-like folder (no matching uninstall entry)".into(),
                evidence,
                shared: false,
                user_data: false,
                user_library: false,
                size_kb: None,
                bucket: None,
            });
            if out.len() >= crate::constants::ORPHAN_RESULT_CAP {
                crate::scanner::fill_item_sizes(&mut out);
                crate::scanner::fill_item_buckets(&mut out, "");
                remember_orphan_paths(&scanned);
                return out;
            }
        }
    }
    crate::scanner::fill_item_sizes(&mut out);
    crate::scanner::fill_item_buckets(&mut out, "");
    remember_orphan_paths(&scanned);
    out
}

/// Server-side allow-list of paths from the latest `scan_orphans` (S-R6-01).
static ORPHAN_PATHS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

fn orphan_paths() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    ORPHAN_PATHS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// F-R7-02: normalize allow-list keys so case / trailing `\` / slash style cannot bypass.
fn normalize_orphan_key(path: &str) -> String {
    path.replace('/', "\\")
        .trim()
        .trim_matches('"')
        .trim_end_matches('\\')
        .to_lowercase()
}

fn remember_orphan_paths(paths: &std::collections::HashSet<String>) {
    if let Ok(mut g) = orphan_paths().lock() {
        *g = paths.iter().map(|p| normalize_orphan_key(p)).collect();
    }
}

/// True when `path` came from the most recent orphan scan in this process.
pub fn was_recent_orphan_path(path: &str) -> bool {
    let key = normalize_orphan_key(path);
    orphan_paths()
        .lock()
        .map(|g| g.contains(&key))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::InstalledApp;

    fn app(name: &str, loc: &str) -> InstalledApp {
        InstalledApp {
            name: name.into(),
            version: String::new(),
            publisher: String::new(),
            install_location: loc.into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKCU".into(),
            registry_key: "k".into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        }
    }

    #[test]
    fn match_installed_by_path() {
        let installed = [app("Foo", r"C:\Program Files\FooApp")];
        let p = std::path::Path::new(r"C:\Program Files\FooApp");
        assert!(match_installed(&installed, p));
        let p2 = std::path::Path::new(r"C:\Program Files\OtherThing");
        assert!(!match_installed(&installed, p2));
    }

    #[test]
    fn match_installed_requires_path_segment_boundary() {
        let installed = [app("App", r"C:\Program Files\App")];
        // Sibling that merely shares a string prefix must not match (REV-BE-06).
        let evil = std::path::Path::new(r"C:\Program Files\AppEvil");
        assert!(!match_installed(&installed, evil));
        let child = std::path::Path::new(r"C:\Program Files\App\bin");
        assert!(match_installed(&installed, child));
    }

    #[test]
    fn orphan_evidence_builder_includes_no_owner_and_shape() {
        let tmp = std::env::temp_dir().join(format!("remova_orphan_ev_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("app.exe"), b"mz").unwrap();
        std::fs::write(tmp.join("config.json"), b"{}").unwrap();
        assert!(looks_like_app_dir(&tmp));
        let (files, has_exe, has_config) = dir_shape(&tmp);
        assert!(has_exe);
        assert!(has_config);
        assert!(files >= 2);
        let path_str = r"C:\Program Files\FakeVendor\Leaf".to_string();
        assert_eq!(scan_root_label(&path_str), Some("Program Files"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn orphan_allow_list_normalizes_case_and_trailing_slash() {
        // F-R7-02: case / trailing `\` / slash style must not bypass the allow-list.
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(r"C:\Program Files\SomeVendor\App".to_string());
        remember_orphan_paths(&set);
        assert!(was_recent_orphan_path(r"C:\Program Files\SomeVendor\App"));
        assert!(was_recent_orphan_path(r"c:\program files\somevendor\app"));
        assert!(was_recent_orphan_path(r"C:\Program Files\SomeVendor\App\"));
        assert!(was_recent_orphan_path(r"C:/Program Files/SomeVendor/App"));
        assert!(!was_recent_orphan_path(r"C:\Program Files\Other\App"));
        assert!(!was_recent_orphan_path(r"C:\Program Files\SomeVendor"));
    }

    #[cfg(windows)]
    #[test]
    fn scan_orphans_runs() {
        let apps = crate::apps::scan_installed_apps();
        let items = scan_orphans(&apps);
        for it in &items {
            assert_eq!(it.kind, ItemKind::Dir);
            assert!(it.path.len() > 3);
            assert!(
                !it.evidence.is_empty(),
                "orphan items must carry judgment evidence"
            );
            assert!(it
                .evidence
                .iter()
                .any(|e| e.code == "orphan_no_owner" || e.code == "orphan_dir"));
        }
    }
}
