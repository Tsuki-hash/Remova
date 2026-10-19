//! Specialty tool-cache scan (dev / game / browser).
//!
//! Two kinds of root. Single-owner caches come from a fixed allow-list and are
//! reported whole. Roots several tools share (`~/.cache`, JetBrains) are
//! enumerated one level down so the shared root is never itself a target.
//! Never touches game saves, browser profiles, or package store indexes that
//! break tooling.

use crate::constants::{SPECIALTY_RESULT_CAP, TOOL_CACHE_CHILD_CAP};
use crate::scan_allow::{self, AllowScope};
use crate::scanner::{
    fill_item_buckets, fill_item_sizes, CleanupItem, Confidence, Evidence, ItemKind, RiskLevel,
    SCORE_CONFIRMED, SCORE_SUSPECTED_MIN,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDomain {
    Dev,
    Game,
    Browser,
}

impl ToolDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolDomain::Dev => "dev",
            ToolDomain::Game => "game",
            ToolDomain::Browser => "browser",
        }
    }
}

fn local(low: &str) -> Option<PathBuf> {
    let v = std::env::var_os(low)?;
    Some(PathBuf::from(v))
}

fn userprofile() -> Option<PathBuf> {
    local("USERPROFILE")
}

/// Build the fixed single-owner cache list from explicit bases.
///
/// Kept separate from `tool_roots` so tests can supply a synthetic home: a path
/// that is simply wrong vanishes from `tool_roots` via the `exists()` filter and
/// is invisible otherwise, which is how the old `pnpm\cache` rule went unnoticed.
fn tool_roots_from(
    up: &Path,
    local: &Path,
    roaming: &Path,
    program_files: Option<&Path>,
) -> Vec<(ToolDomain, PathBuf, &'static str)> {
    let mut out = vec![(ToolDomain::Dev, local.join("npm-cache"), "npm-cache")];
    out.push((
        ToolDomain::Dev,
        local.join("pip").join("cache"),
        "pip-cache",
    ));
    out.push((
        ToolDomain::Dev,
        local.join("NuGet").join("v3-cache"),
        "nuget-cache",
    ));
    out.push((
        ToolDomain::Dev,
        local.join("Yarn").join("Cache"),
        "yarn-cache",
    ));
    // pnpm's store moved between versions — cover the known layouts. The
    // previous `pnpm\cache` rule never matched any real pnpm install.
    out.push((ToolDomain::Dev, local.join("pnpm-cache"), "pnpm-cache"));
    out.push((
        ToolDomain::Dev,
        local.join("pnpm").join("store"),
        "pnpm-store",
    ));
    out.push((ToolDomain::Dev, local.join("pnpm-store"), "pnpm-store"));
    out.push((ToolDomain::Dev, local.join("uv").join("cache"), "uv-cache"));
    out.push((
        ToolDomain::Dev,
        up.join(".bun").join("install").join("cache"),
        "bun-cache",
    ));
    out.push((
        ToolDomain::Dev,
        up.join("go").join("pkg").join("mod"),
        "go-mod-cache",
    ));
    out.push((
        ToolDomain::Dev,
        up.join(".m2").join("repository"),
        "maven-cache",
    ));
    out.push((
        ToolDomain::Dev,
        up.join(".nuget").join("packages"),
        "nuget-packages",
    ));
    out.push((
        ToolDomain::Dev,
        up.join(".cargo").join("registry").join("cache"),
        "cargo-cache",
    ));
    out.push((
        ToolDomain::Dev,
        local.join("gradle").join("caches"),
        "gradle-cache",
    ));
    out.push((
        ToolDomain::Browser,
        local
            .join("Google")
            .join("Chrome")
            .join("User Data")
            .join("Default")
            .join("Cache"),
        "chrome-cache",
    ));
    out.push((
        ToolDomain::Browser,
        local
            .join("Microsoft")
            .join("Edge")
            .join("User Data")
            .join("Default")
            .join("Cache"),
        "edge-cache",
    ));
    // VS Code keeps its caches under Roaming, not Local.
    out.push((
        ToolDomain::Browser,
        roaming.join("Code").join("Cache"),
        "vscode-cache",
    ));
    out.push((
        ToolDomain::Browser,
        roaming.join("Code").join("CachedData"),
        "vscode-cached-data",
    ));
    out.push((
        ToolDomain::Browser,
        local.join("Mozilla").join("Firefox").join("Profiles"),
        "firefox-profiles-skip",
    ));
    if let Some(pf) = program_files {
        out.push((
            ToolDomain::Game,
            pf.join("Steam").join("steamapps").join("shadercache"),
            "steam-shadercache",
        ));
    }
    out
}

