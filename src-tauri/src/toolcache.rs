//! Specialty tool-cache scan (dev / game / browser).
//!
//! Two kinds of root. Single-owner caches come from a fixed allow-list and are
//! reported whole. Roots several tools share (`~/.cache`, JetBrains) are
//! enumerated one level down so the shared root is never itself a target.
//! Never touches game saves, browser profiles, or package store indexes that
//! break tooling.
//!
//! **Base directories.** `%LOCALAPPDATA%` and `%APPDATA%` are read from the
//! environment, not rebuilt by joining `AppData\Local` onto `%USERPROFILE%` —
//! hardcoding the base while lecturing the tools about relocation is the
//! contradiction this module used to contain. `%USERPROFILE%`-relative entries
//! (`~/.m2`, `~/.nuget`, `~/.cargo`, `~/.gradle`, `~/.bun`, `~/go`, `~/.yarn`)
//! stay joined onto `USERPROFILE` because those tools publish their location as
//! an environment variable rather than a shell known folder (see `ToolCacheEnv`).
//!
//! **Relocation.** Most single-owner caches honour an environment variable that
//! names the cache directory, so a toolchain installed on another drive is still
//! found. The exceptions are stated where they live: `~/.m2/repository` (Maven's
//! repository location is the build setting `maven.repo.local`, not an
//! environment variable) and the structural editor/browser scans, which follow
//! the shell folders above instead of a variable.
//!
//! **Rules that are recognised but never offered.** A rule row carries a
//! [`RuleMode`]; `KeepOnly` means "we know what this is, and it is never a
//! deletion candidate". Firefox profiles are the case in point: they are user
//! data (bookmarks, logins, sessions), so they are not listed at all rather than
//! listed-and-ignored. The mode is a typed field precisely so that this decision
//! cannot be flipped by renaming a label.

use crate::constants::{SPECIALTY_RESULT_CAP, TOOL_CACHE_CHILD_CAP};
use crate::scan_allow::{self, AllowScope};
use crate::scanner::{
    fill_item_buckets, fill_item_sizes, CleanupItem, Confidence, Evidence, ItemKind, RiskLevel,
    ScanGaps, ScopedScanResult, SCORE_CONFIRMED, SCORE_SUSPECTED_MIN,
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

/// Read one environment variable as a path. `None` when unset or empty.
fn env_path(key: &str) -> Option<PathBuf> {
    let v = std::env::var_os(key)?;
    if v.is_empty() {
        return None;
    }
    Some(PathBuf::from(v))
}

fn userprofile() -> Option<PathBuf> {
    env_path("USERPROFILE")
}

/// Whether a rule row may be offered as a deletion candidate.
///
/// The rule table used to decide this by sniffing the label text
/// (`label.contains("skip") || label.contains("profile")`), so renaming a row
/// from `firefox-profiles-skip` to `firefox-profiles-keep` would have silently
/// promoted a keep-only rule into a preselected deletion. Carrying the mode in
/// the tuple makes the decision explicit per row and impossible to get wrong by
/// editing a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleMode {
    /// Single-owner disposable cache: reported whole, `Confirmed` / `Low`.
    Delete,
    /// Recognised but never reported as a deletion candidate (user data).
    KeepOnly,
}

/// Whether a rule of this mode may produce a cleanup item.
fn mode_offers_deletion(mode: RuleMode) -> bool {
    matches!(mode, RuleMode::Delete)
}

/// The relocation environment, and through it the resolved cache roots.
///
/// One type with two identities, deliberately. As *the environment* it is what
/// `from_process` read out of the process: each field is `None` when the user
/// never set that variable, and a set value is the tool's own answer for where
/// its cache lives. As *the resolved cache roots* it is what `tool_roots_from`
/// and `shared_cache_roots_from` turn those values into — every root this scan
/// can offer is either one of these fields or a default derived from a shell
/// folder, so there is no third source and no hidden one. Tests build it by hand
/// (`none()` plus the fields under test) instead of mutating the process
/// environment, which is what keeps the fixtures deterministic.
///
/// Most of these tools publish an environment variable naming their cache
/// directory, usually so the cache can live on another drive. Ignoring it means
/// the scan silently finds nothing for anyone who moved their cache — the same
/// class of miss as a wrong hardcoded path, just triggered by configuration
/// instead. The exceptions are documented on the fields that have none.
#[derive(Debug)]
struct ToolCacheEnv {
    cargo_home: Option<PathBuf>,
    /// May be a `;`-separated list on Windows; the first entry wins.
    gopath: Option<PathBuf>,
    pip_cache_dir: Option<PathBuf>,
    nuget_packages: Option<PathBuf>,
    /// NuGet's *HTTP* cache (`NuGet\v3-cache`), not the global-packages folder
    /// above — the two are separate directories with separate variables.
    nuget_http_cache: Option<PathBuf>,
    gradle_user_home: Option<PathBuf>,
    uv_cache_dir: Option<PathBuf>,
    pnpm_home: Option<PathBuf>,
    npm_cache: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
    /// Yarn 1 (`Yarn\Cache`). Yarn Berry only honours this when a project sets
    /// `enableGlobalCache: false` — see `yarn_global_folder` for the default.
    yarn_cache_folder: Option<PathBuf>,
    /// Yarn Berry's global folder (`<globalFolder>/berry/cache`): most Berry
    /// projects cache in the global folder, relocated by `YARN_GLOBAL_FOLDER`
    /// (default `~/.yarn/berry` on non-Windows, `%LOCALAPPDATA%\Yarn\Berry` on
    /// Windows — both fold to `~/.yarn/berry` here via the `up` default).
    yarn_global_folder: Option<PathBuf>,
    bun_install_cache: Option<PathBuf>,
}

