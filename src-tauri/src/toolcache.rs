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

/// Cache locations each tool can be told to use.
///
/// Every one of these tools has an official relocation mechanism, usually so the
/// cache can live on another drive. Ignoring it means the scan silently finds
/// nothing for anyone who moved their cache — the same class of miss as a wrong
/// hardcoded path, just triggered by configuration instead.
#[derive(Debug, Default)]
struct ToolCacheEnv {
    cargo_home: Option<PathBuf>,
    /// May be a `;`-separated list on Windows; the first entry wins.
    gopath: Option<PathBuf>,
    pip_cache_dir: Option<PathBuf>,
    nuget_packages: Option<PathBuf>,
    gradle_user_home: Option<PathBuf>,
    uv_cache_dir: Option<PathBuf>,
    pnpm_home: Option<PathBuf>,
    npm_cache: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
}

impl ToolCacheEnv {
    fn from_process() -> Self {
        let first_path = |key: &str| {
            local(key)
                .map(|p| {
                    p.to_string_lossy()
                        .split(';')
                        .find(|s| !s.trim().is_empty())
                        .unwrap_or_default()
                        .to_string()
                })
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        };
        Self {
            cargo_home: first_path("CARGO_HOME"),
            gopath: first_path("GOPATH"),
            pip_cache_dir: first_path("PIP_CACHE_DIR"),
            nuget_packages: first_path("NUGET_PACKAGES"),
            gradle_user_home: first_path("GRADLE_USER_HOME"),
            uv_cache_dir: first_path("UV_CACHE_DIR"),
            pnpm_home: first_path("PNPM_HOME"),
            npm_cache: first_path("npm_config_cache"),
            xdg_cache_home: first_path("XDG_CACHE_HOME"),
        }
    }
}

/// Build the fixed single-owner cache list from explicit bases.
///
/// Kept separate from `tool_roots` so tests can supply a synthetic home and
/// environment: a path that is simply wrong vanishes from `tool_roots` via the
/// `exists()` filter and is invisible otherwise, which is how the old
/// `pnpm\cache` rule went unnoticed.
fn tool_roots_from(
    up: &Path,
    local: &Path,
    roaming: &Path,
    program_files: Option<&Path>,
    env: &ToolCacheEnv,
) -> Vec<(ToolDomain, PathBuf, &'static str)> {
    // Each entry prefers the tool's own relocation variable over its default.
    let npm_cache = env
        .npm_cache
        .clone()
        .unwrap_or_else(|| local.join("npm-cache"));
    let pip_cache = env
        .pip_cache_dir
        .clone()
        .unwrap_or_else(|| local.join("pip").join("cache"));
    let nuget_v3 = local.join("NuGet").join("v3-cache");
    let nuget_packages = env
        .nuget_packages
        .clone()
        .unwrap_or_else(|| up.join(".nuget").join("packages"));
    let yarn_cache = local.join("Yarn").join("Cache");
    let yarn_berry = up.join(".yarn").join("berry").join("cache");
    let pnpm_stores: Vec<PathBuf> = env
        .pnpm_home
        .clone()
        .map(|home| vec![home.join("store")])
        .unwrap_or_else(|| {
            vec![
                local.join("pnpm-cache"),
                local.join("pnpm").join("store"),
                local.join("pnpm-store"),
            ]
        });
    let uv_cache = env
        .uv_cache_dir
        .clone()
        .unwrap_or_else(|| local.join("uv").join("cache"));
    let bun_cache = up.join(".bun").join("install").join("cache");
    let go_mod = env
        .gopath
        .clone()
        .map(|p| p.join("pkg").join("mod"))
        .unwrap_or_else(|| up.join("go").join("pkg").join("mod"));
    let maven_repo = up.join(".m2").join("repository");
    let cargo_cache = env
        .cargo_home
        .clone()
        .map(|h| h.join("registry").join("cache"))
        .unwrap_or_else(|| up.join(".cargo").join("registry").join("cache"));
    // Gradle's user home is `~/.gradle`, not `%LOCALAPPDATA%\gradle`.
    let gradle_caches = env
        .gradle_user_home
        .clone()
        .map(|h| h.join("caches"))
        .unwrap_or_else(|| up.join(".gradle").join("caches"));
    let mut out = vec![
        (ToolDomain::Dev, npm_cache, "npm-cache"),
        (ToolDomain::Dev, pip_cache, "pip-cache"),
        (ToolDomain::Dev, nuget_v3, "nuget-cache"),
        (ToolDomain::Dev, yarn_cache, "yarn-cache"),
        (ToolDomain::Dev, yarn_berry, "yarn-berry-cache"),
        (ToolDomain::Dev, uv_cache, "uv-cache"),
        (ToolDomain::Dev, bun_cache, "bun-cache"),
        (ToolDomain::Dev, go_mod, "go-mod-cache"),
        (ToolDomain::Dev, maven_repo, "maven-cache"),
        (ToolDomain::Dev, nuget_packages, "nuget-packages"),
        (ToolDomain::Dev, cargo_cache, "cargo-cache"),
        (ToolDomain::Dev, gradle_caches, "gradle-cache"),
    ];
    out.extend(
        pnpm_stores
            .into_iter()
            .map(|p| (ToolDomain::Dev, p, "pnpm-store")),
    );
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
    let mut out = tool_roots_from(
        &up,
        &local,
        &roaming,
        program_files.as_deref(),
        &ToolCacheEnv::from_process(),
    );
    // Firefox profiles are user data — listed for completeness but never auto-picked.
    out.retain(|(_, p, _)| p.exists());
    out
}

