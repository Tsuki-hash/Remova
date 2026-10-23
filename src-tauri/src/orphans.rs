//! Orphan leftover directory scan (-2) —no matching installed app.

use crate::apps::InstalledApp;
use crate::scanner::{CleanupItem, Confidence, Evidence, ItemKind, RiskLevel, SCORE_SUSPECTED_MIN};

fn slug_tokens(name: &str) -> Vec<String> {
    crate::scanner::slugify(name)
}

/// Heuristic stats for orphan candidate dirs (feeds judgment evidence).
fn dir_shape(p: &std::path::Path) -> (usize, bool, bool) {
    if crate::scan_task::cancelled() {
        return (0, false, false);
    }
    let Ok(rd) = std::fs::read_dir(p) else {
        return (0, false, false);
    };
    let mut files = 0;
    let mut has_exe = false;
    let mut has_config = false;
    for e in rd.flatten().take(40) {
        if crate::scan_task::cancelled() {
            break;
        }
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
        if e.file_type().is_ok_and(|t| t.is_file()) {
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

fn modified_age_days(modified: std::time::SystemTime) -> Option<i64> {
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .ok()?
        .as_secs()
        / 86_400;
    Some(age as i64)
}

fn last_write_age_days(p: &std::path::Path) -> Option<i64> {
    let meta = std::fs::metadata(p).ok()?;
    modified_age_days(meta.modified().ok()?)
}

/// The newest write among the directory and its top-level entries. Writing an
/// existing file does not update the parent directory's mtime on Windows, so a
/// directory-only check misses active logs/config files.
fn recent_write_age_days(p: &std::path::Path) -> Option<i64> {
    let mut newest = last_write_age_days(p);
    let Ok(entries) = std::fs::read_dir(p) else {
        return newest;
    };
    for entry in entries.flatten().take(200) {
        if crate::scan_task::cancelled() {
            break;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if let Some(age) = modified_age_days(modified) {
            newest = Some(newest.map_or(age, |current| current.min(age)));
        }
    }
    newest
}

/// Path prefix with segment boundary : `C:\App` must not own `C:\AppEvil`.
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

struct InstalledMatch {
    loc: String,
    rooted: bool,
    tokens: Vec<String>,
}

fn build_match_index(installed: &[InstalledApp]) -> Vec<InstalledMatch> {
    installed
        .iter()
        .map(|app| {
            let loc = app.install_location.replace('/', "\\").to_lowercase();
            let rooted = loc
                .trim_end_matches('\\')
                .split('\\')
                .filter(|s| !s.is_empty())
                .count()
                >= 2;
            InstalledMatch {
                loc,
                rooted,
                tokens: slug_tokens(&app.name),
            }
        })
        .collect()
}

fn match_installed_index(index: &[InstalledMatch], dir: &std::path::Path) -> bool {
    let d = dir.to_string_lossy().replace('/', "\\").to_lowercase();
    let leaf = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for app in index {
        if crate::scan_task::cancelled() {
            return true;
        }
        let loc_trim = app.loc.trim_end_matches('\\');
        if app.rooted && path_same_or_under(&d, loc_trim) {
            return true;
        }
        if leaf.is_empty() {
            continue;
        }
        if app
            .tokens
            .iter()
            .any(|t| t.len() >= 4 && crate::scanner::slug_boundary_hit(&leaf, t))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
fn match_installed(installed: &[InstalledApp], dir: &std::path::Path) -> bool {
    let d = dir.to_string_lossy().replace('/', "\\").to_lowercase();
    let leaf = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for app in installed {
        if crate::scan_task::cancelled() {
            return true;
        }
        let loc = app.install_location.replace('/', "\\").to_lowercase();
        let loc_trim = loc.trim_end_matches('\\');
        // a broken InstallLocation of a drive root (`C:` / `C:\`)
        // must not claim every path on that drive — require a real depth.
        let rooted = loc_trim.split('\\').filter(|s| !s.is_empty()).count() >= 2;
        if rooted && path_same_or_under(&d, loc_trim) {
            return true;
        }
        if leaf.is_empty() {
            continue;
        }
        let tokens = slug_tokens(&app.name);
        // boundary-anchored like the install-path side — `Notepad`
        // must not claim `NotepadPlusPlus`.
        if tokens
            .iter()
            .any(|t| t.len() >= 4 && crate::scanner::slug_boundary_hit(&leaf, t))
        {
            return true;
        }
    }
    false
}

/// the cap used to be invisible. The UI now says how many candidates
/// the result limit left out.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrphanScanResult {
    pub items: Vec<CleanupItem>,
    #[serde(default)]
    pub omitted: u32,
}

fn orphan_roots() -> Vec<std::path::PathBuf> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{
            FOLDERID_LocalAppData, FOLDERID_ProgramData, FOLDERID_ProgramFiles,
            FOLDERID_ProgramFilesX86,
        };
        [
            &FOLDERID_ProgramFiles,
            &FOLDERID_ProgramFilesX86,
            &FOLDERID_LocalAppData,
            &FOLDERID_ProgramData,
        ]
        .into_iter()
        .filter_map(crate::sysops::known_folder)
        .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

pub fn scan_orphans(installed: &[InstalledApp]) -> OrphanScanResult {
    if crate::scan_task::cancelled() {
        return OrphanScanResult {
            items: Vec::new(),
            omitted: 0,
        };
    }
    // load ignore rules once (was per candidate in the loop).
    let ignore = crate::ignore::load();
    let roots = orphan_roots();
    scan_orphans_in_roots(installed, &ignore, roots)
}

fn scan_orphans_in_roots(
    installed: &[InstalledApp],
    ignore: &crate::ignore::IgnoreList,
    roots: Vec<std::path::PathBuf>,
) -> OrphanScanResult {
    let mut out = Vec::new();
    let mut scanned: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Normalize each installed app once instead of once per candidate.
    let match_index = build_match_index(installed);
    // Collect candidates across every root first, then order and truncate. The
    // old `if out.len() >= ORPHAN_RESULT_CAP { return }` inside the enumeration
    // made the reported subset depend on `read_dir` order, and stopped the scan
    // early on a machine whose first root happened to be full of leftovers.
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    for root in roots {
        if !crate::scan_task::checkpoint("files", candidates.len()) {
            return OrphanScanResult {
                items: out,
                omitted: 0,
            };
        }
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            if !crate::scan_task::checkpoint("files", candidates.len()) {
                return OrphanScanResult {
                    items: out,
                    omitted: 0,
                };
            }
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            // skip obvious system / installer cache folders
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
                    | "temp"
                    | "tmp"
                    | "cache"
                    | "fontcache"
                    | "gpucache"
                    | "shadercache"
                    | "d3dshadercache"
                    | "code cache"
                    | "crashdumps"
                    | "squirreltemp"
            ) {
                continue;
            }
            if match_installed_index(&match_index, &p) {
                continue;
            }
            // honor ignore path rules for orphan candidates.
            if crate::ignore::should_skip_leftover_path(ignore, &p.to_string_lossy()) {
                continue;
            }
            candidates.push(p);
        }
    }
    let ordered = crate::fsutil::order_candidates(candidates);
    let omitted = ordered
        .len()
        .saturating_sub(crate::constants::ORPHAN_RESULT_CAP) as u32;
    for p in ordered
        .into_iter()
        .take(crate::constants::ORPHAN_RESULT_CAP)
    {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return OrphanScanResult {
                items: out,
                omitted,
            };
        }
        // The expensive shape probe runs only for what the cap keeps.
        if !looks_like_app_dir(&p) {
            continue;
        }
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
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
        let write_age_days = recent_write_age_days(&p);
        if let Some(days) = write_age_days {
            evidence.push(Evidence {
                code: "orphan_mtime".into(),
                label: "Last write age".into(),
                weight: 0,
                detail: format!("{days} day(s) ago"),
            });
            // SOP §2.3: a directory still being written is active data, not a
            // leftover. Keep-class (high risk) rather than a hard refusal: the
            // user can still inspect and confirm it explicitly.
            if days < 3 {
                evidence.push(Evidence {
                    code: "orphan_recent".into(),
                    label: "Recently written (active data)".into(),
                    weight: 0,
                    detail: format!("written {days} day(s) ago"),
                });
            }
        }
        // Orphans stay Suspected/Medium —never auto-select; user must confirm.
        let recent = write_age_days.is_some_and(|days| days < 3);
        let path_str = p.to_string_lossy().to_string();
        scanned.insert(path_str.clone());
        out.push(CleanupItem {
            path: path_str,
            kind: ItemKind::Dir,
            score,
            confidence: Confidence::Suspected,
            risk: if recent {
                RiskLevel::High
            } else {
                RiskLevel::Medium
            },
            reason: "Orphan app-like folder (no matching uninstall entry)".into(),
            evidence,
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
    if !crate::scan_task::checkpoint("sizes", out.len()) {
        return OrphanScanResult {
            items: out,
            omitted,
        };
    }
    // Orphan rows otherwise bypass the app analyzer's service-bound marking.
    crate::scanner::reg_scans::mark_service_bound(&mut out);
    crate::scanner::fill_item_sizes(&mut out);
    if !crate::scan_task::checkpoint("finalizing", out.len()) {
        return OrphanScanResult {
            items: out,
            omitted,
        };
    }
    crate::scanner::fill_item_buckets(&mut out, "");
    remember_orphan_paths(&scanned);
    OrphanScanResult {
        items: out,
        omitted,
    }
}

/// Server-side allow-list of paths from the latest `scan_orphans`.
static ORPHAN_PATHS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

fn orphan_paths() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    ORPHAN_PATHS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Normalize an allow-list key so case, trailing `\`, slash style and Win32's
/// per-segment trailing dots/spaces cannot bypass it.
///
/// Delegates to `fsutil::path_key`. This used to be a private copy that was
/// byte-identical to `scan_allow::normalize` and, unlike `safety::path_utf8_lower`,
/// did not strip per-segment trailing dots and spaces — so the orphan allow-list
/// and the red-line checks disagreed about what a path was.
fn normalize_orphan_key(path: &str) -> Option<String> {
    crate::fsutil::path_key(std::path::Path::new(path))
}

fn remember_orphan_paths(paths: &std::collections::HashSet<String>) {
    crate::scan_task::publish(|| {
        if let Ok(mut g) = orphan_paths().lock() {
            // A path that cannot be named is never authorized.
            *g = paths
                .iter()
                .filter_map(|p| normalize_orphan_key(p))
                .collect();
        }
    });
}

/// True when `path` came from the most recent orphan scan in this process.
pub fn was_recent_orphan_path(path: &str) -> bool {
    let Some(key) = normalize_orphan_key(path) else {
        return false;
    };
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

    #[cfg(windows)]
    #[test]
    fn orphan_roots_ignore_environment_overrides() {
        let _g = crate::test_support::EnvLock::new();
        let _local = crate::test_support::EnvVar::set("LOCALAPPDATA", r"D:\");
        let _pf = crate::test_support::EnvVar::set("ProgramFiles", r"D:\");
        let roots = super::orphan_roots();
        assert!(!roots.is_empty(), "Known Folder roots must resolve");
        assert!(roots.iter().all(|p| p.is_absolute()));
        assert!(
            roots.iter().all(|p| p != std::path::Path::new(r"D:\")),
            "a rewritten environment variable must not become a scan root"
        );
    }

    #[test]
    fn match_installed_requires_path_segment_boundary() {
        let installed = [app("App", r"C:\Program Files\App")];
        // Sibling that merely shares a string prefix must not match.
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
        let _guard = crate::scan_allow::test_lock();
        // Case / trailing `\` / slash style must not bypass the allow-list.
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

    /// the orphan allow-list now shares `fsutil::path_key` with the
    /// red-line checks. This used to be a private copy missing Win32's
    /// per-segment trailing dots and spaces, so a reported `…\App.` stored
    /// `…\app.` and the delete request for `…\App` was refused.
    #[test]
    fn orphan_allow_list_matches_win32_on_trailing_dots_and_spaces() {
        let _guard = crate::scan_allow::test_lock();
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(r"C:\Program Files\SomeVendor\App.".to_string());
        remember_orphan_paths(&set);
        assert!(was_recent_orphan_path(r"C:\Program Files\SomeVendor\App"));
        assert!(was_recent_orphan_path(r"c:/program files/somevendor/app "));
        // An unnameable path never authorizes.
        let mut blank = HashSet::new();
        blank.insert("  ".to_string());
        remember_orphan_paths(&blank);
        assert!(!was_recent_orphan_path("  "));
    }

    #[cfg(windows)]
    #[test]
    fn scan_orphans_runs() {
        let _guard = crate::scan_allow::test_lock();
        let apps = crate::apps::scan_installed_inventory();
        let items = scan_orphans(&apps).items;
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

    /// The orphan cap used to be applied while pushing, so which folders were
    /// reported depended on `read_dir` order. Collecting every candidate first
    /// and ordering through `order_candidates` makes the reported subset — and
    /// the allow-list armed from it — reproducible.
    ///
    /// the fixture names make the filesystem's enumeration order the
    /// *reverse* of the sort order. NTFS compares names case-insensitively on
    /// upper-cased code units, so `_` (0x5F) enumerates after `Z` (0x5A);
    /// `order_candidates` sorts lower-cased strings, where `_` (0x5F) comes
    /// before `z` (0x7A). A `take(cap)` applied during enumeration therefore
    /// keeps a subset disjoint from the sorted one. The old `Leftover###`
    /// names enumerated in the same order they sorted, so the test stayed
    /// green even with the ordering reverted.
    #[test]
    fn orphan_cap_selects_a_deterministic_subset() {
        let _guard = crate::scan_allow::test_lock();
        let cap = crate::constants::ORPHAN_RESULT_CAP;
        let root = crate::test_support::TempDir::new("orphan_cap");
        // `cap + 12` orphan-like folders. The `_Anchor` ones sort first but
        // enumerate last; reversing the production ordering flips which subset
        // survives the cap.
        const EARLY_IN_SORT_ORDER: usize = 12;
        let names = (0..EARLY_IN_SORT_ORDER)
            .map(|i| format!("_Anchor{i:03}"))
            .chain((0..cap).map(|i| format!("Zulu{i:03}")));
        for name in names {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("app.exe"), b"mz").unwrap();
        }

        let result = scan_orphans_in_roots(
            &[],
            &crate::ignore::IgnoreList::default(),
            vec![root.path().to_path_buf()],
        );
        let rows = result.items;

        assert_eq!(rows.len(), cap);
        assert_eq!(
            result.omitted, EARLY_IN_SORT_ORDER as u32,
            "candidates the cap dropped must be reported, not silently lost"
        );
        let paths: Vec<&str> = rows.iter().map(|it| it.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_by_key(|p| p.to_lowercase());
        assert_eq!(
            paths, sorted,
            "the reported subset must be in a stable order"
        );
        // The cap keeps the alphabetically-first folders, not whichever subset
        // the filesystem happened to enumerate first.
        assert!(
            paths.iter().any(|p| p.ends_with("_Anchor000")),
            "a folder that sorts first must survive the cap even though NTFS enumerates it last"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("Zulu079")),
            "a folder that sorts last must be the one the cap drops"
        );
        // Every reported row is authorized; a dropped candidate is not.
        for it in &rows {
            assert!(was_recent_orphan_path(&it.path), "{}", it.path);
        }
        assert!(!was_recent_orphan_path(
            &root.join("Zulu079").to_string_lossy()
        ));
    }

    #[test]
    fn recent_write_activity_includes_top_level_files() {
        let root = crate::test_support::TempDir::new("orphan_recent_file");
        let dir = root.join("App");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.log"), b"live").unwrap();
        assert_eq!(super::recent_write_age_days(&dir), Some(0));
    }

    #[test]
    fn recently_written_orphans_are_keep_class() {
        let _guard = crate::scan_allow::test_lock();
        let root = crate::test_support::TempDir::new("orphan_recent");
        let dir = root.join("FreshLeftover");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.exe"), b"mz").unwrap();

        let result = scan_orphans_in_roots(
            &[],
            &crate::ignore::IgnoreList::default(),
            vec![root.path().to_path_buf()],
        );
        let row = result
            .items
            .iter()
            .find(|it| it.path.ends_with("FreshLeftover"))
            .expect("fixture must be reported");
        assert_eq!(
            row.risk,
            RiskLevel::High,
            "a directory written moments ago is active data, not a plain leftover"
        );
        assert!(row.evidence.iter().any(|e| e.code == "orphan_recent"));
    }

    #[test]
    fn hidden_installed_products_never_gain_orphan_authorization() {
        let _guard = crate::scan_allow::test_lock();
        let root = std::env::temp_dir().join(format!(
            "remova-owned-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let product = root.join("OwnedProduct");
        std::fs::create_dir_all(&product).unwrap();
        std::fs::write(product.join("app.exe"), b"mz").unwrap();
        let path = product.to_string_lossy().into_owned();
        let mut owner = app("OwnedProduct", &path);
        owner.publisher = "OwnedVendor".into();
        owner.uninstall_string.clear();
        owner.quiet_uninstall_string.clear();
        let inventory = vec![owner];
        for ignore in [
            crate::ignore::IgnoreList::default(),
            crate::ignore::IgnoreList {
                names: vec!["OwnedProduct".into()],
                ..Default::default()
            },
            crate::ignore::IgnoreList {
                publishers: vec!["OwnedVendor".into()],
                ..Default::default()
            },
        ] {
            let mut visible = inventory.clone();
            crate::apps::filter_visible_apps(&mut visible, &ignore);
            assert!(visible.is_empty());
            let rows = scan_orphans_in_roots(&inventory, &ignore, vec![root.clone()]).items;
            assert!(rows.is_empty());
            assert!(!was_recent_orphan_path(&path));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