impl ToolCacheEnv {
    /// No relocation variables set — the defaults every tool ships with.
    ///
    /// An explicit constructor rather than a derived `Default`: on a struct
    /// whose fields are all `Option`, `Default` reads as "an empty environment"
    /// and invites `..Default::default()` in a fixture that then forgets the one
    /// field under test. Production always reads the process environment, so
    /// this exists for the fixtures.
    #[cfg(test)]
    fn none() -> Self {
        Self {
            cargo_home: None,
            gopath: None,
            pip_cache_dir: None,
            nuget_packages: None,
            nuget_http_cache: None,
            gradle_user_home: None,
            uv_cache_dir: None,
            pnpm_home: None,
            npm_cache: None,
            xdg_cache_home: None,
            yarn_cache_folder: None,
            yarn_global_folder: None,
            bun_install_cache: None,
        }
    }

    fn from_process() -> Self {
        let first_path = |key: &str| {
            env_path(key)
                .map(|p| {
                    p.to_string_lossy()
                        .split(';')
                        .find(|s| !s.trim().is_empty())
                        .unwrap_or_default()
                        .to_string()
                })
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                // A relocated root becomes a delete target, so a value that
                // points somewhere implausible is dropped rather than trusted.
                .filter(|p| is_plausible_cache_root(p))
        };
        Self {
            cargo_home: first_path("CARGO_HOME"),
            gopath: first_path("GOPATH"),
            pip_cache_dir: first_path("PIP_CACHE_DIR"),
            nuget_packages: first_path("NUGET_PACKAGES"),
            nuget_http_cache: first_path("NUGET_HTTP_CACHE_PATH"),
            gradle_user_home: first_path("GRADLE_USER_HOME"),
            uv_cache_dir: first_path("UV_CACHE_DIR"),
            pnpm_home: first_path("PNPM_HOME"),
            npm_cache: first_path("npm_config_cache"),
            xdg_cache_home: first_path("XDG_CACHE_HOME"),
            yarn_cache_folder: first_path("YARN_CACHE_FOLDER"),
            yarn_global_folder: first_path("YARN_GLOBAL_FOLDER"),
            bun_install_cache: first_path("BUN_INSTALL_CACHE_DIR"),
        }
    }
}

/// Whether a user-supplied relocation variable names something that could be a
/// cache root.
///
/// These values become whole-directory delete targets (or, for
/// `XDG_CACHE_HOME`, an enumeration root), so `XDG_CACHE_HOME=D:\` must not
/// offer every top-level directory on the drive, and `GRADLE_USER_HOME=D:\Work`
/// must not turn a work tree into a preselected deletion. The check is
/// deliberately structural rather than a name list: an absolute drive path with
/// at least three components, so it can be neither a drive root nor a bare
/// library folder, and not a reparse point that resolves elsewhere.
fn is_plausible_cache_root(p: &Path) -> bool {
    if p.as_os_str().is_empty() || p.is_relative() {
        return false;
    }
    let comps: Vec<_> = p.components().collect();
    if comps.len() < 3 {
        return false;
    }
    // Reject a bare library root such as `C:\Users\x\Documents`.
    let last = comps
        .last()
        .and_then(|c| c.as_os_str().to_str())
        .unwrap_or_default()
        .to_lowercase();
    const LIBRARY_NAMES: &[&str] = &[
        "documents",
        "downloads",
        "desktop",
        "pictures",
        "videos",
        "music",
        "favorites",
        "contacts",
        "links",
        "saved games",
        "searches",
    ];
    if LIBRARY_NAMES.contains(&last.as_str()) {
        return false;
    }
    !crate::fsutil::is_reparse_point(p)
}

