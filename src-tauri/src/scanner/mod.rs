//! Read-only association scanner (Phase 1) — parity-oriented port of Python association.

use crate::safety::is_safe_to_delete_registry;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCORE_CONFIRMED: i32 = 90;
pub const SCORE_SUSPECTED_MIN: i32 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    File,
    Dir,
    Registry,
    /// Environment PATH entry to scrub (User/Machine).
    Path,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Confirmed,
    Suspected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub code: String,
    pub label: String,
    pub weight: i32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupItem {
    pub path: String,
    pub kind: ItemKind,
    pub score: i32,
    pub confidence: Confidence,
    pub risk: RiskLevel,
    pub reason: String,
    pub evidence: Vec<Evidence>,
    /// Shared runtime / redistributable —default do-not-select.
    #[serde(default)]
    pub shared: bool,
    /// Library roots / sync-conflict red line —never delete.
    #[serde(default)]
    pub user_data: bool,
    /// Under a user library folder (Documents/Downloads/— but not the root —confirm,
    /// never default-select; cleanable when associated.
    #[serde(default)]
    pub user_library: bool,
    /// Best-effort size in KB for file/dir leftovers only (None for registry/path or when bounded walk hits a cap).
    #[serde(default)]
    pub size_kb: Option<u64>,
    /// Display bucket for the detail panel (program_files / config_files / —.
    #[serde(default)]
    pub bucket: Option<String>,
}

/// Presentation bucket for linked-items UI (does not affect cleanup selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkedBucket {
    ProgramFiles,
    ConfigFiles,
    Registry,
    Shortcuts,
    Startup,
    Other,
}

impl LinkedBucket {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkedBucket::ProgramFiles => "programFiles",
            LinkedBucket::ConfigFiles => "configFiles",
            LinkedBucket::Registry => "registry",
            LinkedBucket::Shortcuts => "shortcuts",
            LinkedBucket::Startup => "startup",
            LinkedBucket::Other => "other",
        }
    }
}

fn is_lnk_path(path: &str) -> bool {
    path.to_lowercase().ends_with(".lnk")
}

fn is_startup_path(path: &str) -> bool {
    let p = path.to_lowercase().replace('/', "\\");
    p.contains("\\currentversion\\run")
        || p.contains("\\startup")
        || p.contains("\\start menu\\programs\\startup")
}

fn is_config_path(path: &str) -> bool {
    let p = path.to_lowercase().replace('/', "\\");
    p.contains("\\appdata\\") || p.contains("\\programdata\\") || p.contains("\\application data\\")
}

/// True when `install` is deep enough to act as an association prefix.
/// Drive roots (`C:` / `C:\`) and empty strings never claim a whole drive.
fn install_root_is_deep_enough(install: &str) -> bool {
    let p = install.trim().replace('/', "\\");
    let p = p.trim_end_matches('\\');
    if p.is_empty() {
        return false;
    }
    p.split('\\').filter(|s| !s.is_empty()).count() >= 2
}

fn is_under_install(path: &str, install: &str) -> bool {
    if !install_root_is_deep_enough(install) {
        return false;
    }
    let a = path.to_lowercase().replace('/', "\\");
    let b = install
        .to_lowercase()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_string();
    if b.is_empty() {
        return false;
    }
    a == b || a.starts_with(&format!("{b}\\"))
}

/// Product-name hit on a raw candidate string: exact normalize-equality or a
/// boundary-anchored slug (`steam` hits `Steam Tray` / `steam.exe` but never
/// `SteamToolsSvc` or `mysteamapp`).
pub(crate) fn name_boundary_hit(candidate: &str, slug: &str) -> bool {
    let cand_low = candidate.to_lowercase();
    let slug_low = slug.to_lowercase();
    if slug_low.is_empty() {
        return false;
    }
    if normalize_for_match(candidate) == normalize_for_match(slug) {
        return true;
    }
    slug_boundary_hit(&cand_low, &slug_low)
}