/// Candidate shared cache roots for a given environment.
fn shared_cache_roots_from(env: &ToolCacheEnv) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(up) = userprofile() {
        // `~/.cache` is the XDG default; tools honour XDG_CACHE_HOME when set.
        out.push(
            env.xdg_cache_home
                .clone()
                .unwrap_or_else(|| up.join(".cache")),
        );
    }
    if let Some(local) = local("LOCALAPPDATA") {
        out.push(local.join("JetBrains"));
    }
    out
}

/// Roots several tools write into. Each is enumerated one level down and every
/// child is reported on its own, so a shared root is never itself a target.
fn shared_cache_roots() -> Vec<PathBuf> {
    let mut out = shared_cache_roots_from(&ToolCacheEnv::from_process());
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
            ("bun-cache", ".bun/install/cache"),
            ("go-mod-cache", "go/pkg/mod"),
            ("maven-cache", ".m2/repository"),
            ("nuget-packages", ".nuget/packages"),
            ("cargo-cache", ".cargo/registry/cache"),
            ("gradle-cache", ".gradle/caches"),
            ("vscode-cache", "AppData/Roaming/Code/Cache"),
            ("vscode-cached-data", "AppData/Roaming/Code/CachedData"),
        ];
        for (_, rel) in expected {
            std::fs::create_dir_all(up.join(rel)).expect("create fixture cache");
        }

        let roots = tool_roots_from(&up, &local, &roaming, None, &ToolCacheEnv::default());
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
        // Gradle's user home is `~/.gradle`, not under LocalAppData.
        let wrong_gradle = Path::new("gradle").join("caches");
        assert!(!roots
            .iter()
            .any(|(_, p, _)| p.starts_with(&local) && p.ends_with(&wrong_gradle)));

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    /// Each tool's own relocation variable must win over its default, or the
    /// scan finds nothing for anyone who moved a cache to another drive.
    #[test]
    fn relocation_variables_override_default_locations() {
        let root = unique_temp("relocated");
        let up = root.join("home");
        let local = up.join("AppData").join("Local");
        let roaming = up.join("AppData").join("Roaming");
        std::fs::create_dir_all(&local).expect("create local");
        std::fs::create_dir_all(&roaming).expect("create roaming");

        // A relocated toolchain on another drive, with the default left absent.
        let drive_d = root.join("d-drive");
        let cargo_home = drive_d.join("cargo");
        let gopath = drive_d.join("go-workspace");
        let gradle_home = drive_d.join("gradle-home");
        let uv_cache = drive_d.join("uv-cache");
        let pnpm_home = drive_d.join("pnpm-home");
        for p in [
            &cargo_home,
            &gopath,
            &gradle_home,
            &uv_cache,
            &pnpm_home,
            &drive_d.join("pip-cache"),
            &drive_d.join("nuget-packages"),
            &drive_d.join("npm-cache"),
        ] {
            std::fs::create_dir_all(p).expect("create relocated cache");
        }

        let env = ToolCacheEnv {
            cargo_home: Some(cargo_home.clone()),
            gopath: Some(gopath.clone()),
            pip_cache_dir: Some(drive_d.join("pip-cache")),
            nuget_packages: Some(drive_d.join("nuget-packages")),
            gradle_user_home: Some(gradle_home.clone()),
            uv_cache_dir: Some(uv_cache.clone()),
            pnpm_home: Some(pnpm_home.clone()),
            npm_cache: Some(drive_d.join("npm-cache")),
            xdg_cache_home: Some(drive_d.join("xdg-cache")),
        };
        let roots = tool_roots_from(&up, &local, &roaming, None, &env);
        let find = |label: &str| {
            roots
                .iter()
                .find(|(_, _, l)| *l == label)
                .map(|(_, p, _)| p.clone())
        };
        assert_eq!(
            find("cargo-cache"),
            Some(cargo_home.join("registry").join("cache"))
        );
        assert_eq!(find("go-mod-cache"), Some(gopath.join("pkg").join("mod")));
        assert_eq!(find("gradle-cache"), Some(gradle_home.join("caches")));
        assert_eq!(find("uv-cache"), Some(uv_cache));
        assert_eq!(find("pip-cache"), Some(drive_d.join("pip-cache")));
        assert_eq!(find("nuget-packages"), Some(drive_d.join("nuget-packages")));
        assert_eq!(find("npm-cache"), Some(drive_d.join("npm-cache")));
        // PNPM_HOME replaces the whole variant list with a single store.
        assert_eq!(find("pnpm-store"), Some(pnpm_home.join("store")));
        assert_eq!(
            roots.iter().filter(|(_, _, l)| *l == "pnpm-store").count(),
            1,
            "PNPM_HOME must replace the default variants, not add to them"
        );

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    /// A shared cache root follows XDG_CACHE_HOME when the tools that honour it
    /// have been pointed elsewhere.
    #[test]
    fn shared_cache_root_follows_xdg_cache_home() {
        // Only the resolver is exercised here; a real `~/.cache` on the test
        // machine would make an end-to-end assertion machine-dependent.
        let env = ToolCacheEnv {
            xdg_cache_home: Some(PathBuf::from(r"D:\cache-home")),
            ..ToolCacheEnv::default()
        };
        let root = shared_cache_roots_from(&env);
        assert!(
            root.iter().any(|p| p == Path::new(r"D:\cache-home")),
            "XDG_CACHE_HOME must replace ~/.cache, got {root:?}"
        );
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