/// Build the fixed single-owner cache list from explicit bases.
///
/// Kept separate from `tool_roots` so tests can supply a synthetic home and
/// environment: a path that is simply wrong vanishes from `tool_roots` via the
/// `exists()` filter and is invisible otherwise, which is how the old
/// `pnpm\cache` rule went unnoticed.
///
/// Every row carries an explicit [`RuleMode`]; a new row must choose one.
fn tool_roots_from(
    up: &Path,
    local: &Path,
    program_files: Option<&Path>,
    env: &ToolCacheEnv,
) -> Vec<(ToolDomain, PathBuf, &'static str, RuleMode)> {
    // Each entry prefers the tool's own relocation variable over its default.
    let npm_cache = env
        .npm_cache
        .clone()
        .unwrap_or_else(|| local.join("npm-cache"));
    let pip_cache = env
        .pip_cache_dir
        .clone()
        .unwrap_or_else(|| local.join("pip").join("cache"));
    let nuget_v3 = env
        .nuget_http_cache
        .clone()
        .unwrap_or_else(|| local.join("NuGet").join("v3-cache"));
    let nuget_packages = env
        .nuget_packages
        .clone()
        .unwrap_or_else(|| up.join(".nuget").join("packages"));
    // Yarn 1 relocates its cache with YARN_CACHE_FOLDER. Yarn Berry only reads
    // that variable when a project opts out with enableGlobalCache:false — by
    // default it keeps the cache in <globalFolder>/cache, relocated with
    // YARN_GLOBAL_FOLDER. Feeding YARN_CACHE_FOLDER to berry too used to fold
    // both rules onto the same path whenever the variable was set.
    let yarn_cache = env
        .yarn_cache_folder
        .clone()
        .unwrap_or_else(|| local.join("Yarn").join("Cache"));
    let yarn_berry = env
        .yarn_global_folder
        .as_ref()
        .map(|p| p.join("berry").join("cache"))
        .unwrap_or_else(|| up.join(".yarn").join("berry").join("cache"));
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
    // Bun names its install cache directory itself, not a cache inside it.
    let bun_cache = env
        .bun_install_cache
        .clone()
        .unwrap_or_else(|| up.join(".bun").join("install").join("cache"));
    let go_mod = env
        .gopath
        .clone()
        .map(|p| p.join("pkg").join("mod"))
        .unwrap_or_else(|| up.join("go").join("pkg").join("mod"));
    // No environment variable: Maven's repository location is the build setting
    // `maven.repo.local`, which only a build file can set.
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
        (ToolDomain::Dev, npm_cache, "npm-cache", RuleMode::Delete),
        (ToolDomain::Dev, pip_cache, "pip-cache", RuleMode::Delete),
        (ToolDomain::Dev, nuget_v3, "nuget-cache", RuleMode::Delete),
        (ToolDomain::Dev, yarn_cache, "yarn-cache", RuleMode::Delete),
        (
            ToolDomain::Dev,
            yarn_berry,
            "yarn-berry-cache",
            RuleMode::Delete,
        ),
        (ToolDomain::Dev, uv_cache, "uv-cache", RuleMode::Delete),
        (ToolDomain::Dev, bun_cache, "bun-cache", RuleMode::Delete),
        (ToolDomain::Dev, go_mod, "go-mod-cache", RuleMode::Delete),
        (ToolDomain::Dev, maven_repo, "maven-cache", RuleMode::Delete),
        (
            ToolDomain::Dev,
            nuget_packages,
            "nuget-packages",
            RuleMode::Delete,
        ),
        (
            ToolDomain::Dev,
            cargo_cache,
            "cargo-cache",
            RuleMode::Delete,
        ),
        (
            ToolDomain::Dev,
            gradle_caches,
            "gradle-cache",
            RuleMode::Delete,
        ),
    ];
    out.extend(
        pnpm_stores
            .into_iter()
            .map(|p| (ToolDomain::Dev, p, "pnpm-store", RuleMode::Delete)),
    );
    if let Some(pf) = program_files {
        out.push((
            ToolDomain::Game,
            pf.join("Steam").join("steamapps").join("shadercache"),
            "steam-shadercache",
            RuleMode::Delete,
        ));
    }
    out
}

/// Fixed relative roots per domain (joined with env bases). Missing paths are skipped.
///
/// Every entry here is a single-owner cache whose whole directory is disposable,
/// which is what earns it `Confirmed` / `Low` and the UI's default selection.
/// Roots shared by several tools are NOT listed here — see `shared_cache_roots`.
fn tool_roots(degraded: &mut Vec<String>) -> Vec<(ToolDomain, PathBuf, &'static str, RuleMode)> {
    let Some(up) = userprofile() else {
        degraded
            .push("USERPROFILE is not set; caches under the user profile were not scanned".into());
        return Vec::new();
    };
    // The shell folders are the authority for where this user's AppData lives;
    // rebuilding them by joining onto `USERPROFILE` is exactly the hardcoding
    // this module asks the tools not to do. The fallback only fires on an
    // account with no AppData variables at all, where nothing is scannable.
    let local = env_path("LOCALAPPDATA").unwrap_or_else(|| up.join("AppData").join("Local"));
    let roaming = env_path("APPDATA").unwrap_or_else(|| up.join("AppData").join("Roaming"));
    let program_files = env_path("ProgramFiles(x86)").or_else(|| env_path("ProgramFiles"));
    let mut out = tool_roots_from(
        &up,
        &local,
        program_files.as_deref(),
        &ToolCacheEnv::from_process(),
    );
    out.extend(vscode_family_caches(&roaming, degraded));
    out.extend(chromium_profile_caches(&local, degraded));
    out.retain(|(_, p, _, _)| p.exists());
    out
}

/// VS Code and its forks keep two cache directories side by side under
/// `%APPDATA%`. That pairing is the signature used here, rather than a list of
/// product names: forks (VSCodium, Cursor, Windsurf, Positron, …) are then
/// covered without being named, and a list would have to be maintained.
fn vscode_family_caches(
    roaming: &Path,
    degraded: &mut Vec<String>,
) -> Vec<(ToolDomain, PathBuf, &'static str, RuleMode)> {
    let Ok(rd) = std::fs::read_dir(roaming) else {
        // Not-found is just "no editors here"; anything else is a root we could
        // not look at, which must not read as "no editor caches".
        if std::fs::metadata(roaming).is_ok() {
            degraded.push(crate::scanner::unreadable_root_code(roaming));
        }
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return out;
        }
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        // A junction here would resolve the caches somewhere other than where
        // the reported path says they are.
        if crate::fsutil::is_reparse_point(&dir) {
            continue;
        }
        let cache = dir.join("Cache");
        let cached_data = dir.join("CachedData");
        if !(cache.is_dir() && cached_data.is_dir()) {
            continue;
        }
        out.push((ToolDomain::Browser, cache, "editor-cache", RuleMode::Delete));
        out.push((
            ToolDomain::Browser,
            cached_data,
            "editor-cached-data",
            RuleMode::Delete,
        ));
    }
    out.sort_by_cached_key(|(_, p, _, _)| p.to_string_lossy().to_lowercase());
    out
}