/// Install-location hit with path-segment boundaries for command-line data
/// (`Run` values, service ImagePath): `C:\Steam` must not claim
/// `C:\SteamTools\...`. Quoted tokens and the unquoted head are checked for a
/// boundary-anchored install prefix (`install` itself or `install\…`).
fn cmdline_refs_install(data: &str, install_low: &str) -> bool {
    if install_low.is_empty() || !install_root_is_deep_enough(install_low) {
        return false;
    }
    let d = data.to_lowercase().replace('/', "\\");
    let under = format!("{install_low}\\");
    for (i, chunk) in d.split('"').enumerate() {
        if i % 2 == 1 {
            // Odd chunks are quoted tokens — each is one path.
            let c = chunk.trim();
            if c == install_low || c.starts_with(&under) {
                return true;
            }
        } else {
            // Unquoted chunk: the leading command token, plus any boundary-
            // anchored occurrence (argument values like `--dir=C:\App\…`).
            let tok = chunk.trim_start().split([' ', '\t']).next().unwrap_or("");
            if tok == install_low || tok.starts_with(&under) || chunk.contains(&under) {
                return true;
            }
        }
    }
    false
}

/// a slug counts only at non-alphanumeric boundaries in the raw
/// lowercased text — `steam` hits `steam.exe`/`Steam Tray` but never
/// `SteamTools` or `mysteam`.
pub(crate) fn slug_boundary_hit(hay_low: &str, slug: &str) -> bool {
    if slug.is_empty() {
        return false;
    }
    let mut from = 0usize;
    while let Some(i) = hay_low[from..].find(slug) {
        let s = from + i;
        let e = s + slug.len();
        let before_ok = s == 0
            || !hay_low[..s]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric());
        let after_ok = e >= hay_low.len()
            || !hay_low[e..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        // Advance by a whole character; CJK slugs must never split UTF-8.
        from = s + hay_low[s..].chars().next().map_or(0, char::len_utf8);
    }
    false
}

/// Classify one leftover into a display bucket (path heuristics; mirrors frontend linkedItems).
pub fn classify_bucket(kind: &ItemKind, path: &str, install_location: &str) -> LinkedBucket {
    match kind {
        ItemKind::File if is_lnk_path(path) => {
            if is_startup_path(path) {
                LinkedBucket::Startup
            } else {
                LinkedBucket::Shortcuts
            }
        }
        ItemKind::Registry => {
            if is_startup_path(path) {
                LinkedBucket::Startup
            } else {
                LinkedBucket::Registry
            }
        }
        ItemKind::Path => LinkedBucket::Other,
        ItemKind::File | ItemKind::Dir => {
            if is_startup_path(path) {
                LinkedBucket::Startup
            } else if is_config_path(path) {
                LinkedBucket::ConfigFiles
            } else if is_under_install(path, install_location)
                || path.to_lowercase().contains("\\program files")
            {
                LinkedBucket::ProgramFiles
            } else {
                LinkedBucket::ConfigFiles
            }
        }
    }
}

/// Fill display buckets (idempotent).
pub fn fill_item_buckets(items: &mut [CleanupItem], install_location: &str) {
    for it in items.iter_mut() {
        if crate::scan_task::cancelled() {
            break;
        }
        if it.bucket.is_none() {
            it.bucket = Some(
                classify_bucket(&it.kind, &it.path, install_location)
                    .as_str()
                    .into(),
            );
        }
    }
}

/// Size for a leftover path: file metadata or bounded directory walk.
pub fn path_size_kb(kind: &ItemKind, path: &str) -> Option<u64> {
    match kind {
        ItemKind::File | ItemKind::Dir => {
            crate::dirsize::walk_size_kb_limited(std::path::Path::new(path))
        }
        ItemKind::Registry | ItemKind::Path => None,
    }
}