/// Fixed relative roots per domain (joined with env bases). Missing paths are skipped.
///
/// Every entry here is a single-owner cache whose whole directory is disposable,
/// which is what earns it `Confirmed` / `Low` and the UI's default selection.
/// Roots shared by several tools are NOT listed here — see `shared_cache_roots`.
fn tool_roots() -> Vec<(ToolDomain, PathBuf, &'static str)> {
    let Some(up) = userprofile() else {
        return Vec::new();
    };
    // nested `join()` — `/` segments are not portable separators.
    let appdata = up.join("AppData");
    let local = appdata.join("Local");
    let roaming = appdata.join("Roaming");
    let program_files = std::env::var_os("ProgramFiles(x86)")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .map(PathBuf::from);
    let mut out = tool_roots_from(&up, &local, &roaming, program_files.as_deref());
    // Firefox profiles are user data — listed for completeness but never auto-picked.
    out.retain(|(_, p, _)| p.exists());
    out
}

/// Roots several tools write into. Each is enumerated one level down and every
/// child is reported on its own, so a shared root is never itself a target.
fn shared_cache_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(up) = userprofile() {
        out.push(up.join(".cache"));
    }
    if let Some(local) = local("LOCALAPPDATA") {
        out.push(local.join("JetBrains"));
    }
    out.retain(|p| p.is_dir());
    out
}

/// Explicitly disposable subdirectories inside a JetBrains IDE home.
/// An IDE home also holds settings and user-installed plugins, so this is an
/// allow-list: anything not named here is left alone on purpose.
const JETBRAINS_SAFE_SUBDIRS: &[&str] = &["caches", "compile-server", "log", "tmp"];

/// One level below a shared cache root.
///
/// A JetBrains IDE home is recognised by having at least one safe subdirectory;
/// those subdirectories are reported as `Confirmed` because the allow-list above
/// names them. A JetBrains entry that is not an IDE home (a downloaded dist) and
/// every `~/.cache` child are reported as `Suspected` / `Medium`, since nothing
/// here proves the whole child is disposable — the user decides.
fn scan_shared_cache_children(
    root: &Path,
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    // Collect first, then order and truncate: a `take(n)` during enumeration
    // would drop a different subset depending on filesystem order.
    let mut children: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        if !crate::scan_task::checkpoint("files", children.len()) {
            return;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Junctions/mounts are never enumerated as targets; the delete gate
        // would refuse them anyway.
        if crate::fsutil::is_reparse_point(&path) {
            continue;
        }
        children.push(path);
    }
    for path in crate::fsutil::order_candidates(children)
        .into_iter()
        .take(TOOL_CACHE_CHILD_CAP)
    {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let safe: Vec<PathBuf> = JETBRAINS_SAFE_SUBDIRS
            .iter()
            .map(|sub| path.join(sub))
            .filter(|sub| sub.is_dir())
            .collect();
        if safe.is_empty() {
            push_item(out, scanned, path, ToolDomain::Dev, &name, false);
        } else {
            for sub in safe {
                if crate::scan_task::cancelled() {
                    return;
                }
                let leaf = sub
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                push_item(
                    out,
                    scanned,
                    sub,
                    ToolDomain::Dev,
                    &format!("{name}\\{leaf}"),
                    true,
                );
            }
        }
    }
}

fn push_item(
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    path: PathBuf,
    domain: ToolDomain,
    label: &str,
    // Whole directory is disposable and owned by one tool. When false the item
    // is a child of a shared cache root: reported, but never preselected.
    confirmed: bool,
) {
    // Non-UTF-8 never becomes a delete candidate (fail-closed).
    let Some(p) = crate::fsutil::path_utf8(&path) else {
        return;
    };
    let (risk, score) = if confirmed {
        (RiskLevel::Low, SCORE_CONFIRMED)
    } else {
        (RiskLevel::Medium, SCORE_SUSPECTED_MIN + 20)
    };
    out.push(CleanupItem {
        path: p.clone(),
        kind: ItemKind::Dir,
        score,
        confidence: if confirmed {
            Confidence::Confirmed
        } else {
            Confidence::Suspected
        },
        risk,
        reason: if confirmed {
            format!("tool cache ({})", domain.as_str())
        } else {
            "shared tool cache".into()
        },
        evidence: vec![Evidence {
            code: if confirmed {
                "tool_cache".into()
            } else {
                "tool_cache_shared".into()
            },
            label: domain.as_str().into(),
            weight: if confirmed { 50 } else { 30 },
            detail: label.into(),
        }],
        shared: false,
        user_data: crate::safety::is_user_data_path(&p),
        user_library: crate::safety::is_user_library_path(&p)
            && !crate::safety::is_user_data_path(&p),
        size_kb: None,
        bucket: Some(domain.as_str().into()),
    });
    scanned.insert(p);
}

