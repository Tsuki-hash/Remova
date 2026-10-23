//! Filesystem association scans (shortcuts, TEMP, WebView, cross-drive).

use super::*;

/// Red-line flags computed from the real path — hardcoding `false` shipped
/// items inside user libraries as default-selectable (the FE decision logic
/// trusts `user_data`).
fn is_user_data(p: &std::path::Path) -> bool {
    crate::safety::is_user_data_path(&p.to_string_lossy())
}

fn is_user_library(p: &std::path::Path) -> bool {
    crate::safety::is_user_library_path(&p.to_string_lossy())
}

/// Shortcut peek budget : LNK target strings sit near the header.
const SHORTCUT_PEEK_BYTES: usize = 8 * 1024;

/// Top-level entries examined per non-default drive.
///
/// `read_dir` order is filesystem-dependent, so a `take(n)` applied during
/// enumeration silently drops a different subset on different machines. The
/// entries are collected, ordered through `order_candidates`, and only then
/// truncated.
const OTHER_DRIVE_ROOT_CAP: usize = 80;

fn find_bytes(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w == needle)
}

pub(super) fn scan_other_drive_roots(name_slugs: &[String], items: &mut Vec<CleanupItem>) {
    for letter in b'C'..=b'Z' {
        if crate::scan_task::cancelled() {
            break;
        }
        // skip remote/CDROM — `exists()` on a dead network mapping can stall 30s+.
        if !crate::diskradar::is_local_fixed_drive(letter as char) {
            continue;
        }
        let drive = format!("{}:\\", letter as char);
        let root = PathBuf::from(&drive);
        if !root.exists() {
            continue;
        }
        scan_one_drive_root(&root, &drive, name_slugs, items);
    }
}