/// Chromium-based browsers and their User Data roots, as path segments under
/// `%LOCALAPPDATA%`. Chrome and Edge were the only two covered before; the rest
/// of the mainstream family uses the same layout.
///
/// This one is a list rather than a structural scan: recognising a browser by
/// walking `%LOCALAPPDATA%\*\User Data\*` would mean enumerating hundreds of
/// unrelated directories, and `User Data` alone is not browser-specific. A new
/// mainstream Chromium browser is rare, so it is added here.
const CHROMIUM_USER_DATA_ROOTS: &[&[&str]] = &[
    &["Google", "Chrome", "User Data"],
    &["Microsoft", "Edge", "User Data"],
    &["BraveSoftware", "Brave-Browser", "User Data"],
    &["Vivaldi", "User Data"],
    &["Chromium", "User Data"],
    &["Opera Software", "Opera Stable"],
    &["Opera Software", "Opera GX Stable"],
];

/// Disposable cache directories inside a Chromium profile.
const CHROMIUM_PROFILE_CACHES: &[&str] = &["Cache", "Code Cache", "GPUCache"];

/// Cache directories for every profile of every known Chromium browser.
///
/// Only `Default` and numbered `Profile N` are enumerated — the other entries in
/// User Data are settings, extensions and local state.
fn chromium_profile_caches(
    local: &Path,
    degraded: &mut Vec<String>,
) -> Vec<(ToolDomain, PathBuf, &'static str, RuleMode)> {
    let mut out = Vec::new();
    for segments in CHROMIUM_USER_DATA_ROOTS {
        let mut root = local.to_path_buf();
        for segment in *segments {
            root = root.join(segment);
        }
        let Ok(rd) = std::fs::read_dir(&root) else {
            // A known browser root that exists but cannot be read is a real gap;
            // one that is simply absent is not.
            if std::fs::metadata(&root).is_ok() {
                degraded.push(crate::scanner::unreadable_root_code(&root));
            }
            continue;
        };
        for entry in rd.flatten() {
            if !crate::scan_task::checkpoint("files", out.len()) {
                return out;
            }
            let profile = entry.path();
            if !profile.is_dir() {
                continue;
            }
            if crate::fsutil::is_reparse_point(&profile) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let numbered = name
                .strip_prefix("Profile ")
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
            if name != "Default" && !numbered {
                continue;
            }
            for cache in CHROMIUM_PROFILE_CACHES {
                let cand = profile.join(cache);
                if cand.is_dir() {
                    out.push((ToolDomain::Browser, cand, "browser-cache", RuleMode::Delete));
                }
            }
        }
    }
    out.sort_by_cached_key(|(_, p, _, _)| p.to_string_lossy().to_lowercase());
    out
}

/// Candidate shared cache roots for a given profile and environment.
///
/// `up` is passed in rather than re-read here so the caller's notion of the
/// profile is the only one; `%LOCALAPPDATA%` still comes from the environment,
/// because that is the shell's answer for the JetBrains root.
fn shared_cache_roots_from(up: &Path, env: &ToolCacheEnv) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // `~/.cache` is the XDG default; tools honour XDG_CACHE_HOME when set.
    out.push(
        env.xdg_cache_home
            .clone()
            .unwrap_or_else(|| up.join(".cache")),
    );
    if let Some(local) = env_path("LOCALAPPDATA") {
        out.push(local.join("JetBrains"));
    }
    out
}

/// Roots several tools write into. Each is enumerated one level down and every
/// child is reported on its own, so a shared root is never itself a target.
fn shared_cache_roots(degraded: &mut Vec<String>) -> Vec<PathBuf> {
    if userprofile().is_none() {
        degraded.push(crate::scanner::DEGRADED_USERPROFILE_NOT_SET.to_string());
    }
    if env_path("LOCALAPPDATA").is_none() {
        degraded.push(crate::scanner::DEGRADED_LOCALAPPDATA_NOT_SET.to_string());
    }
    let env = ToolCacheEnv::from_process();
    let mut out = match userprofile() {
        Some(up) => shared_cache_roots_from(&up, &env),
        // Without a profile there is no `~/.cache` default, but a relocated
        // XDG_CACHE_HOME still names a real root.
        None => env.xdg_cache_home.into_iter().collect(),
    };
    out.retain(|p| p.is_dir());
    out
}

/// Explicitly disposable subdirectories inside a JetBrains IDE home.
/// An IDE home also holds settings and user-installed plugins, so this is an
/// allow-list: anything not named here is left alone on purpose.
const JETBRAINS_SAFE_SUBDIRS: &[&str] = &["caches", "compile-server", "log", "tmp"];

