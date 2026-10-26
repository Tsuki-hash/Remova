//! Global CLI package inventory (npm / pnpm / yarn) — read-only (M4).
//!
//! Lists what is provably there: npm and yarn-classic global `node_modules`
//! trees (package name from the directory, version from its package.json),
//! and pnpm global command shims (the store layout is link-based and private
//! to pnpm, so shims are the honest inventory). Deletion gates come later,
//! one manager at a time; this module never removes anything.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ITEM_CAP: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliGlobalItem {
    /// "npm" | "pnpm" | "yarn"
    pub manager: String,
    /// Package name (`@scope/name` for scopes) or shim stem for pnpm.
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    pub path: String,
    #[serde(default)]
    pub size_kb: i64,
    #[serde(default)]
    pub size_capped: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliGlobalScanResult {
    pub items: Vec<CliGlobalItem>,
    #[serde(default)]
    pub omitted: u32,
    /// Managers whose root was not found or not readable — reported, never silent.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Injectable roots so the layout walk is testable against fixtures.
#[derive(Debug, Clone, Default)]
pub struct CliRoots {
    pub npm_prefix: Option<PathBuf>,
    pub pnpm_home: Option<PathBuf>,
    pub yarn_global: Option<PathBuf>,
}

pub fn roots_from_env() -> CliRoots {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    // Every root is resolved per machine: env override → the user's npmrc →
    // the manager's documented default. No absolute path is baked in.
    let npmrc = npmrc_path();
    let npm_prefix = npm_prefix_from(
        var("npm_config_prefix"),
        npmrc.as_deref().and_then(|p| npmrc_value_at(p, "prefix")),
        var("APPDATA"),
    );
    let pnpm_home = var("PNPM_HOME")
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            npmrc
                .as_deref()
                .and_then(|p| npmrc_value_at(p, "global-bin-dir"))
        })
        .or_else(|| var("LOCALAPPDATA").map(|local| local.join("pnpm")));
    let yarn_global = var("YARN_GLOBAL_FOLDER")
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(yarnrc_global_folder)
        .or_else(|| var("LOCALAPPDATA").map(|local| local.join(r"Yarn\Data\global")));
    CliRoots {
        npm_prefix,
        pnpm_home,
        yarn_global,
    }
}

/// npm's documented Windows default when neither env nor npmrc overrides it.
fn npm_prefix_from(
    env_prefix: Option<PathBuf>,
    npmrc_prefix: Option<PathBuf>,
    appdata: Option<PathBuf>,
) -> Option<PathBuf> {
    env_prefix
        .filter(|p| !p.as_os_str().is_empty())
        .or(npmrc_prefix)
        .or_else(|| appdata.map(|appdata| appdata.join("npm")))
}

fn npmrc_path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join(".npmrc"))
}

/// Yarn classic stores its global folder in `~/.yarnrc` as `global-folder "…"`.
fn yarnrc_global_folder() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    yarnrc_global_folder_at(&home.join(".yarnrc"))
}

fn yarnrc_global_folder_at(path: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut found: Option<PathBuf> = None;
    for line in raw.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if key != "global-folder" {
            continue;
        }
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        if value.is_empty() {
            continue;
        }
        found = Some(PathBuf::from(expand_env_vars(value)));
    }
    found
}

/// Read one key from the user's `~/.npmrc`, with `%VAR%` / `${VAR}` expansion.
/// npm applies ini last-wins semantics, so a repeated key resolves to its last
/// value; a valueless line is skipped rather than ending the search.
fn npmrc_value_at(path: &Path, key: &str) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut found: Option<PathBuf> = None;
    for line in raw.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((candidate, value)) = line.split_once('=') else {
            continue;
        };
        if !candidate.trim().eq_ignore_ascii_case(key) {
            continue;
        }
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        if value.is_empty() {
            continue;
        }
        found = Some(PathBuf::from(expand_env_vars(value)));
    }
    found
}

