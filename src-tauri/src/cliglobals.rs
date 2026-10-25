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
    let npm_prefix =
        var("npm_config_prefix").or_else(|| var("APPDATA").map(|appdata| appdata.join("npm")));
    let pnpm_home =
        var("PNPM_HOME").or_else(|| var("LOCALAPPDATA").map(|local| local.join("pnpm")));
    let yarn_global = var("YARN_GLOBAL_FOLDER")
        .or_else(|| var("LOCALAPPDATA").map(|local| local.join(r"Yarn\Data\global")));
    CliRoots {
        npm_prefix,
        pnpm_home,
        yarn_global,
    }
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

/// Enumerate one npm/yarn-style global `node_modules` tree.
fn scan_node_modules_tree(
    root: &Path,
    manager: &str,
    out: &mut Vec<CliGlobalItem>,
    omitted: &mut u32,
) -> bool {
    let node_modules = root.join("node_modules");
    let Ok(read) = std::fs::read_dir(&node_modules) else {
        return false;
    };
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
    found
}

/// pnpm's global store is link-based; the shims in PNPM_HOME are the provable
/// inventory. Sizes/versions stay unknown rather than guessed.
fn scan_pnpm_shims(home: &Path, out: &mut Vec<CliGlobalItem>, omitted: &mut u32) -> bool {
    let Ok(read) = std::fs::read_dir(home) else {
        return false;
    };
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
    found
}

/// Core scan over injected roots.
pub fn scan_with(roots: &CliRoots) -> CliGlobalScanResult {
    let mut items = Vec::new();
    let mut omitted = 0u32;
    let mut notes = Vec::new();
    if let Some(npm) = &roots.npm_prefix {
        if !scan_node_modules_tree(npm, "npm", &mut items, &mut omitted) {
            notes.push("npm".into());
        }
    } else {
        notes.push("npm".into());
    }
    if let Some(pnpm) = &roots.pnpm_home {
        if !scan_pnpm_shims(pnpm, &mut items, &mut omitted) {
            notes.push("pnpm".into());
        }
    } else {
        notes.push("pnpm".into());
    }
    if let Some(yarn) = &roots.yarn_global {
        if !scan_node_modules_tree(yarn, "yarn", &mut items, &mut omitted) {
            notes.push("yarn".into());
        }
    } else {
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
}