/// One level below a shared cache root.
///
/// `jetbrains_home` is the only root whose children may be promoted to
/// `Confirmed` / `Low`, because it is the only one where a fixed allow-list of
/// subdirectory names is meaningful. Every other shared root — `~/.cache` above
/// all — holds unrelated tools' data, so its children stay `Suspected` /
/// `Medium`: a child that merely happens to contain a `caches` or `tmp`
/// subdirectory must not become a preselected delete target.
fn scan_shared_cache_children(
    root: &Path,
    jetbrains_home: bool,
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    report: &mut ScanGaps,
) {
    let Ok(rd) = std::fs::read_dir(root) else {
        // A root that exists but cannot be read is not an empty root: saying
        // nothing would read as "this machine has no tool caches here".
        report.degrade(crate::scanner::unreadable_root_code(root));
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
        // PERF-C: one buffered metadata read answers both the directory and the
        // reparse-bit questions instead of two extra syscalls per child.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() || crate::dirsize::is_reparse_metadata(&meta) {
            continue;
        }
        children.push(path);
    }
    let children = crate::fsutil::order_candidates(children);
    // CQ-8: `omitted` is rows, not children. A JetBrains child expands into its
    // cache subdirectories, so a dropped child can hide more than one row.
    let dropped = children.len().saturating_sub(TOOL_CACHE_CHILD_CAP);
    if dropped > 0 {
        let rows = if jetbrains_home {
            children
                .iter()
                .skip(TOOL_CACHE_CHILD_CAP)
                .map(|p| {
                    let n = JETBRAINS_SAFE_SUBDIRS
                        .iter()
                        .filter(|sub| p.join(sub).is_dir())
                        .count();
                    n.max(1)
                })
                .sum()
        } else {
            dropped
        };
        report.omit(rows);
    }
    for path in children.into_iter().take(TOOL_CACHE_CHILD_CAP) {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        // Only a JetBrains IDE home gets the subdirectory allow-list. On any
        // other shared root the child is reported whole and unconfirmed, so a
        // directory that merely contains a `caches`/`tmp` subdirectory can never
        // become a preselected delete target.
        let safe: Vec<PathBuf> = if jetbrains_home {
            JETBRAINS_SAFE_SUBDIRS
                .iter()
                .map(|sub| path.join(sub))
                .filter(|sub| sub.is_dir())
                .collect()
        } else {
            Vec::new()
        };
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
        service_bound: false,
        user_data: crate::safety::is_user_data_path(&p),
        user_library: crate::safety::is_user_library_path(&p)
            && !crate::safety::is_user_data_path(&p),
        size_kb: None,
        size_capped: false,
        bucket: Some(domain.as_str().into()),
    });
    scanned.insert(p);
}