/// npmrc values may use either Windows (`%APPDATA%`) or npm (`${HOME}`)
/// variable syntax. Unknown variables stay literal.
fn expand_env_vars(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i..].starts_with("${") {
            if let Some(end) = raw[i + 2..].find('}') {
                let name = &raw[i + 2..i + 2 + end];
                if let Some(v) = std::env::var_os(name) {
                    out.push_str(&v.to_string_lossy());
                    i += 2 + end + 1;
                    continue;
                }
            }
        } else if raw[i..].starts_with('%') {
            if let Some(end) = raw[i + 1..].find('%') {
                let name = &raw[i + 1..i + 1 + end];
                if let Some(v) = std::env::var_os(name) {
                    out.push_str(&v.to_string_lossy());
                    i += 1 + end + 1;
                    continue;
                }
            }
        }
        let ch = raw[i..].chars().next().expect("char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Read `"version"` out of a package.json without failing the listing.
fn package_version(pkg_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(pkg_dir.join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("version")?.as_str().map(str::to_string)
}

fn push_pkg(
    out: &mut Vec<CliGlobalItem>,
    omitted: &mut u32,
    manager: &str,
    name: String,
    dir: &Path,
) {
    if out.len() >= ITEM_CAP {
        *omitted += 1;
        return;
    }
    let (size_kb, size_capped) = crate::dirsize::walk_size_kb_capped(dir);
    out.push(CliGlobalItem {
        manager: manager.into(),
        name,
        version: package_version(dir),
        path: dir.to_string_lossy().into_owned(),
        size_kb,
        size_capped,
    });
}

/// Enumerate one npm/yarn-style global `node_modules` tree. `Ok(found)` keeps
/// "readable but empty" distinct from a read failure.
fn scan_node_modules_tree(
    root: &Path,
    manager: &str,
    out: &mut Vec<CliGlobalItem>,
    omitted: &mut u32,
) -> Result<bool, ()> {
    let node_modules = root.join("node_modules");
    if !node_modules.exists() {
        return Ok(false);
    }
    let read = std::fs::read_dir(&node_modules).map_err(|_| ())?;
    let mut found = false;
    for entry in read.flatten() {
        let path = entry.path();
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            // .bin and dotfiles are tooling internals, not packages.
            continue;
        }
        found = true;
        if name.starts_with('@') {
            let Ok(scope_read) = std::fs::read_dir(&path) else {
                continue;
            };
            for scoped in scope_read.flatten() {
                let sp = scoped.path();
                if !scoped.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let scoped_name = scoped.file_name().to_string_lossy().into_owned();
                if scoped_name.starts_with('.') {
                    continue;
                }
                push_pkg(out, omitted, manager, format!("{name}/{scoped_name}"), &sp);
            }
        } else {
            push_pkg(out, omitted, manager, name, &path);
        }
    }
    Ok(found)
}

/// pnpm's global store is link-based; the shims in PNPM_HOME are the provable
/// inventory. Sizes/versions stay unknown rather than guessed.
fn scan_pnpm_shims(
    home: &Path,
    out: &mut Vec<CliGlobalItem>,
    omitted: &mut u32,
) -> Result<bool, ()> {
    let read = std::fs::read_dir(home).map_err(|_| ())?;
    let mut found = false;
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".cmd") || name.ends_with(".ps1") {
            // One command may carry both shims; the .cmd is the canonical row.
            if name.ends_with(".cmd") {
                found = true;
                if out.len() >= ITEM_CAP {
                    *omitted += 1;
                } else {
                    out.push(CliGlobalItem {
                        manager: "pnpm".into(),
                        name: name.trim_end_matches(".cmd").to_string(),
                        version: None,
                        path: entry.path().to_string_lossy().into_owned(),
                        size_kb: 0,
                        size_capped: false,
                    });
                }
            }
        }
    }
    Ok(found)
}

/// Core scan over injected roots.
pub fn scan_with(roots: &CliRoots) -> CliGlobalScanResult {
    let mut items = Vec::new();
    let mut omitted = 0u32;
    let mut notes = Vec::new();
    // A manager is reported once when its root is missing, unreadable, or the
    // scan fails — all three are "not scanned" with the same user-facing note.
    let npm_ok = roots.npm_prefix.as_deref().is_some_and(|npm| {
        npm.is_dir() && scan_node_modules_tree(npm, "npm", &mut items, &mut omitted).is_ok()
    });
    if !npm_ok {
        notes.push("npm".into());
    }
    let pnpm_ok = roots.pnpm_home.as_deref().is_some_and(|pnpm| {
        pnpm.is_dir() && scan_pnpm_shims(pnpm, &mut items, &mut omitted).is_ok()
    });
    if !pnpm_ok {
        notes.push("pnpm".into());
    }
    let yarn_ok = roots.yarn_global.as_deref().is_some_and(|yarn| {
        yarn.is_dir() && scan_node_modules_tree(yarn, "yarn", &mut items, &mut omitted).is_ok()
    });
    if !yarn_ok {
        notes.push("yarn".into());
    }
    items.sort_by_key(|i| (i.manager.clone(), i.name.to_lowercase()));
    CliGlobalScanResult {
        items,
        omitted,
        notes,
    }
}