/// Scan specialty tool caches. Results feed the toolcache allow-list.
pub fn scan_tool_caches() -> Vec<CleanupItem> {
    let mut out = Vec::new();
    let mut scanned = HashSet::new();
    if !crate::scan_task::checkpoint("files", 0) {
        return out;
    }
    for (domain, path, label) in tool_roots() {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return out;
        }
        if label.contains("skip") || label.contains("profile") {
            // Surface as keep-only high-risk if we ever list it; currently skip entirely.
            continue;
        }
        push_item(&mut out, &mut scanned, path, domain, label, true);
        if out.len() >= SPECIALTY_RESULT_CAP {
            break;
        }
    }
    // Each shared root bounds itself, so neither crowds the other out.
    for root in shared_cache_roots() {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return out;
        }
        scan_shared_cache_children(&root, &mut out, &mut scanned);
    }
    if !crate::scan_task::checkpoint("sizes", out.len()) {
        return out;
    }
    fill_item_sizes(&mut out);
    if !crate::scan_task::checkpoint("finalizing", out.len()) {
        return out;
    }
    fill_item_buckets(&mut out, "");
    scan_allow::remember(AllowScope::ToolCache, &scanned);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "remova-toolcache-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).expect("create fixture root");
        p
    }

    #[test]
    fn domains_label() {
        assert_eq!(ToolDomain::Dev.as_str(), "dev");
        assert_eq!(ToolDomain::Game.as_str(), "game");
        assert_eq!(ToolDomain::Browser.as_str(), "browser");
    }

    /// A wrong path in the fixed list silently disappears behind the `exists()`
    /// filter, so the list is asserted against a synthetic home instead.
    #[test]
    fn fixed_roots_point_at_real_cache_locations() {
        let root = unique_temp("roots");
        let up = root.join("home");
        let local = up.join("AppData").join("Local");
        let roaming = up.join("AppData").join("Roaming");
        std::fs::create_dir_all(&local).expect("create local");
        std::fs::create_dir_all(&roaming).expect("create roaming");

        let expected: &[(&str, &str)] = &[
            ("npm-cache", "AppData/Local/npm-cache"),
            ("pip-cache", "AppData/Local/pip/cache"),
            ("uv-cache", "AppData/Local/uv/cache"),
            ("pnpm-cache", "AppData/Local/pnpm-cache"),
            ("pnpm-store", "AppData/Local/pnpm/store"),
            ("bun-cache", ".bun/install/cache"),
            ("go-mod-cache", "go/pkg/mod"),
            ("maven-cache", ".m2/repository"),
            ("nuget-packages", ".nuget/packages"),
            ("cargo-cache", ".cargo/registry/cache"),
            ("gradle-cache", "AppData/Local/gradle/caches"),
            ("vscode-cache", "AppData/Roaming/Code/Cache"),
            ("vscode-cached-data", "AppData/Roaming/Code/CachedData"),
        ];
        for (_, rel) in expected {
            std::fs::create_dir_all(up.join(rel)).expect("create fixture cache");
        }

        let roots = tool_roots_from(&up, &local, &roaming, None);
        for (label, rel) in expected {
            let want = up.join(rel);
            assert!(want.is_dir(), "fixture missing {rel}");
            assert!(
                roots.iter().any(|(_, p, l)| *l == *label && p == &want),
                "root {label} does not point at {rel}"
            );
        }
        // The old rule looked for `pnpm\cache`, which no pnpm install ever
        // creates — it must not come back.
        let stale = Path::new("pnpm").join("cache");
        assert!(!roots.iter().any(|(_, p, _)| p.ends_with(&stale)));

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    /// A shared root must be broken into children, never offered whole.
    #[test]
    fn shared_cache_root_is_never_a_target_itself() {
        let root = unique_temp("shared");
        let cache = root.join(".cache");
        for child in ["codex-runtimes", "opencode", "huggingface"] {
            std::fs::create_dir_all(cache.join(child).join("inner")).expect("create child");
        }
        std::fs::write(cache.join("loose-file"), b"fixture").expect("create loose file");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        scan_shared_cache_children(&cache, &mut out, &mut scanned);

        assert_eq!(out.len(), 3, "one item per child directory");
        let root_utf8 = crate::fsutil::path_utf8(&cache).expect("utf8 root");
        assert!(
            !out.iter().any(|it| it.path == root_utf8),
            "the shared root itself must never be a cleanup target"
        );
        assert!(out.iter().all(|it| !it.path.ends_with("loose-file")));
        // Nothing here proves a child is wholly disposable, so nothing is preselected.
        for it in &out {
            assert_eq!(it.confidence, Confidence::Suspected);
            assert_eq!(it.risk, RiskLevel::Medium);
            assert_eq!(it.reason, "shared tool cache");
            assert_eq!(it.evidence[0].code, "tool_cache_shared");
            assert_eq!(it.kind, ItemKind::Dir);
        }

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    /// An IDE home mixes disposable caches with settings and plugins; only the
    /// named subdirectories may be offered.
    #[test]
    fn jetbrains_lists_only_known_safe_subdirs() {
        let root = unique_temp("jetbrains");
        let jb = root.join("JetBrains");
        let ide = jb.join("PyCharm2025.1");
        for sub in JETBRAINS_SAFE_SUBDIRS {
            std::fs::create_dir_all(ide.join(sub)).expect("create safe subdir");
        }
        for sub in ["options", "plugins", "extensions"] {
            std::fs::create_dir_all(ide.join(sub)).expect("create config subdir");
        }
        // A downloaded dist, not an IDE home — no safe subdirs to find.
        std::fs::create_dir_all(jb.join("JetBrainsClientDist")).expect("create dist");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        scan_shared_cache_children(&jb, &mut out, &mut scanned);

        assert_eq!(out.len(), JETBRAINS_SAFE_SUBDIRS.len() + 1);
        for it in &out {
            let in_ide = it.path.contains("PyCharm2025.1");
            if in_ide {
                assert_eq!(it.confidence, Confidence::Confirmed, "{}", it.path);
                assert_eq!(it.risk, RiskLevel::Low, "{}", it.path);
            } else {
                assert_eq!(it.confidence, Confidence::Suspected, "{}", it.path);
                assert_eq!(it.risk, RiskLevel::Medium, "{}", it.path);
            }
        }
        for it in &out {
            for sub in ["options", "plugins", "extensions"] {
                assert!(
                    !it.path.contains(sub),
                    "config subdir {sub} must stay untouched"
                );
            }
        }

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    #[test]
    fn shared_cache_children_are_ordered_and_bounded() {
        let root = unique_temp("ordered");
        // Created in an order that no filesystem is obliged to preserve, and
        // past the child cap so truncation has to pick deterministically.
        for i in 0..(TOOL_CACHE_CHILD_CAP + 12) {
            std::fs::create_dir(root.join(format!("tool-{i:03}-cache"))).expect("create child");
        }
        std::fs::create_dir(root.join("aaa-first")).expect("create first");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        scan_shared_cache_children(&root, &mut out, &mut scanned);

        assert_eq!(out.len(), TOOL_CACHE_CHILD_CAP);
        assert_eq!(scanned.len(), TOOL_CACHE_CHILD_CAP);
        // Sorted case-insensitively, so the same subset is chosen every run.
        let paths: Vec<&str> = out.iter().map(|it| it.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_by_key(|p| p.to_lowercase());
        assert_eq!(paths, sorted, "children must be reported in a stable order");
        // The cap keeps the alphabetically-first children and drops the last
        // ones — not an arbitrary filesystem-order subset.
        assert!(
            paths[0].ends_with("aaa-first"),
            "first child is {}",
            paths[0]
        );
        assert!(paths.iter().any(|p| p.ends_with("tool-000-cache")));
        assert!(!paths.iter().any(|p| p.ends_with("tool-071-cache")));

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    #[test]
    fn scan_runs_under_cap() {
        let _allow = crate::scan_allow::test_lock();
        let items = scan_tool_caches();
        assert!(items.len() <= SPECIALTY_RESULT_CAP);
        for it in &items {
            assert!(!it.path.is_empty());
            // User-data red lines are never default-selectable, whatever the score.
            if it.user_data || it.user_library {
                assert_eq!(it.confidence, Confidence::Suspected);
            }
        }
    }
}