/// Scan specialty tool caches. Results feed the toolcache allow-list.
///
/// Returns `Result` so the caller can tell a scan that ran from one that never
/// started; every in-scan shortfall is reported inside the result as `omitted`
/// (a cap dropped rows) or `degraded` (a root could not be read), never as an
/// empty item list that reads as "this machine is clean".
pub fn scan_tool_caches() -> Result<ScopedScanResult, String> {
    let mut out = Vec::new();
    let mut scanned = HashSet::new();
    let mut report = ScanGaps::default();
    if !crate::scan_task::checkpoint("files", 0) {
        return Ok(ScopedScanResult::empty());
    }
    let roots = tool_roots(&mut report.degraded);
    for (domain, path, label, mode) in roots {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return Ok(ScopedScanResult::empty());
        }
        if !mode_offers_deletion(mode) {
            // Recognised but never offered — see `RuleMode::KeepOnly`.
            continue;
        }
        push_item(&mut out, &mut scanned, path, domain, label, true);
    }
    // Each shared root bounds itself, so neither crowds the other out.
    for root in shared_cache_roots(&mut report.degraded) {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return Ok(ScopedScanResult::empty());
        }
        // The JetBrains root is the only one whose children may be promoted by a
        // fixed subdirectory allow-list; see `scan_shared_cache_children`.
        let jetbrains_home = root
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("JetBrains"));
        scan_shared_cache_children(&root, jetbrains_home, &mut out, &mut scanned, &mut report);
    }
    // Bound the whole scan, not just the fixed-root phase, and keep the
    // allow-list in step with what is actually reported. Before this the
    // shared-root phase could push the total past `SPECIALTY_RESULT_CAP`, so
    // the cap documented on the constant was not what the code guaranteed.
    if out.len() > SPECIALTY_RESULT_CAP {
        let reported: HashSet<String> = out[..SPECIALTY_RESULT_CAP]
            .iter()
            .map(|it| it.path.clone())
            .collect();
        report.omit(out.len() - SPECIALTY_RESULT_CAP);
        out.truncate(SPECIALTY_RESULT_CAP);
        scanned.retain(|p| reported.contains(p));
    }
    if !crate::scan_task::checkpoint("sizes", out.len()) {
        return Ok(ScopedScanResult::empty());
    }
    crate::scanner::reg_scans::mark_service_bound(&mut out);
    fill_item_sizes(&mut out);
    if !crate::scan_task::checkpoint("finalizing", out.len()) {
        return Ok(ScopedScanResult::empty());
    }
    fill_item_buckets(&mut out, "");
    // Armed from exactly what is reported: a truncated or cancelled candidate is
    // never authorized.
    scan_allow::remember(AllowScope::ToolCache, &scanned);
    Ok(ScopedScanResult {
        items: out,
        omitted: report.omitted,
        degraded: report.degraded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture root that removes itself on drop, including on panic. The
    /// hand-rolled `remove_dir_all` at the end of each test body used to be skipped
    /// by every failing assertion.
    fn unique_temp(tag: &str) -> crate::test_support::TempDir {
        crate::test_support::TempDir::new(&format!("toolcache-{tag}"))
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
            ("nuget-cache", "AppData/Local/NuGet/v3-cache"),
            ("yarn-cache", "AppData/Local/Yarn/Cache"),
            ("yarn-berry-cache", ".yarn/berry/cache"),
            ("uv-cache", "AppData/Local/uv/cache"),
            ("bun-cache", ".bun/install/cache"),
            ("go-mod-cache", "go/pkg/mod"),
            ("maven-cache", ".m2/repository"),
            ("nuget-packages", ".nuget/packages"),
            ("cargo-cache", ".cargo/registry/cache"),
            ("gradle-cache", ".gradle/caches"),
            ("pnpm-store", "AppData/Local/pnpm/store"),
        ];
        for (_, rel) in expected {
            std::fs::create_dir_all(up.join(rel)).expect("create fixture cache");
        }

        let roots = tool_roots_from(&up, &local, None, &ToolCacheEnv::none());
        for (label, rel) in expected {
            let want = up.join(rel);
            assert!(want.is_dir(), "fixture missing {rel}");
            assert!(
                roots.iter().any(|(_, p, l, _)| *l == *label && p == &want),
                "root {label} does not point at {rel}"
            );
        }
        // The old rule looked for `pnpm\cache`, which no pnpm install ever
        // creates — it must not come back.
        let stale = Path::new("pnpm").join("cache");
        assert!(!roots.iter().any(|(_, p, _, _)| p.ends_with(&stale)));
        // Gradle's user home is `~/.gradle`, not under LocalAppData.
        let wrong_gradle = Path::new("gradle").join("caches");
        assert!(!roots
            .iter()
            .any(|(_, p, _, _)| p.starts_with(&local) && p.ends_with(&wrong_gradle)));
    }

    /// The label set is exhaustive, and every row states its own mode.
    ///
    /// This is the guard that replaced `label.contains("skip") ||
    /// label.contains("profile")`: a rule used to be recognised as
    /// never-deletable by the *text* of its label, so renaming
    /// `firefox-profiles-skip` to `firefox-profiles-keep` would have turned a
    /// keep-only row into a preselected deletion without touching any logic.
    /// Adding a row to the table now fails here until its label and its
    /// [`RuleMode`] are recorded, and a label may never carry two modes.
    #[test]
    fn every_rule_label_declares_an_explicit_mode() {
        let root = unique_temp("labels");
        let up = root.join("home");
        let local = up.join("AppData").join("Local");
        let roaming = up.join("AppData").join("Roaming");
        std::fs::create_dir_all(&local).expect("create local");
        std::fs::create_dir_all(&roaming).expect("create roaming");

        // Sorted. `steam-shadercache` needs `ProgramFiles`, which the fixture
        // does not supply, so it is asserted separately below.
        const EXPECTED: &[&str] = &[
            "bun-cache",
            "cargo-cache",
            "go-mod-cache",
            "gradle-cache",
            "maven-cache",
            "npm-cache",
            "nuget-cache",
            "nuget-packages",
            "pip-cache",
            "pnpm-store",
            "uv-cache",
            "yarn-berry-cache",
            "yarn-cache",
        ];
        let roots = tool_roots_from(&up, &local, None, &ToolCacheEnv::none());
        let mut labels: Vec<&'static str> = roots.iter().map(|(_, _, l, _)| *l).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels, EXPECTED,
            "rule table changed: add the new label here together with its RuleMode"
        );

        // One label, one mode — never two.
        let mut by_label: Vec<(&str, RuleMode)> = Vec::new();
        for (_, _, label, mode) in &roots {
            match by_label.iter_mut().find(|(l, _)| l == label) {
                Some((_, seen)) => assert_eq!(*seen, *mode, "{label} carries two modes"),
                None => by_label.push((label, *mode)),
            }
        }
        // Pin the mapping itself: `KeepOnly` must never reach `push_item`, so a
        // future mode cannot quietly become deletable.
        assert!(mode_offers_deletion(RuleMode::Delete));
        assert!(!mode_offers_deletion(RuleMode::KeepOnly));
        for (_, mode) in &by_label {
            assert!(
                mode_offers_deletion(*mode),
                "a fixed single-owner cache is disposable by definition; use KeepOnly only for user data"
            );
        }

        // The ProgramFiles-dependent row is covered too, so the table is whole.
        let pf = root.join("pf");
        std::fs::create_dir_all(pf.join("Steam").join("steamapps").join("shadercache"))
            .expect("create steam fixture");
        let with_pf = tool_roots_from(&up, &local, Some(&pf), &ToolCacheEnv::none());
        assert!(
            with_pf.iter().any(|(_, p, l, m)| *l == "steam-shadercache"
                && *m == RuleMode::Delete
                && p.ends_with(Path::new("Steam").join("steamapps").join("shadercache"))),
            "steam-shadercache must be an explicit Delete rule"
        );
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
        let yarn_cache = drive_d.join("yarn-cache");
        let bun_cache = drive_d.join("bun-cache");
        let nuget_http = drive_d.join("nuget-http-cache");
        for p in [
            &cargo_home,
            &gopath,
            &gradle_home,
            &uv_cache,
            &pnpm_home,
            &yarn_cache,
            &drive_d.join("yarn-global"),
            &bun_cache,
            &nuget_http,
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
            nuget_http_cache: Some(nuget_http.clone()),
            gradle_user_home: Some(gradle_home.clone()),
            uv_cache_dir: Some(uv_cache.clone()),
            pnpm_home: Some(pnpm_home.clone()),
            npm_cache: Some(drive_d.join("npm-cache")),
            xdg_cache_home: Some(drive_d.join("xdg-cache")),
            yarn_cache_folder: Some(yarn_cache.clone()),
            yarn_global_folder: Some(drive_d.join("yarn-global")),
            bun_install_cache: Some(bun_cache.clone()),
        };
        let roots = tool_roots_from(&up, &local, None, &env);
        let find = |label: &str| {
            roots
                .iter()
                .find(|(_, _, l, _)| *l == label)
                .map(|(_, p, _, _)| p.clone())
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
            roots
                .iter()
                .filter(|(_, _, l, _)| *l == "pnpm-store")
                .count(),
            1,
            "PNPM_HOME must replace the default variants, not add to them"
        );
        // Yarn 1 takes YARN_CACHE_FOLDER; Berry follows YARN_GLOBAL_FOLDER
        // (which YARN_CACHE_FOLDER neither feeds nor displaces). The two rules
        // must never fold onto one path — that used to produce a duplicate row
        // whose second delete was always "path missing".
        assert_eq!(find("yarn-cache"), Some(yarn_cache.clone()));
        let berry_default = drive_d.join("yarn-global").join("berry").join("cache");
        assert_eq!(find("yarn-berry-cache"), Some(berry_default));
        assert_ne!(
            find("yarn-berry-cache"),
            find("yarn-cache"),
            "the two Yarn caches must never be the same directory"
        );
        // Bun and NuGet's HTTP cache each name their own directory.
        assert_eq!(find("bun-cache"), Some(bun_cache));
        assert_eq!(find("nuget-cache"), Some(nuget_http));
    }

    /// A shared cache root follows XDG_CACHE_HOME when the tools that honour it
    /// have been pointed elsewhere.
    #[test]
    fn shared_cache_root_follows_xdg_cache_home() {
        // Only the resolver is exercised here; a real `~/.cache` on the test
        // machine would make an end-to-end assertion machine-dependent.
        let env = ToolCacheEnv {
            xdg_cache_home: Some(PathBuf::from(r"D:\cache-home")),
            ..ToolCacheEnv::none()
        };
        // The profile is irrelevant here: XDG_CACHE_HOME must replace the
        // `~/.cache` default whatever the home directory is.
        let root = shared_cache_roots_from(Path::new(r"C:\Users\x"), &env);
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
        let mut gaps = ScanGaps::default();
        scan_shared_cache_children(&cache, false, &mut out, &mut scanned, &mut gaps);

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
        let mut gaps = ScanGaps::default();
        scan_shared_cache_children(&jb, true, &mut out, &mut scanned, &mut gaps);

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
        let mut gaps = ScanGaps::default();
        scan_shared_cache_children(&root, false, &mut out, &mut scanned, &mut gaps);

        assert_eq!(out.len(), TOOL_CACHE_CHILD_CAP);
        assert_eq!(scanned.len(), TOOL_CACHE_CHILD_CAP);
        // The children past the per-root cap are counted as omitted, so the UI
        // can say "N not shown" instead of implying the list is exhaustive.
        assert_eq!(gaps.omitted, 13);
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
    }

    /// CQ-8: `omitted` is measured in rows. Three dropped JetBrains homes, each
    /// expanding into two cache rows, hide six rows — not three children.
    #[test]
    fn dropped_jetbrains_children_are_counted_as_expanded_rows() {
        let root = unique_temp("ordered-rows");
        let dropped = 3;
        for i in 0..(TOOL_CACHE_CHILD_CAP + dropped) {
            let home = root.join(format!("IDE-{i:03}"));
            std::fs::create_dir_all(home.join("caches")).expect("create caches");
            std::fs::create_dir_all(home.join("tmp")).expect("create tmp");
        }

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_shared_cache_children(&root, true, &mut out, &mut scanned, &mut gaps);

        assert_eq!(gaps.omitted, (dropped * 2) as u32);
    }

    /// Forks are recognised by the cache layout, not by name, so an editor this
    /// project has never heard of is still covered.
    #[test]
    fn vscode_family_caches_are_found_by_layout_not_name() {
        let root = unique_temp("editors");
        let roaming = root.join("AppData").join("Roaming");
        for editor in ["Code", "Code - Insiders", "VSCodium", "Cursor", "Positron"] {
            let home = roaming.join(editor);
            std::fs::create_dir_all(home.join("Cache")).expect("create Cache");
            std::fs::create_dir_all(home.join("CachedData")).expect("create CachedData");
            // Settings and extensions must not be offered.
            std::fs::create_dir_all(home.join("User").join("settings")).expect("create settings");
        }
        // An app with only one of the two is not an editor.
        let partial = roaming.join("SomeApp");
        std::fs::create_dir_all(partial.join("Cache")).expect("create partial");

        let mut degraded = Vec::new();
        let roots = vscode_family_caches(&roaming, &mut degraded);
        assert_eq!(roots.len(), 10, "two cache dirs per editor");
        for editor in ["Code", "Code - Insiders", "VSCodium", "Cursor", "Positron"] {
            for cache in ["Cache", "CachedData"] {
                let want = roaming.join(editor).join(cache);
                assert!(
                    roots.iter().any(|(_, p, _, _)| *p == want),
                    "missing {editor}\\{cache}"
                );
            }
        }
        assert!(roots.iter().all(|(_, p, _, _)| !p.ends_with("settings")));
    }

    /// Every profile of every known Chromium browser, and nothing else that
    /// lives in User Data.
    #[test]
    fn chromium_caches_cover_products_and_profiles_only() {
        let root = unique_temp("chromium");
        let local = root.join("AppData").join("Local");
        let user_data = local.join("Google").join("Chrome").join("User Data");
        for profile in ["Default", "Profile 1", "Profile 12"] {
            for cache in CHROMIUM_PROFILE_CACHES {
                std::fs::create_dir_all(user_data.join(profile).join(cache))
                    .expect("create profile cache");
            }
        }
        // Not a profile: settings, extensions and local state.
        for other in ["Local State", "Extensions", "Graphite Perceptual Hash"] {
            std::fs::create_dir_all(user_data.join(other).join("Cache"))
                .expect("create non-profile");
        }
        // A browser from the list, absent on this fixture.
        std::fs::create_dir_all(local.join("Vivaldi").join("User Data")).expect("create vivaldi");

        let mut degraded = Vec::new();
        let roots = chromium_profile_caches(&local, &mut degraded);
        assert_eq!(roots.len(), 9, "three caches per profile");
        for profile in ["Default", "Profile 1", "Profile 12"] {
            for cache in CHROMIUM_PROFILE_CACHES {
                let want = user_data.join(profile).join(cache);
                assert!(
                    roots.iter().any(|(_, p, _, _)| *p == want),
                    "missing {profile}\\{cache}"
                );
            }
        }
        for other in ["Local State", "Extensions"] {
            assert!(
                !roots
                    .iter()
                    .any(|(_, p, _, _)| p.ends_with(Path::new(other).join("Cache"))),
                "{other} is not a profile"
            );
        }
    }

    /// The JetBrains subdirectory allow-list is what makes a child preselected,
    /// so it must not leak to any other shared root. `~/.cache/<tool>` holding a
    /// `caches` or `tmp` subdirectory is a real shape and must stay unselected.
    #[test]
    fn jetbrains_allow_list_does_not_leak_to_other_shared_roots() {
        let root = unique_temp("leak");
        let shared = root.join(".cache");
        let tool = shared.join("some-tool");
        std::fs::create_dir_all(tool.join("caches")).expect("create caches");
        std::fs::create_dir_all(tool.join("tmp")).expect("create tmp");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        // jetbrains_home = false: this is the XDG-style shared root.
        scan_shared_cache_children(&shared, false, &mut out, &mut scanned, &mut gaps);

        assert_eq!(out.len(), 1, "the tool dir is reported whole, not expanded");
        let it = &out[0];
        assert!(it.path.ends_with("some-tool"), "{}", it.path);
        assert_eq!(it.confidence, Confidence::Suspected);
        assert_eq!(it.risk, RiskLevel::Medium);

        // The same layout under the JetBrains root still promotes, proving the
        // flag is what gates it and not the directory shape.
        let jb = root.join("JetBrains");
        let ide = jb.join("IdeaIC2025.2");
        std::fs::create_dir_all(ide.join("caches")).expect("create jb caches");
        let mut jb_out = Vec::new();
        let mut jb_scanned = HashSet::new();
        let mut jb_gaps = ScanGaps::default();
        scan_shared_cache_children(&jb, true, &mut jb_out, &mut jb_scanned, &mut jb_gaps);
        assert_eq!(jb_out.len(), 1);
        assert_eq!(jb_out[0].confidence, Confidence::Confirmed);
        assert_eq!(jb_out[0].risk, RiskLevel::Low);
    }

    /// A relocated root becomes a delete target, so an implausible value must be
    /// dropped rather than trusted: `XDG_CACHE_HOME=D:\` would otherwise offer
    /// every top-level directory on the drive.
    #[test]
    fn implausible_relocation_roots_are_rejected() {
        for bad in [
            "D:",
            "D:/",
            "C:",
            "relative/cache",
            "",
            "C:/Users/x/Documents",
        ] {
            assert!(
                !is_plausible_cache_root(Path::new(bad)),
                "{bad} must not be accepted as a cache root"
            );
        }
        for good in [
            "D:/cache-home",
            "C:/Users/x/AppData/Local/uv/cache",
            "D:/tools/cargo",
            "C:/Users/x/.gradle",
        ] {
            assert!(
                is_plausible_cache_root(Path::new(good)),
                "{good} must be accepted as a cache root"
            );
        }
    }

    /// The whole scan is bounded, not just the fixed-root phase, and the
    /// allow-list only ever names paths that were actually reported.
    #[test]
    fn scan_never_exceeds_the_documented_result_cap() {
        let _allow = crate::scan_allow::test_lock();
        let result = scan_tool_caches().expect("tool-cache scan");
        assert!(
            result.items.len() <= SPECIALTY_RESULT_CAP,
            "reported {} items, cap is {}",
            result.items.len(),
            SPECIALTY_RESULT_CAP
        );
        for it in &result.items {
            assert!(crate::scan_allow::was_recent(
                AllowScope::ToolCache,
                &it.path
            ));
        }
    }

    #[test]
    fn scan_runs_under_cap() {
        let _allow = crate::scan_allow::test_lock();
        let result = scan_tool_caches().expect("tool-cache scan");
        assert!(result.items.len() <= SPECIALTY_RESULT_CAP);
        for it in &result.items {
            assert!(!it.path.is_empty());
            // User-data red lines are never default-selectable, whatever the score.
            if it.user_data || it.user_library {
                assert_eq!(it.confidence, Confidence::Suspected);
            }
        }
    }

    /// A shared root that exists but cannot be read must say so. An empty item
    /// list with no `degraded` entry reads as "this machine has no tool caches
    /// here", which is the opposite of the truth for a permission-restricted
    /// user — the exact failure the scoped panel cannot otherwise distinguish.
    #[test]
    fn unreadable_shared_root_is_reported_as_degraded() {
        let root = unique_temp("unreadable");
        // A file where a directory is expected: `read_dir` fails with something
        // other than not-found, which is the shape of a permission refusal.
        let blocked = root.join("blocked-cache");
        std::fs::write(&blocked, b"not a directory").expect("create blocked root");
        assert!(std::fs::metadata(&blocked).is_ok());

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_shared_cache_children(&blocked, false, &mut out, &mut scanned, &mut gaps);

        assert!(out.is_empty(), "an unreadable root yields no items");
        assert_eq!(gaps.omitted, 0);
        assert_eq!(
            gaps.degraded.len(),
            1,
            "an unreadable root must be reported, got {:?}",
            gaps.degraded
        );
        assert!(
            gaps.degraded[0].starts_with("unreadable_root:"),
            "{}",
            gaps.degraded[0]
        );
        assert!(
            gaps.degraded[0].to_lowercase().contains("blocked-cache"),
            "the reason names the root: {}",
            gaps.degraded[0]
        );
    }
}