pub fn scan_cli_globals() -> CliGlobalScanResult {
    scan_with(&roots_from_env())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(root: &Path, name: &str, version: &str) {
        let dir = root.join("node_modules").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            format!(r#"{{"name":"{name}","version":"{version}"}}"#),
        )
        .unwrap();
    }

    #[test]
    fn npm_tree_lists_packages_scopes_and_skips_dotdirs() {
        let tmp = crate::test_support::TempDir::new("cli-npm");
        let root = tmp.path().join("npm-prefix");
        pkg(&root, "typescript", "5.6.0");
        pkg(&root, "@scope/tool", "1.2.3");
        std::fs::create_dir_all(root.join(r"node_modules\.bin")).unwrap();
        std::fs::write(root.join(r"node_modules\.package-lock.json"), b"{}").unwrap();

        let out = scan_with(&CliRoots {
            npm_prefix: Some(root),
            pnpm_home: None,
            yarn_global: None,
        });
        let names: Vec<&str> = out.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["@scope/tool", "typescript"]);
        assert_eq!(out.items[0].version.as_deref(), Some("1.2.3"));
        assert!(out.items.iter().all(|i| i.manager == "npm"));
        assert!(out.items.iter().all(|i| i.size_kb >= 0));
        // pnpm/yarn roots were absent → both noted.
        assert_eq!(out.notes, ["pnpm", "yarn"]);
    }

    #[test]
    fn pnpm_lists_cmd_shims_without_guessing_versions() {
        let tmp = crate::test_support::TempDir::new("cli-pnpm");
        let home = tmp.path().join("pnpm");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("rimraf.cmd"), b"@echo off").unwrap();
        std::fs::write(home.join("rimraf.ps1"), b"exit 0").unwrap();
        std::fs::write(home.join("readme.txt"), b"x").unwrap();

        let out = scan_with(&CliRoots {
            npm_prefix: None,
            pnpm_home: Some(home),
            yarn_global: None,
        });
        assert_eq!(out.items.len(), 1, "one row per command, not per shim file");
        assert_eq!(out.items[0].name, "rimraf");
        assert_eq!(out.items[0].version, None);
        assert_eq!(out.notes, ["npm", "yarn"]);
    }

    #[test]
    fn unreadable_roots_report_the_manager_not_silence() {
        let out = scan_with(&CliRoots::default());
        assert!(out.items.is_empty());
        assert_eq!(out.notes, ["npm", "pnpm", "yarn"]);
    }

    #[test]
    fn readable_roots_without_packages_are_not_reported_as_missing() {
        let tmp = crate::test_support::TempDir::new("cli-empty");
        let npm = tmp.path().join("npm-prefix");
        let pnpm = tmp.path().join("pnpm");
        let yarn = tmp.path().join("yarn-global");
        for root in [&npm, &pnpm, &yarn] {
            std::fs::create_dir_all(root).unwrap();
        }
        let out = scan_with(&CliRoots {
            npm_prefix: Some(npm),
            pnpm_home: Some(pnpm),
            yarn_global: Some(yarn),
        });
        assert!(out.items.is_empty());
        assert!(
            out.notes.is_empty(),
            "empty but readable roots are not errors: {:?}",
            out.notes
        );
    }

    #[test]
    fn npmrc_prefix_reads_a_custom_global_root() {
        let tmp = crate::test_support::TempDir::new("cli-npmrc");
        let npmrc = tmp.path().join(".npmrc");
        std::fs::write(
            &npmrc,
            "cache=D:\\npm-cache\nPREFIX = \"D:\\nodejs-global\"\n",
        )
        .unwrap();
        assert_eq!(
            npmrc_value_at(&npmrc, "prefix"),
            Some(PathBuf::from(r"D:\nodejs-global"))
        );
    }

    #[test]
    fn npmrc_prefix_supports_the_common_comment_shapes() {
        let tmp = crate::test_support::TempDir::new("cli-npmrc-comments");
        let npmrc = tmp.path().join(".npmrc");
        std::fs::write(&npmrc, "# comment\n; another\nprefix='D:\\nodejs-global'\n").unwrap();
        assert_eq!(
            npmrc_value_at(&npmrc, "prefix"),
            Some(PathBuf::from(r"D:\nodejs-global"))
        );
    }

    /// npm applies ini last-wins semantics: `npm config get prefix` on a file
    /// with repeated keys answers the last value, so the resolver must too.
    #[test]
    fn npmrc_repeated_keys_resolve_to_the_last_value() {
        let tmp = crate::test_support::TempDir::new("cli-npmrc-dup");
        let npmrc = tmp.path().join(".npmrc");
        std::fs::write(
            &npmrc,
            "prefix=D:\\first\\one\nprefix=\nprefix=D:\\second\\two\n",
        )
        .unwrap();
        assert_eq!(
            npmrc_value_at(&npmrc, "prefix"),
            Some(PathBuf::from(r"D:\second\two"))
        );
    }

    #[test]
    fn npmrc_valueless_lines_are_skipped_not_terminal() {
        let tmp = crate::test_support::TempDir::new("cli-npmrc-empty");
        let npmrc = tmp.path().join(".npmrc");
        std::fs::write(&npmrc, "prefix=\nother=x\nprefix=\"D:\\real\"\n").unwrap();
        assert_eq!(
            npmrc_value_at(&npmrc, "prefix"),
            Some(PathBuf::from(r"D:\real"))
        );
        assert_eq!(npmrc_value_at(&npmrc, "missing"), None);
    }

    #[test]
    fn yarnrc_repeated_keys_resolve_to_the_last_value() {
        let tmp = crate::test_support::TempDir::new("cli-yarnrc-dup");
        let rc = tmp.path().join(".yarnrc");
        std::fs::write(
            &rc,
            "global-folder \"D:\\first\"\nglobal-folder \"D:\\second\"\n",
        )
        .unwrap();
        assert_eq!(
            yarnrc_global_folder_at(&rc),
            Some(PathBuf::from(r"D:\second"))
        );
    }

    #[test]
    fn npmrc_global_bin_dir_feeds_the_pnpm_root() {
        let tmp = crate::test_support::TempDir::new("cli-npmrc-pnpm");
        let npmrc = tmp.path().join(".npmrc");
        std::fs::write(&npmrc, "global-bin-dir=D:\\pnpm-bin\n").unwrap();
        assert_eq!(
            npmrc_value_at(&npmrc, "global-bin-dir"),
            Some(PathBuf::from(r"D:\pnpm-bin"))
        );
    }

    #[test]
    fn yarnrc_global_folder_is_honored() {
        let tmp = crate::test_support::TempDir::new("cli-yarnrc");
        let rc = tmp.path().join(".yarnrc");
        std::fs::write(
            &rc,
            "# yarn config\nnetwork-timeout 100000\nglobal-folder \"D:\\yarn-global\"\n",
        )
        .unwrap();
        assert_eq!(
            yarnrc_global_folder_at(&rc),
            Some(PathBuf::from(r"D:\yarn-global"))
        );
    }

    #[test]
    fn npm_prefix_resolves_per_machine_not_from_a_constant() {
        let env = PathBuf::from(r"C:\env-prefix");
        let npmrc = PathBuf::from(r"D:\custom-prefix");
        let appdata = PathBuf::from(r"C:\Users\someone\AppData\Roaming");
        // Env wins; then the user's npmrc; then npm's documented default.
        assert_eq!(
            npm_prefix_from(
                Some(env.clone()),
                Some(npmrc.clone()),
                Some(appdata.clone())
            ),
            Some(env)
        );
        assert_eq!(
            npm_prefix_from(None, Some(npmrc.clone()), Some(appdata.clone())),
            Some(npmrc)
        );
        assert_eq!(
            npm_prefix_from(None, None, Some(appdata.clone())),
            Some(appdata.join("npm"))
        );
        assert_eq!(npm_prefix_from(None, None, None), None);
        // An empty env value must not mask the npmrc/default fallback.
        assert_eq!(
            npm_prefix_from(
                Some(PathBuf::new()),
                Some(PathBuf::from(r"D:\rc-prefix")),
                None
            ),
            Some(PathBuf::from(r"D:\rc-prefix"))
        );
    }

    #[test]
    fn npmrc_prefix_keeps_unknown_env_tokens_literal() {
        assert_eq!(
            expand_env_vars(r"%REMOva_TEST_NOT_SET%\npm"),
            r"%REMOva_TEST_NOT_SET%\npm"
        );
        assert_eq!(
            expand_env_vars(r"${REMOva_TEST_NOT_SET}/npm"),
            r"${REMOva_TEST_NOT_SET}/npm"
        );
    }
}