/// Fill missing size_kb on file/dir items (idempotent; skips registry/path).
pub fn fill_item_sizes(items: &mut [CleanupItem]) {
    for it in items.iter_mut() {
        if crate::scan_task::cancelled() {
            break;
        }
        if it.size_kb.is_none() {
            it.size_kb = path_size_kb(&it.kind, &it.path);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub app_name: String,
    /// Stable scan↔app identity (`source\0registry_key\0name`, matching the
    /// frontend `appKey()`) — two same-named apps can no longer cross-pair.
    #[serde(default)]
    pub app_key: String,
    pub items: Vec<CleanupItem>,
}

pub fn normalize_for_match(s: &str) -> String {
    // Unicode alphanumeric (not just ASCII) — CJK product/folder names must
    // normalize to something matchable, not to the empty string.
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub fn slugify(text: &str) -> Vec<String> {
    let t = text.trim();
    if t.is_empty() {
        return vec![];
    }
    let re_dropped = [
        "installer",
        "setup",
        "uninstall",
        "for windows",
        "win64",
        "win32",
        "x64",
        "x86",
        "64-bit",
        "32-bit",
    ];
    let mut cleaned = t.to_string();
    for d in re_dropped {
        let pat = format!(" {} ", d);
        cleaned = cleaned.to_lowercase().replace(&pat, " ");
    }
    let cleaned = cleaned.trim();
    let tokens: Vec<String> = cleaned
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if tokens.is_empty() {
        return vec![];
    }
    let joined = tokens.concat();
    let hyphen = tokens.join("-");
    let underscore = tokens.join("_");
    let spaced = tokens.join(" ");
    let short = if tokens.len() >= 2 {
        format!("{}{}", tokens[0], tokens[1])
    } else {
        joined.clone()
    };
    let mut slugs: Vec<String> = vec![];
    for s in [
        joined,
        hyphen,
        underscore,
        spaced.to_lowercase(),
        cleaned.to_lowercase(),
        short,
    ] {
        let s = s.trim().to_string();
        if s.len() >= 2 && !slugs.contains(&s) {
            slugs.push(s);
        }
    }
    slugs
}

pub fn extract_exe_stems(install: &Path, name_slugs: &[String]) -> Vec<String> {
    let mut stems: Vec<String> = vec![];
    if let Ok(rd) = install.read_dir() {
        for e in rd.flatten() {
            if crate::scan_task::cancelled() {
                break;
            }
            let p = e.path();
            if p.extension()
                .map(|x| x.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
                && p.is_file()
            {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    let lower = stem.to_lowercase();
                    if !matches!(
                        lower.as_str(),
                        "unins000" | "uninstall" | "unins001" | "setup"
                    ) {
                        stems.push(stem.to_string());
                    }
                }
            }
        }
    }
    let name_norms: Vec<String> = name_slugs.iter().map(|s| normalize_for_match(s)).collect();
    stems.sort_by_key(|s| {
        let sn = normalize_for_match(s);
        let rank = if name_norms.iter().any(|n| {
            (n.len() >= 3 && (sn.contains(n.as_str()) || n.contains(sn.as_str()))) || n == &sn
        }) {
            0
        } else if matches!(
            sn.as_str(),
            "update" | "helper" | "launcher" | "crashhandler" | "service" | "agent"
        ) {
            3
        } else {
            2
        };
        (rank, sn)
    });
    stems.dedup();
    stems
}

pub fn finalize_score(score: i32) -> (Confidence, RiskLevel) {
    let score = score.clamp(-200, 120);
    let conf = if score >= SCORE_CONFIRMED {
        Confidence::Confirmed
    } else {
        Confidence::Suspected
    };
    let risk = if score >= SCORE_CONFIRMED {
        RiskLevel::Low
    } else if score >= SCORE_SUSPECTED_MIN {
        RiskLevel::Medium
    } else {
        RiskLevel::High
    };
    (conf, risk)
}

pub(crate) fn push_item(
    items: &mut Vec<CleanupItem>,
    path: String,
    kind: ItemKind,
    reason: String,
    evidence: Vec<Evidence>,
) {
    let mut score: i32 = evidence.iter().map(|e| e.weight).sum();
    score = score.clamp(-200, 120);
    if score < SCORE_SUSPECTED_MIN {
        return;
    }
    let (confidence, risk) = finalize_score(score);
    let size_kb = path_size_kb(&kind, &path);
    items.push(CleanupItem {
        path,
        kind,
        score,
        confidence,
        risk,
        reason,
        evidence,
        shared: false,
        user_data: false,
        user_library: false,
        size_kb,
        bucket: None,
    });
}

pub(crate) fn matches_product_dir(
    name: &str,
    name_slugs: &[String],
    exe_stems: &[String],
) -> Option<&'static str> {
    let norm = normalize_for_match(name);
    if norm.len() < 2 {
        return None;
    }
    for slug in name_slugs {
        let sn = normalize_for_match(slug);
        if sn.len() >= 2 && norm == sn {
            return Some("confirmed");
        }
    }
    for stem in exe_stems {
        let sn = normalize_for_match(stem);
        if sn.len() >= 3 && norm == sn {
            return Some("confirmed");
        }
    }
    for slug in name_slugs {
        let sn = normalize_for_match(slug);
        if sn.len() >= 4
            && norm.len() >= 4
            && (sn.contains(norm.as_str()) || norm.contains(sn.as_str()))
        {
            return Some("suspected");
        }
    }
    None
}

pub(crate) fn is_safe_fs(p: &Path) -> bool {
    crate::safety::is_safe_fs(p)
}

/// Analyze filesystem + registry associations for one installed app (read-only).
pub fn analyze_associations(
    name: &str,
    install_location: &str,
    publisher: &str,
    registry_key: &str,
    source: &str,
) -> ScanResult {
    let name_slugs = slugify(name);
    // Publisher slug matching lives in association.rs (no dead local table).
    let _ = publisher;
    let install = if install_location.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(install_location))
    };
    let exe_stems = install
        .as_ref()
        .filter(|p| p.is_dir())
        .map(|p| extract_exe_stems(p, &name_slugs))
        .unwrap_or_default();

    let mut items = Vec::new();
    // A cancelled task's partial result is rejected by scan_task::run.
    macro_rules! phase {
        ($stage:literal) => {
            if !crate::scan_task::checkpoint($stage, items.len()) {
                return ScanResult {
                    app_name: name.to_string(),
                    app_key: format!("{source}\0{registry_key}\0{name}"),
                    items,
                };
            }
        };
    }

    // 1. Install location
    phase!("files");
    if let Some(root) = install.as_ref() {
        if root.exists() && is_safe_fs(root) {
            let is_dir = root.is_dir();
            items.push(CleanupItem {
                path: root.to_string_lossy().to_string(),
                kind: if is_dir {
                    ItemKind::Dir
                } else {
                    ItemKind::File
                },
                score: SCORE_CONFIRMED,
                confidence: Confidence::Confirmed,
                risk: RiskLevel::Low,
                reason: "Install location".into(),
                evidence: vec![Evidence {
                    code: "install_location".into(),
                    label: "Matches official install path".into(),
                    weight: 90,
                    detail: root.to_string_lossy().to_string(),
                }],
                shared: false,
                user_data: false,
                user_library: false,
                size_kb: None,
                bucket: None,
            });
        }
    }

    // 2. Named roots
    let mut bases: Vec<PathBuf> = vec![];
    if let Some(v) = std::env::var_os("LOCALAPPDATA") {
        bases.push(PathBuf::from(v));
    }
    if let Some(v) = std::env::var_os("APPDATA") {
        bases.push(PathBuf::from(v));
    }
    if let Some(v) = std::env::var_os("PROGRAMDATA") {
        bases.push(PathBuf::from(v));
    }
    if let Some(v) = std::env::var_os("ProgramFiles") {
        bases.push(PathBuf::from(v));
    }
    if let Some(v) = std::env::var_os("ProgramFiles(x86)") {
        bases.push(PathBuf::from(v));
    }

    let mut seen = std::collections::HashSet::new();
    for base in bases {
        if crate::scan_task::cancelled() {
            break;
        }
        if !base.exists() || !is_safe_fs(&base) {
            continue;
        }
        let Ok(rd) = base.read_dir() else {
            continue;
        };
        for e in rd.flatten() {
            if crate::scan_task::cancelled() {
                break;
            }
            let child = e.path();
            if !child.is_dir() {
                continue;
            }
            let key = child.to_string_lossy().to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            let Some(file_name) = child.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(m) = matches_product_dir(file_name, &name_slugs, &exe_stems) else {
                continue;
            };
            if !is_safe_fs(&child) {
                continue;
            }
            seen.insert(key);
            let conf = if m == "confirmed" {
                Confidence::Confirmed
            } else {
                Confidence::Suspected
            };
            let risk = if conf == Confidence::Confirmed {
                RiskLevel::Low
            } else {
                RiskLevel::Medium
            };
            let score = if conf == Confidence::Confirmed {
                40
            } else {
                30
            };
            let label = if conf == Confidence::Confirmed {
                "Product folder name matches exactly"
            } else {
                "Product folder name fuzzy match"
            };
            items.push(CleanupItem {
                path: child.to_string_lossy().to_string(),
                kind: ItemKind::Dir,
                score,
                confidence: conf,
                risk,
                reason: format!("Product dir under {}", base.display()),
                evidence: vec![Evidence {
                    code: if m == "confirmed" {
                        "product_dir_exact".into()
                    } else {
                        "fuzzy_name_only".into()
                    },
                    label: label.into(),
                    weight: score,
                    detail: file_name.to_string(),
                }],
                shared: false,
                user_data: false,
                user_library: false,
                size_kb: None,
                bucket: None,
            });
        }
    }

    // 3. Uninstall key
    phase!("registry");
    if !registry_key.trim().is_empty() && is_safe_to_delete_registry(registry_key).is_ok() {
        items.push(CleanupItem {
            path: registry_key.to_string(),
            kind: ItemKind::Registry,
            score: SCORE_CONFIRMED,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "Uninstall registry key".into(),
            evidence: vec![Evidence {
                code: "uninstall_key".into(),
                label: "This product's uninstall key".into(),
                weight: 90,
                detail: registry_key.to_string(),
            }],
            shared: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        });
    }

    // 4. App Paths — only when install location verifies
    if let Some(install) = install.as_ref() {
        let install_str = install.to_string_lossy().to_lowercase();
        if !install_str.is_empty() {
            for stem in exe_stems.iter().take(3) {
                if crate::scan_task::cancelled() {
                    break;
                }
                for key in crate::regscan::find_app_paths(&format!("{stem}.exe")) {
                    if crate::scan_task::cancelled() {
                        break;
                    }
                    let Some(target) = crate::regscan::read_string_default(&key) else {
                        continue;
                    };
                    let t = target.trim().trim_matches('"').to_lowercase();
                    // Boundary-anchored: `C:\Steam` must not claim an
                    // App Paths entry pointing at `C:\SteamTools\…`.
                    if !is_under_install(&t, &install_str) {
                        continue;
                    }
                    if is_safe_to_delete_registry(&key).is_ok() {
                        items.push(CleanupItem {
                            path: key.clone(),
                            kind: ItemKind::Registry,
                            score: 50,
                            confidence: Confidence::Confirmed,
                            risk: RiskLevel::Low,
                            reason: format!("App Paths: {stem}.exe"),
                            evidence: vec![Evidence {
                                code: "app_paths".into(),
                                label: "App Paths points into install dir".into(),
                                weight: 50,
                                detail: target,
                            }],
                            shared: false,
                            user_data: false,
                            user_library: false,
                            size_kb: None,
                            bucket: None,
                        });
                    }
                }
            }
        }
    }

    // 5. Run startup values
    let install_low = install
        .as_ref()
        .map(|p| {
            p.to_string_lossy()
                .to_lowercase()
                .trim_end_matches('\\')
                .to_string()
        })
        .unwrap_or_default();
    // enumerate the same Run-family keys the manage view and the
    // install monitor use (HKLM32 / RunOnce / Policies\Explorer\Run included);
    // a scanner-only list missed 32-bit and one-shot autostarts.
    for key in crate::manage::run_key_paths() {
        if crate::scan_task::cancelled() {
            break;
        }
        for (vname, vdata) in crate::regscan::list_values(&key) {
            if crate::scan_task::cancelled() {
                break;
            }
            let hit_install = cmdline_refs_install(&vdata, &install_low);
            // the name hit needs raw-text boundaries too — the
            // normalized contains form hit `SteamTools` data for `Steam`.
            let data_low = vdata.to_lowercase();
            let hit_name = name_slugs
                .iter()
                .map(|s| normalize_for_match(s))
                .any(|n| n.len() >= 4 && slug_boundary_hit(&data_low, &n));
            if !hit_install && !hit_name {
                continue;
            }
            let path = format!("{key}|{vname}");
            if is_safe_to_delete_registry(&path).is_err() {
                continue;
            }
            let score = if hit_install { 55 } else { 40 };
            push_item(
                &mut items,
                path,
                ItemKind::Registry,
                format!("Run startup: {vname}"),
                vec![Evidence {
                    code: "run_startup".into(),
                    label: if hit_install {
                        "Startup points at install dir".into()
                    } else {
                        "Startup name matches product".into()
                    },
                    weight: score,
                    detail: vdata.chars().take(120).collect(),
                }],
            );
        }
    }

    // 6. Windows services (suspected / high — display only)
    phase!("services");
    reg_scans::scan_services(&name_slugs, &exe_stems, &install_low, &mut items);

    // 7. Scheduled tasks (suspected / high — display only)
    phase!("tasks");
    reg_scans::scan_scheduled_tasks(&name_slugs, &mut items);

    // 8. Software registry keys HKLM64/HKLM32/HKCU SOFTWARE\Product
    phase!("registry");
    reg_scans::scan_software_keys(&name_slugs, &mut items);

    // 9. Shortcuts + TEMP
    phase!("shortcuts");
    fs_scans::scan_shortcuts(&name_slugs, &exe_stems, &install_low, &mut items);
    phase!("shortcuts");
    fs_scans::scan_temp(&name_slugs, &mut items);

    // 10. SOP: PATH entries, shell extensions, drivers, cross-drive roots, WebView2 masks
    phase!("other");
    reg_scans::scan_path_env(&name_slugs, &install_low, &mut items);
    phase!("other");
    reg_scans::scan_shell_extensions(&name_slugs, &install_low, &mut items);
    phase!("other");
    reg_scans::scan_drivers(&name_slugs, &install_low, &mut items);
    phase!("other");
    fs_scans::scan_other_drive_roots(&name_slugs, &mut items);
    phase!("other");
    fs_scans::scan_webview_masks(&name_slugs, &mut items);

    for it in &mut items {
        if crate::scan_task::cancelled() {
            break;
        }
        it.shared = crate::shared::is_shared_item(name, &it.path, &it.reason);
        if crate::safety::is_user_data_path(&it.path)
            || crate::safety::looks_like_sync_conflict(&it.path)
        {
            // Frontend i18n renders the user-data hint — keep reason English-neutral.
            it.user_data = true;
            it.risk = RiskLevel::High;
        } else if crate::safety::is_user_library_path(&it.path) {
            // Library subpath (Documents/<App>, Downloads/pkg, — —confirm, never default-select.
            it.user_library = true;
        }
    }

    phase!("sizes");
    fill_item_sizes(&mut items);
    fill_item_buckets(&mut items, install_location);

    // drop leftovers under user-ignored path prefixes (no silent re-appearance).
    phase!("finalizing");
    let ignore = crate::ignore::load();
    items.retain(|it| !crate::ignore::should_skip_leftover_path(&ignore, &it.path));

    ScanResult {
        app_name: name.to_string(),
        app_key: format!("{source}\0{registry_key}\0{name}"),
        items,
    }
}