/// One drive's top-level scan.
///
/// Split from the drive loop so the order-then-truncate contract behind
/// [`OTHER_DRIVE_ROOT_CAP`] is testable against a fixture directory instead of
/// requiring a second real drive.
fn scan_one_drive_root(
    root: &Path,
    drive: &str,
    name_slugs: &[String],
    items: &mut Vec<CleanupItem>,
) {
    let Ok(rd) = root.read_dir() else {
        return;
    };
    // Collect everything first: a `take(n)` during enumeration would make the
    // result depend on filesystem order.
    let mut dirs: Vec<PathBuf> = Vec::new();
    for e in rd.flatten() {
        if crate::scan_task::cancelled() {
            break;
        }
        // PERF-C: the directory bit is already in the FindNextFileW buffer.
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let p = e.path();
        dirs.push(p);
    }
    for p in crate::fsutil::order_candidates(dirs)
        .into_iter()
        .take(OTHER_DRIVE_ROOT_CAP)
    {
        if crate::scan_task::cancelled() {
            break;
        }
        let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !is_safe_fs(&p) {
            continue;
        }
        let Some(m) = matches_product_dir(fname, name_slugs, &[]) else {
            continue;
        };
        items.push(CleanupItem {
            path: p.to_string_lossy().to_string(),
            kind: ItemKind::Dir,
            score: if m == "confirmed" { 50 } else { 35 },
            confidence: if m == "confirmed" {
                Confidence::Confirmed
            } else {
                Confidence::Suspected
            },
            risk: if m == "confirmed" {
                RiskLevel::Medium
            } else {
                RiskLevel::High
            },
            reason: format!("Other-drive product folder: {drive}{fname}"),
            evidence: vec![Evidence {
                code: "cross_drive".into(),
                label: "Folder on non-default drive matches product".into(),
                weight: if m == "confirmed" { 50 } else { 35 },
                detail: p.to_string_lossy().chars().take(120).collect(),
            }],
            shared: false,
            service_bound: false,
            user_data: is_user_data(&p),
            user_library: is_user_library(&p),
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
}

/// Whether a LOCALAPPDATA/APPDATA child dir name looks like a product WebView2/Electron
/// mask. Name match is REQUIRED — a folder that merely contains EBWebView is not evidence.
pub(crate) fn webview_mask_matches(dir_name: &str, name_slugs: &[String]) -> bool {
    let low = dir_name.to_lowercase();
    if !low.ends_with(".exe") && !low.contains("ebwebview") {
        return false;
    }
    let stem_n = normalize_for_match(low.trim_end_matches(".exe"));
    if stem_n.len() < 4 {
        return false;
    }
    name_slugs.iter().any(|s| {
        let n = normalize_for_match(s);
        n.len() >= 4 && stem_n.contains(n.as_str())
    })
}

/// M4 (SOP §8-20): dot-directories at the user-profile root (`.ollama`,
/// `.gemini\<app>`) are invisible to the AppData/Program Files roots. Only
/// names matching a product slug are reported; shared vendor roots themselves
/// are never candidates, only their matching children.
pub(super) fn scan_home_dot_dirs(
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) -> bool {
    #[cfg(windows)]
    {
        // Known Folder, not USERPROFILE: a rewritten environment variable must
        // not redirect the roots that feed cleanup associations.
        let up = windows::Win32::UI::Shell::FOLDERID_Profile;
        let Some(profile) = crate::sysops::known_folder(&up) else {
            return false;
        };
        scan_home_dot_dirs_from(&profile, name_slugs, install_low, items)
    }
    #[cfg(not(windows))]
    {
        let _ = (name_slugs, install_low, items);
        false
    }
}

fn scan_home_dot_dirs_from(
    up: &Path,
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) -> bool {
    scan_home_dot_dirs_with_cap(up, name_slugs, install_low, items, 2_000)
}

fn scan_home_dot_dirs_with_cap(
    up: &Path,
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
    max_shared_children: usize,
) -> bool {
    let slugs: Vec<&String> = name_slugs
        .iter()
        .filter(|s| super::normalize_for_match(s).len() >= 4)
        .collect();
    if slugs.is_empty() {
        return false;
    }
    let matches_slug = |name: &str| slugs.iter().any(|slug| super::name_variant_hit(name, slug));
    const SHARED_DOT_ROOTS: &[&str] = &[
        ".config", ".gemini", ".claude", ".codex", ".vscode", ".cursor", ".npm", ".cargo",
        ".gradle", ".m2", ".nuget", ".bun",
    ];
    let Ok(rd) = up.read_dir() else {
        return false;
    };
    // Home profiles are small, but a shared dot root can hold thousands of
    // children on dev machines. Bound the walk so analyze stays predictable.
    let mut shared_children = 0usize;
    let mut truncated = false;
    for entry in rd.flatten() {
        if crate::scan_task::cancelled() {
            break;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() || crate::dirsize::is_reparse_metadata(&meta) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with('.') || name == ".git" || name == ".ssh" || name == ".gnupg" {
            continue;
        }
        let path = entry.path();
        let shared = SHARED_DOT_ROOTS
            .iter()
            .any(|r| r.eq_ignore_ascii_case(&name));
        if !shared && matches_slug(&name) {
            push_home_dot_item(&path, &name, install_low, items);
        }
        if !shared {
            continue;
        }
        let Ok(children) = path.read_dir() else {
            continue;
        };
        for child in children.flatten() {
            if crate::scan_task::cancelled() {
                break;
            }
            if shared_children >= max_shared_children {
                truncated = true;
                break;
            }
            shared_children += 1;
            let Ok(meta) = child.metadata() else {
                continue;
            };
            if !meta.is_dir() || crate::dirsize::is_reparse_metadata(&meta) {
                continue;
            }
            let child_name = child.file_name().to_string_lossy().to_string();
            if matches_slug(&child_name) {
                push_home_dot_item(&child.path(), &child_name, install_low, items);
            }
        }
        if truncated {
            // Keep scanning top-level dot dirs (they are not capped); only the
            // shared roots' child enumeration is budgeted.
            continue;
        }
    }
    truncated
}

fn push_home_dot_item(path: &Path, name: &str, install_low: &str, items: &mut Vec<CleanupItem>) {
    let path_str = path.to_string_lossy().to_string();
    let path_low = path_str.to_lowercase();
    let install_low = install_low.trim_end_matches('\\');
    // A dot directory that *is* the install root is handled by the normal scan.
    if !install_low.is_empty()
        && (path_low == install_low || path_low.starts_with(&format!("{install_low}\\")))
    {
        return;
    }
    items.push(CleanupItem {
        path: path_str,
        kind: ItemKind::Dir,
        score: 30,
        confidence: Confidence::Suspected,
        risk: RiskLevel::Medium,
        reason: format!("Home dot-directory: {name}"),
        evidence: vec![Evidence {
            code: "home_dot_dir".into(),
            label: "Dot-directory under the user profile".into(),
            weight: 20,
            detail: name.into(),
        }],
        shared: false,
        service_bound: false,
        user_data: false,
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: Some("configFiles".into()),
    });
}

/// WebView2 cache dirs named like `app.exe` (SOP: ollama app.exe\EBWebView).
pub(super) fn scan_webview_masks(name_slugs: &[String], items: &mut Vec<CleanupItem>) {
    #[cfg(windows)]
    let roots: Vec<PathBuf> = {
        use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_RoamingAppData};
        [&FOLDERID_LocalAppData, &FOLDERID_RoamingAppData]
            .into_iter()
            .filter_map(crate::sysops::known_folder)
            .collect()
    };
    #[cfg(not(windows))]
    let roots: Vec<PathBuf> = {
        let mut roots = Vec::new();
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            roots.push(PathBuf::from(local));
        }
        if let Some(roaming) = std::env::var_os("APPDATA") {
            roots.push(PathBuf::from(roaming));
        }
        roots
    };
    for base in roots {
        if crate::scan_task::cancelled() {
            break;
        }
        let Ok(rd) = base.read_dir() else {
            continue;
        };
        for e in rd.flatten() {
            if crate::scan_task::cancelled() {
                break;
            }
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let p = e.path();
            let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            // Name match is REQUIRED — WebView2 folder alone is not evidence.
            if !webview_mask_matches(fname, name_slugs) {
                continue;
            }
            let has_web = p.join("EBWebView").exists() || p.join("WebView2").exists();
            if !is_safe_fs(&p) {
                continue;
            }
            items.push(CleanupItem {
                path: p.to_string_lossy().to_string(),
                kind: ItemKind::Dir,
                score: 40,
                // AppData / WebView caches are name-match only — require user confirm.
                confidence: Confidence::Suspected,
                risk: RiskLevel::Medium,
                reason: if has_web {
                    "WebView2 / Electron cache folder".into()
                } else {
                    "AppData folder matching product name".into()
                },
                evidence: vec![Evidence {
                    code: "webview_mask".into(),
                    label: "Looks like app cache under AppData".into(),
                    weight: 40,
                    detail: fname.to_string(),
                }],
                shared: false,
                service_bound: false,
                user_data: is_user_data(&p),
                user_library: is_user_library(&p),
                size_kb: None,
                size_capped: false,
                bucket: None,
            });
        }
    }
}

#[derive(Clone)]
struct ShortcutEntry {
    path: PathBuf,
    stem: String,
    peek: Vec<u8>,
    /// Lowercased UTF-16 decode of `peek`, computed once per inventory. The
    /// old matcher decoded every shortcut again for every app in the batch.
    text_lc: String,
}

#[derive(Clone)]
struct TempEntry {
    path: PathBuf,
    is_dir: bool,
}

/// Shortcut/TEMP listings are app-independent: a batch used to re-walk the
/// same 4 shortcut roots and TEMP directory for every app. Cache the raw
/// entries for the batch and invalidate on a new inventory.
mod scan_cache {
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    const TTL: Duration = Duration::from_secs(5 * 60);
    /// A partial TEMP listing (one root unreadable) is reused briefly so a
    /// batch does not re-walk every readable root per app, then retried.
    const PARTIAL_TTL: Duration = Duration::from_secs(30);

    type ShortcutState = Option<(Instant, Arc<Vec<super::ShortcutEntry>>)>;
    type TempState = Option<(Instant, Arc<Vec<super::TempEntry>>, bool)>;

    fn shortcuts_state() -> &'static Mutex<ShortcutState> {
        static STATE: OnceLock<Mutex<ShortcutState>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(None))
    }

    fn temp_state() -> &'static Mutex<TempState> {
        static STATE: OnceLock<Mutex<TempState>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(None))
    }

    pub(super) fn shortcuts(
        build: impl FnOnce() -> Vec<super::ShortcutEntry>,
    ) -> Arc<Vec<super::ShortcutEntry>> {
        {
            let guard = shortcuts_state().lock().unwrap_or_else(|e| e.into_inner());
            if let Some((at, entries)) = guard.as_ref() {
                if at.elapsed() < TTL {
                    return Arc::clone(entries);
                }
            }
        }
        let built = Arc::new(build());
        if crate::scan_task::cancelled() {
            // A cancelled walk produced a partial list; never publish it.
            return built;
        }
        // Re-lock only to publish: the previous lock covered the whole walk and
        // serialized every concurrent analyze behind one directory traversal.
        let mut guard = shortcuts_state().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, entries)) = guard.as_ref() {
            if at.elapsed() < TTL {
                return Arc::clone(entries);
            }
        }
        *guard = Some((Instant::now(), Arc::clone(&built)));
        built
    }

    /// `build` returns the rows it collected plus a completeness flag. A
    /// partial list is served and reused for [`PARTIAL_TTL`] — readable roots
    /// must not be hidden because a sibling root failed, and a permanently
    /// unreadable root must not make every app re-walk the cache roots.
    pub(super) fn temp(
        build: impl FnOnce() -> (Vec<super::TempEntry>, bool),
    ) -> (Arc<Vec<super::TempEntry>>, bool) {
        {
            let guard = temp_state().lock().unwrap_or_else(|e| e.into_inner());
            if let Some((at, entries, complete)) = guard.as_ref() {
                let ttl = if *complete { TTL } else { PARTIAL_TTL };
                if at.elapsed() < ttl {
                    return (Arc::clone(entries), *complete);
                }
            }
        }
        let (entries, complete) = build();
        let built = Arc::new(entries);
        if crate::scan_task::cancelled() {
            return (built, complete);
        }
        let mut guard = temp_state().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, entries, complete)) = guard.as_ref() {
            let ttl = if *complete { TTL } else { PARTIAL_TTL };
            if at.elapsed() < ttl {
                return (Arc::clone(entries), *complete);
            }
        }
        *guard = Some((Instant::now(), Arc::clone(&built), complete));
        (built, complete)
    }

    #[cfg(test)]
    pub(super) fn expire_temp_for_test() {
        let mut guard = temp_state().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, _, _)) = guard.as_mut() {
            *at = Instant::now() - PARTIAL_TTL - Duration::from_secs(1);
        }
    }

    pub(super) fn clear() {
        *shortcuts_state().lock().unwrap_or_else(|e| e.into_inner()) = None;
        *temp_state().lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn shortcut_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{
            FOLDERID_CommonStartMenu, FOLDERID_Desktop, FOLDERID_PublicDesktop, FOLDERID_StartMenu,
        };
        [
            &FOLDERID_Desktop,
            &FOLDERID_StartMenu,
            &FOLDERID_PublicDesktop,
            &FOLDERID_CommonStartMenu,
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

fn collect_shortcut_entries() -> Vec<ShortcutEntry> {
    let mut out = Vec::new();
    for root in shortcut_roots() {
        if crate::scan_task::cancelled() {
            break;
        }
        if root.exists() {
            collect_shortcut_dir(&root, 0, &mut out);
        }
    }
    out
}

fn collect_shortcut_dir(dir: &Path, depth: u32, out: &mut Vec<ShortcutEntry>) {
    if depth > 3 || crate::scan_task::cancelled() {
        return;
    }
    let Ok(rd) = dir.read_dir() else {
        return;
    };
    for e in rd.flatten() {
        if crate::scan_task::cancelled() {
            break;
        }
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        let p = e.path();
        if is_dir {
            collect_shortcut_dir(&p, depth + 1, out);
            continue;
        }
        let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if !matches!(ext.to_lowercase().as_str(), "lnk" | "url" | "appref-ms") {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let mut peek_buf = [0u8; SHORTCUT_PEEK_BYTES];
        let peek = std::fs::File::open(&p)
            .and_then(|mut f| {
                use std::io::Read;
                f.read(&mut peek_buf)
            })
            .map(|n| peek_buf[..n].to_vec())
            .unwrap_or_default();
        let text_lc = peek_text_lc(&peek);
        out.push(ShortcutEntry {
            path: p,
            stem,
            peek,
            text_lc,
        });
    }
}

fn peek_text_lc(peek: &[u8]) -> String {
    // Decode UTF-16LE and lowercase — byte-searching a lowercase needle in
    // raw-case UTF-16 always missed.
    let u16s: Vec<u16> = peek
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&u16s).to_lowercase()
}

fn shortcut_peek_matches(peek: &[u8], text_lc: &str, install_low: &str) -> bool {
    if install_low.is_empty() {
        return false;
    }
    let peek_lc = peek.to_ascii_lowercase();
    find_bytes(&peek_lc, install_low.as_bytes()) || text_lc.contains(install_low)
}

pub(super) fn scan_shortcuts(
    name_slugs: &[String],
    exe_stems: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    let entries = scan_cache::shortcuts(collect_shortcut_entries);
    for entry in entries.iter() {
        if crate::scan_task::cancelled() {
            break;
        }
        let sn = normalize_for_match(&entry.stem);
        let hit = name_slugs.iter().any(|s| normalize_for_match(s) == sn)
            || exe_stems
                .iter()
                .any(|s| normalize_for_match(s) == sn && sn.len() >= 3)
            || shortcut_peek_matches(&entry.peek, &entry.text_lc, install_low);
        if !hit || !is_safe_fs(&entry.path) {
            continue;
        }
        // A cleanup may have removed this entry while the batch cache lived.
        // Do not report a path that no longer exists as a confirmed leftover.
        if !entry.path.try_exists().unwrap_or(false) {
            continue;
        }
        items.push(CleanupItem {
            path: entry.path.to_string_lossy().to_string(),
            kind: ItemKind::File,
            score: 50,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "Shortcut".into(),
            evidence: vec![Evidence {
                code: "shortcut_target".into(),
                label: "Shortcut matches product".into(),
                weight: 50,
                detail: entry.stem.clone(),
            }],
            shared: false,
            service_bound: false,
            user_data: is_user_data(&entry.path),
            user_library: is_user_library(&entry.path),
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
}

/// Drop the batch-scoped shortcut/TEMP listings.
pub(crate) fn clear_scan_caches() {
    scan_cache::clear();
}

/// TEMP directories to scan. LocalAppData\Temp is the default, but users can
/// redirect TEMP to another local drive; that root must be scanned too or its
/// leftovers never surface. UNC/device targets are skipped (read-only walk,
/// but it would contact a remote share).
fn temp_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::FOLDERID_LocalAppData;
        if let Some(local) = crate::sysops::known_folder(&FOLDERID_LocalAppData) {
            roots.push(local.join("Temp"));
        }
    }
    if let Some(raw) = std::env::var_os("TEMP") {
        let p = PathBuf::from(raw);
        if p.is_absolute() && crate::fsutil::is_local_drive_path(&p.to_string_lossy()) {
            roots.push(p);
        }
    }
    let mut seen = std::collections::HashSet::new();
    roots.retain(|p| {
        // Normalize a trailing separator before dedupe: `TEMP=…\Temp\` and the
        // Known Folder root are the same directory and must be walked once.
        let key = p
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase();
        p.is_dir() && seen.insert(key)
    });
    roots
}

/// Returns `true` when at least one TEMP root could not be read, so the scan
/// result can surface a degraded marker instead of silently looking complete.
pub(super) fn scan_temp(name_slugs: &[String], items: &mut Vec<CleanupItem>) -> bool {
    let roots = temp_roots();
    if roots.is_empty() {
        return false;
    }
    let slugs: Vec<String> = name_slugs
        .iter()
        .map(|s| normalize_for_match(s))
        .filter(|s| s.len() >= 4)
        .collect();
    if slugs.is_empty() {
        return false;
    }
    let (entries, complete) = scan_cache::temp(|| {
        let mut out = Vec::new();
        let mut complete = true;
        for root in &roots {
            match root.read_dir() {
                Ok(rd) => {
                    for e in rd.flatten() {
                        if crate::scan_task::cancelled() {
                            break;
                        }
                        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
                        out.push(TempEntry {
                            path: e.path(),
                            is_dir,
                        });
                    }
                }
                // Keep this call's partial data, but do not publish it: a
                // permission blip at one root must not hide the other roots
                // for the next five minutes.
                Err(_) => complete = false,
            }
            if crate::scan_task::cancelled() {
                break;
            }
        }
        (out, complete)
    });
    for entry in entries.iter() {
        if crate::scan_task::cancelled() {
            break;
        }
        let p = &entry.path;
        // Match the entry NAME only — a slug appearing in a directory
        // component (user name, `Local`, `Temp`, product root) must not
        // claim unrelated files under it.
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !temp_entry_matches(name, &slugs) {
            continue;
        }
        if !is_safe_fs(p) {
            continue;
        }
        if !p.try_exists().unwrap_or(false) {
            continue;
        }
        items.push(CleanupItem {
            path: p.to_string_lossy().to_string(),
            kind: if entry.is_dir {
                ItemKind::Dir
            } else {
                ItemKind::File
            },
            score: 30,
            confidence: Confidence::Suspected,
            risk: RiskLevel::Medium,
            reason: "TEMP name match".into(),
            evidence: vec![Evidence {
                code: "recent_temp_match".into(),
                label: "TEMP folder/file name match".into(),
                weight: 30,
                detail: p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            }],
            shared: false,
            service_bound: false,
            user_data: is_user_data(p),
            user_library: is_user_library(p),
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
    !complete
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn shortcut_list_is_built_once_per_generation() {
        scan_cache::clear();
        let mut calls = 0;
        let first = scan_cache::shortcuts(|| {
            calls += 1;
            vec![ShortcutEntry {
                path: PathBuf::from(r"C:\Shortcuts\Demo.lnk"),
                stem: "Demo".into(),
                peek: b"target".to_vec(),
                text_lc: "target".into(),
            }]
        });
        let second = scan_cache::shortcuts(|| {
            calls += 1;
            Vec::new()
        });
        assert_eq!(calls, 1, "the second call must reuse the cached list");
        assert_eq!(first.len(), 1);
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        clear_scan_caches();
        let third = scan_cache::shortcuts(|| {
            calls += 1;
            Vec::new()
        });
        assert_eq!(calls, 2, "clearing must force a rebuild");
        assert!(third.is_empty());
        scan_cache::clear();
    }

    #[cfg(windows)]
    #[test]
    fn temp_roots_include_a_redirected_temp_and_dedupe_trailing_separators() {
        let _lock = crate::test_support::EnvLock::new();
        let dir = crate::test_support::TempDir::new("temp-roots");
        let custom = dir.subdir("Redirected");
        {
            let _env = crate::test_support::EnvVar::set("TEMP", custom.clone());
            let roots = temp_roots();
            assert!(
                roots.iter().any(|p| p == &custom),
                "a redirected TEMP root must be scanned"
            );
        }
        if let Some(local) =
            crate::sysops::known_folder(&windows::Win32::UI::Shell::FOLDERID_LocalAppData)
        {
            let temp = local.join("Temp");
            if temp.is_dir() {
                let _env =
                    crate::test_support::EnvVar::set("TEMP", format!("{}\\", temp.display()));
                let roots = temp_roots();
                assert_eq!(
                    roots.len(),
                    1,
                    "TEMP and LocalAppData\\Temp with a trailing separator are one root"
                );
                assert!(roots[0]
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&temp.to_string_lossy()));
            }
        }
    }

    #[test]
    fn a_partial_temp_walk_is_reused_briefly_and_retried() {
        scan_cache::clear();
        let (first, complete) = scan_cache::temp(|| {
            (
                vec![TempEntry {
                    path: PathBuf::from(r"C:\Temp\Demo-partial.tmp"),
                    is_dir: false,
                }],
                false,
            )
        });
        assert_eq!(first.len(), 1, "readable roots keep their rows");
        assert!(!complete);
        // Within the short partial TTL the batch reuses the rows instead of
        // re-walking every readable root for every app.
        let (second, complete) =
            scan_cache::temp(|| panic!("a partial walk must be reused briefly"));
        assert_eq!(second.len(), 1);
        assert!(
            !complete,
            "a cached partial list still reports incompleteness"
        );
        // After the TTL the walk is retried and a complete list is cached longer.
        scan_cache::expire_temp_for_test();
        let (third, complete) = scan_cache::temp(|| {
            (
                vec![
                    TempEntry {
                        path: PathBuf::from(r"C:\Temp\Demo-a.tmp"),
                        is_dir: false,
                    },
                    TempEntry {
                        path: PathBuf::from(r"C:\Temp\Demo-b.tmp"),
                        is_dir: false,
                    },
                ],
                true,
            )
        });
        assert_eq!(third.len(), 2);
        assert!(complete);
        let (fourth, complete) = scan_cache::temp(|| panic!("a complete build must be cached"));
        assert_eq!(fourth.len(), 2);
        assert!(complete);
        scan_cache::clear();
    }

    #[test]
    fn shortcut_peek_matches_ascii_and_utf16() {
        let ascii = b"C:\\Apps\\Demo\\demo.exe";
        assert!(!shortcut_peek_matches(b"anything", "", ""));
        assert!(shortcut_peek_matches(
            ascii,
            &peek_text_lc(ascii),
            r"c:\apps\demo"
        ));
        let utf16: Vec<u8> = r"C:\Apps\Demo\demo.exe"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert!(shortcut_peek_matches(
            &utf16,
            &peek_text_lc(&utf16),
            r"c:\apps\demo"
        ));
        let other = b"C:\\Apps\\Other\\other.exe";
        assert!(!shortcut_peek_matches(
            other,
            &peek_text_lc(other),
            r"c:\apps\demo"
        ));
    }
}

#[cfg(test)]
mod user_flag_tests {
    use super::*;

    /// Items inside a user library must carry the red-line flags — the
    /// cross-drive scan's `D:\Users\a\Documents\...` shape previously shipped
    /// with `user_data: false` and was default-selectable in the FE.
    #[test]
    fn red_line_flags_follow_the_real_path() {
        let under_lib = std::path::Path::new(r"D:\Users\a\Documents\SomeGame\cache");
        assert!(!is_user_data(under_lib));
        assert!(is_user_library(under_lib));
        let lib_root = std::path::Path::new(r"D:\Users\a\Documents");
        assert!(is_user_data(lib_root));
        assert!(!is_user_library(lib_root));
        // Non-library shapes stay unflagged.
        let plain = std::path::Path::new(r"D:\Games\SomeGame");
        assert!(!is_user_data(plain));
        assert!(!is_user_library(plain));
    }
}

/// TEMP entry matching is a file-NAME match only: a slug appearing in a
/// directory component (`Users`, the user name, `Local`, `Temp`, an install
/// root…) must not claim unrelated entries under it.
pub(super) fn temp_entry_matches(entry_name: &str, slugs: &[String]) -> bool {
    let name = normalize_for_match(entry_name);
    slugs.iter().any(|s| name.contains(s.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// only the entry name is matched, never the whole path.
    #[test]
    fn temp_match_uses_entry_name_not_full_path() {
        let slugs = vec![normalize_for_match("Steam")];
        assert!(temp_entry_matches("steam_dumps", &slugs));
        assert!(temp_entry_matches("Steam.exe", &slugs));
        assert!(!temp_entry_matches("unrelated.log", &slugs));
        assert!(!temp_entry_matches("savegame.tmp", &slugs));
        // Name-insensitive composite still matches.
        assert!(temp_entry_matches("MySteamTemp", &slugs));
    }

    /// The cross-drive scan collects every top-level directory, orders the
    /// collected set, and only then truncates. The fixture names make
    /// the filesystem's enumeration order the reverse of the sort order: NTFS
    /// compares upper-cased code units, so `_` (0x5F) enumerates after `Z`
    /// (0x5A), while `order_candidates` sorts lower-cased strings, where `_`
    /// comes before `z`. A take-during-enumeration shape therefore keeps a
    /// disjoint subset, so this test fails if the ordering is reverted.
    #[test]
    fn cross_drive_root_cap_selects_a_deterministic_subset() {
        let root = crate::test_support::TempDir::new("drive_root_cap");
        // `normalize_for_match` strips punctuation, so both families match the
        // "acme" slug; only the *order* differs between the two views.
        const EARLY_IN_SORT_ORDER: usize = 12;
        let names = (0..EARLY_IN_SORT_ORDER)
            .map(|i| format!("_AcmeSpare{i:03}"))
            .chain((0..OTHER_DRIVE_ROOT_CAP).map(|i| format!("ZuluAcme{i:03}")));
        for name in names {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        let slugs = vec![normalize_for_match("Acme")];
        let mut items = Vec::new();
        scan_one_drive_root(root.path(), "Q:\\", &slugs, &mut items);

        assert_eq!(items.len(), OTHER_DRIVE_ROOT_CAP);
        let paths: Vec<&str> = items.iter().map(|it| it.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_by_key(|p| p.to_lowercase());
        assert_eq!(
            paths, sorted,
            "the reported subset must be in a stable order"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("_AcmeSpare000")),
            "a folder that sorts first must survive the cap even though NTFS enumerates it last"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("ZuluAcme079")),
            "a folder that sorts last must be the one the cap drops"
        );
        // Rows carry the cross-drive evidence and name the drive they came from.
        assert!(items.iter().all(|it| it.evidence[0].code == "cross_drive"));
        assert!(items.iter().all(|it| it.reason.contains("Q:\\")));
        assert!(items
            .iter()
            .all(|it| it.confidence == Confidence::Suspected));
    }

    /// M4 (SOP §8-20): home dot-directories are matched by product slug;
    /// shared vendor roots contribute only matching children, never the root.
    #[test]
    fn home_dot_dirs_report_matching_dirs_and_shared_children_only() {
        let up = crate::test_support::TempDir::new("home_dot");
        std::fs::create_dir_all(up.join(".acmeproduct").join("config")).unwrap();
        std::fs::create_dir_all(up.join(".acme-product")).unwrap();
        std::fs::create_dir_all(up.join(".gemini").join("acmeproduct")).unwrap();
        std::fs::create_dir_all(up.join(".gemini").join("other")).unwrap();
        let mut items = Vec::new();
        super::scan_home_dot_dirs_from(up.path(), &["AcmeProduct".to_string()], "", &mut items);
        let paths: Vec<String> = items.iter().map(|i| i.path.to_lowercase()).collect();
        assert!(paths.iter().any(|p| p.ends_with(".acmeproduct")));
        assert!(paths.iter().any(|p| p.ends_with(".acme-product")));
        assert!(paths.iter().any(|p| p.ends_with(".gemini\\acmeproduct")));
        assert!(
            !paths.iter().any(|p| p.ends_with(".gemini")),
            "shared root must never be a target"
        );
        assert!(!paths.iter().any(|p| p.ends_with("\\other")));
    }

    #[test]
    fn shared_child_cap_reports_truncation() {
        let up = crate::test_support::TempDir::new("home_dot_cap");
        std::fs::create_dir_all(up.join(".config").join("acme")).unwrap();
        std::fs::create_dir_all(up.join(".config").join("second")).unwrap();
        std::fs::create_dir_all(up.join(".acme")).unwrap();
        let mut items = Vec::new();
        assert!(super::scan_home_dot_dirs_with_cap(
            up.path(),
            &["Acme".to_string()],
            "",
            &mut items,
            1
        ));
        assert!(
            items
                .iter()
                .any(|it| it.path.to_lowercase().ends_with("\\.acme")),
            "the shared-child cap must not hide top-level dot dirs"
        );
    }

    /// A shared dot root whose name happens to match a product slug stays
    /// off the candidate list; only its matching children may be reported.
    #[test]
    fn shared_dot_root_is_never_a_target_even_when_the_slug_matches() {
        let up = crate::test_support::TempDir::new("home_dot_shared");
        std::fs::create_dir_all(up.join(".config").join("config")).unwrap();
        let mut items = Vec::new();
        super::scan_home_dot_dirs_from(up.path(), &["Config".to_string()], "", &mut items);
        let paths: Vec<String> = items.iter().map(|i| i.path.to_lowercase()).collect();
        assert!(
            !paths.iter().any(|p| p.ends_with("\\.config")),
            "shared root must never be a target"
        );
        assert!(paths.iter().any(|p| p.ends_with("\\.config\\config")));
    }
}