pub(crate) mod fs_scans;
pub(crate) mod reg_scans;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_keeps_cjk_tokens() {
        // CJK product names must slugify to matchable slugs (previously the
        // ASCII-only tokenizer returned an empty list for pure-CJK names).
        assert_eq!(slugify("腾讯会议"), vec!["腾讯会议".to_string()]);
        let mixed = slugify("Tencent Meeting 腾讯会议");
        assert!(mixed.iter().any(|s| s.contains("tencent")));
        assert!(mixed.iter().any(|s| s.contains("腾讯会议")));
        assert_eq!(normalize_for_match("腾讯会议"), "腾讯会议");
    }

    #[test]
    fn slugify_tokens() {
        let s = slugify("DemoApp Installer");
        assert!(s.iter().any(|x| x.to_lowercase().contains("demoapp")));
    }

    #[test]
    fn score_finalize() {
        assert_eq!(finalize_score(90).0, Confidence::Confirmed);
        assert_eq!(finalize_score(40).0, Confidence::Suspected);
        assert_eq!(finalize_score(5).1, RiskLevel::High);
    }

    /// command-line install references need path-segment
    /// boundaries — `C:\Steam` must not claim `C:\SteamTools\…`.
    /// slug hits need non-alphanumeric boundaries in raw text.
    #[test]
    fn slug_boundary_hit_rejects_prefix_collisions() {
        assert!(slug_boundary_hit(
            r#""c:\\steam\\steam.exe" -silent"#,
            "steam"
        ));
        assert!(slug_boundary_hit("steam tray helper", "steam"));
        assert!(!slug_boundary_hit(r"c:\\steamtools\\tool.exe", "steam"));
        assert!(!slug_boundary_hit("mysteam", "steam"));
        assert!(!slug_boundary_hit("x软件", "软件"));
        assert!(!slug_boundary_hit("软件工具", "软件"));
        assert!(slug_boundary_hit("x软件 软件.exe", "软件"));
        assert!(slug_boundary_hit(" 软件 ", "软件"));
        assert!(!slug_boundary_hit("abc", ""));
        assert!(!slug_boundary_hit("", ""));
    }

    /// Drive-root InstallLocation (`C:` / `C:\`) must never claim every path
    /// on that drive in any association channel.
    #[test]
    fn install_root_drive_root_never_claims_whole_drive() {
        assert!(!is_under_install(r"C:\Steam\bin", "c:"));
        assert!(!is_under_install(r"C:\Steam\bin", r"c:\"));
        assert!(!is_under_install(r"D:\Games\x", "d:"));
        assert!(!cmdline_refs_install(r"C:\Steam\steam.exe", "c:"));
        assert!(!cmdline_refs_install(r"C:\Steam\steam.exe", r"c:\"));
        // Real portable install root still works.
        assert!(is_under_install(r"C:\Steam\steam.exe", r"C:\Steam"));
        assert!(cmdline_refs_install(r"C:\Steam\steam.exe", r"c:\steam"));
    }

    /// Name-side hits need non-alphanumeric boundaries — `steam` must not
    /// claim `SteamToolsSvc` via bare substring containment.
    #[test]
    fn name_hit_requires_word_boundary() {
        assert!(name_boundary_hit("Steam Tray Helper", "steam"));
        assert!(name_boundary_hit("steamtray", "steamtray"));
        assert!(!name_boundary_hit("SteamToolsSvc", "steam"));
        assert!(!name_boundary_hit("mysteamapp", "steam"));
    }

    #[test]
    fn cmdline_install_refs_need_segment_boundary() {
        let install = r"C:\Steam";
        assert!(cmdline_refs_install(
            r#""C:\Steam\steam.exe" -silent"#,
            &install.to_lowercase()
        ));
        assert!(cmdline_refs_install(
            r"C:\Steam\steam.exe /update",
            &install.to_lowercase()
        ));
        assert!(cmdline_refs_install(r"C:\Steam", &install.to_lowercase()));
        assert!(cmdline_refs_install(
            r#""C:\x.exe" --dir=C:\Steam\cache"#,
            &install.to_lowercase()
        ));
        // Prefix collisions must not hit.
        assert!(!cmdline_refs_install(
            r#""C:\SteamTools\tool.exe""#,
            &install.to_lowercase()
        ));
        assert!(!cmdline_refs_install(
            r"C:\SteamWorks\bin\svc.exe",
            &install.to_lowercase()
        ));
        assert!(!cmdline_refs_install("anything", ""));
        assert!(!cmdline_refs_install("", &install.to_lowercase()));
    }

    #[test]
    fn product_dir_match() {
        let slugs = slugify("DemoApp");
        assert_eq!(
            matches_product_dir("DemoApp", &slugs, &[]),
            Some("confirmed")
        );
        assert!(matches_product_dir("TotallyOther", &slugs, &[]).is_none());
    }

    #[test]
    fn analyze_empty_location_no_panic() {
        let r = analyze_associations(
            "DemoApp",
            "",
            "DemoVendor",
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{X}",
            "registry",
        );
        assert_eq!(r.app_key, "registry\0HKLM64\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{X}\0DemoApp");
        assert!(r.items.iter().any(|i| i.path.contains("Uninstall")));
    }

    #[test]
    fn path_size_kb_registry_is_none() {
        assert_eq!(
            path_size_kb(&ItemKind::Registry, r"HKLM\SOFTWARE\Demo"),
            None
        );
        assert_eq!(path_size_kb(&ItemKind::Path, "C:\\Tools"), None);
    }

    #[test]
    fn fill_item_sizes_sets_file_size() {
        let tmp = std::env::temp_dir().join(format!("remova_scan_size_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("app.bin");
        std::fs::write(&f, vec![0u8; 2048]).unwrap();
        let mut items = vec![CleanupItem {
            path: f.to_string_lossy().to_string(),
            kind: ItemKind::File,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "test".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        }];
        fill_item_sizes(&mut items);
        assert_eq!(items[0].size_kb, Some(2));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn classify_bucket_paths() {
        assert_eq!(
            classify_bucket(
                &ItemKind::Dir,
                "C:\\Program Files\\DemoApp\\bin",
                "C:\\Program Files\\DemoApp"
            ),
            LinkedBucket::ProgramFiles
        );
        assert_eq!(
            classify_bucket(
                &ItemKind::Dir,
                "C:\\Users\\u\\AppData\\Local\\Demo",
                "C:\\Program Files\\Demo"
            ),
            LinkedBucket::ConfigFiles
        );
        assert_eq!(
            classify_bucket(&ItemKind::Registry, "HKCU\\SOFTWARE\\Demo", ""),
            LinkedBucket::Registry
        );
        assert_eq!(
            classify_bucket(
                &ItemKind::Registry,
                "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run\\Demo",
                ""
            ),
            LinkedBucket::Startup
        );
        assert_eq!(
            classify_bucket(
                &ItemKind::File,
                "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Demo.lnk",
                ""
            ),
            LinkedBucket::Shortcuts
        );
    }

    #[test]
    fn webview_mask_requires_product_name() {
        let slugs = vec!["ollama".to_string()];
        // Match: product-named Electron host dir.
        assert!(fs_scans::webview_mask_matches("ollama.exe", &slugs));
        assert!(fs_scans::webview_mask_matches("Ollama.exe", &slugs));
        // False positive ( discipline): unrelated .exe / bare EBWebView must not match
        // even when the real folder on disk would contain EBWebView children.
        assert!(!fs_scans::webview_mask_matches("chrome.exe", &slugs));
        assert!(!fs_scans::webview_mask_matches("Code.exe", &slugs));
        assert!(!fs_scans::webview_mask_matches("EBWebView", &slugs));
        assert!(!fs_scans::webview_mask_matches("msedge.exe", &slugs));
        // Too-short stem is ignored.
        assert!(!fs_scans::webview_mask_matches("ab.exe", &slugs));
    }
}
