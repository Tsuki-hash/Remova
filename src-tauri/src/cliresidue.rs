//! CLI residue diagnostics (M4E.4, read-only) — the evidence around global CLI
//! packages: prefix roots, shim files, empty scope dirs, PATH entries and the
//! user's `.npmrc` keys that mention them.
//!
//! Credential safety: `.npmrc` values that look like tokens or carry URL
//! credentials are never included — only the key name and a redacted flag.
//! PATH is read from the persisted Machine/User values, never the current
//! process snapshot.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliResidueEntry {
    /// `npm` | `pnpm` | `yarn`
    pub manager: String,
    /// `prefix_root` | `shim` | `shim_orphan` | `empty_scope_dir` |
    /// `path_entry` | `npmrc_key`
    pub kind: String,
    /// Name: shim stem, scope name, or the .npmrc key.
    pub name: String,
    /// Safe detail (a path or host); never a credential value.
    pub detail: String,
    /// True when the value would carry a credential and was redacted.
    pub redacted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CliResidueReport {
    pub entries: Vec<CliResidueEntry>,
    #[serde(default)]
    pub completeness: crate::scan_contract::ScanCompleteness,
}

const SECRET_KEY_MARKERS: &[&str] = &[
    "token",
    "authtoken",
    "_auth",
    "password",
    "passwd",
    "secret",
    "certfile",
    "keyfile",
];

/// True when a `.npmrc` key names a credential-bearing setting.
fn is_secret_key(key_lower: &str) -> bool {
    SECRET_KEY_MARKERS.iter().any(|m| key_lower.contains(m))
}

/// Strip credentials from a registry URL: keep only scheme + host, dropping
/// userinfo (RFC 3986: authority ends at the **last** `@`), path and query —
/// the query string can carry `?token=` style secrets.
pub fn redact_registry_url(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(scheme_end) = trimmed.find("://") else {
        // Schemeless registry values: keep everything after the last `@`
        // (userinfo before host), credentials included in the dropped part.
        return trimmed
            .rsplit_once('@')
            .map(|(_, host)| host.to_string())
            .unwrap_or_else(|| trimmed.to_string());
    };
    let (scheme, rest) = trimmed.split_at(scheme_end + 3);
    // Authority ends at the first `/` or `?` — never at an `@` inside the
    // path. Userinfo, if present, terminates at the **last** `@` before that.
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    };
    format!("{scheme}{host}")
}

/// True when an expanded `.npmrc` value still looks credential-bearing after
/// environment expansion (`${VAR}` / `%VAR%` may resolve to a secret). A URL
/// with `@` may carry userinfo credentials.
fn value_looks_secret(expanded: &str) -> bool {
    let lower = expanded.to_lowercase();
    SECRET_KEY_MARKERS.iter().any(|m| lower.contains(m))
        || lower.contains("://") && expanded.contains('@')
}

/// Extract the exe path a shim/`Applications`-style command string points at:
/// the first quoted token, else the first whitespace-delimited token.
pub fn command_exe_path(command: &str) -> Option<String> {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        let end = rest.find('"')?;
        let path = rest[..end].trim();
        return if path.is_empty() {
            None
        } else {
            Some(path.into())
        };
    }
    let token = command.split_whitespace().next()?;
    if token.len() < 4 {
        return None;
    }
    Some(token.into())
}

/// Command names the global tree's package.json `bin` fields can prove:
/// package dir names plus explicit bin map keys. A shim whose stem appears
/// here is alive even when its package is scoped or renamed.
fn package_bin_names(prefix: &Path) -> Option<HashSet<String>> {
    let nm = prefix.join("node_modules");
    let read = std::fs::read_dir(&nm).ok()?;
    let mut out = HashSet::new();
    let bin_of = |dir: &Path| -> Vec<String> {
        let bins = (|| -> Option<Vec<String>> {
            let raw = std::fs::read_to_string(dir.join("package.json")).ok()?;
            let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
            match v.get("bin")? {
                serde_json::Value::String(_) => {
                    // A string bin uses the package name as the command.
                    let name = v.get("name")?.as_str()?.to_string();
                    Some(vec![name.rsplit('/').next().unwrap_or(&name).to_string()])
                }
                serde_json::Value::Object(map) => Some(map.keys().cloned().collect()),
                _ => Some(vec![]),
            }
        })()
        .unwrap_or_default();
        bins
    };
    for entry in read.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if name.starts_with('@') {
            if let Ok(scope) = std::fs::read_dir(entry.path()) {
                for scoped in scope.flatten() {
                    if scoped.file_type().is_ok_and(|t| t.is_dir()) {
                        out.extend(bin_of(&scoped.path()));
                    }
                }
            }
        } else {
            out.extend(bin_of(&entry.path()));
            out.insert(name);
        }
    }
    Some(out)
}

/// True when a shim stem matches no bin evidence in the global tree. An
/// unreadable tree never claims orphan (fail closed).
fn shim_target_missing(prefix: &Path, stem: &str) -> bool {
    package_bin_names(prefix)
        .map(|bins| !bins.contains(stem))
        .unwrap_or(false)
}

fn push(out: &mut Vec<CliResidueEntry>, entry: CliResidueEntry) {
    out.push(entry);
}

/// Read the persisted Machine/User PATH and report entries that match a
/// manager root. Bounded reads via `regscan::read_string`.
fn path_entries_for(out: &mut Vec<CliResidueEntry>, manager: &str, root_lower: &str) {
    for (alias, key) in [
        (
            "Machine",
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
        ("User", r"HKCU\Environment"),
    ] {
        let Some(path_value) = crate::regscan::read_string(key, "Path") else {
            continue;
        };
        for segment in path_value.split(';') {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }
            let expanded = crate::regops::expand_env_string(segment);
            if expanded
                .to_lowercase()
                .replace('/', "\\")
                .starts_with(root_lower)
            {
                push(
                    out,
                    CliResidueEntry {
                        manager: manager.into(),
                        kind: "path_entry".into(),
                        name: alias.to_string(),
                        detail: expanded.clone(),
                        redacted: false,
                    },
                );
            }
        }
    }
}

/// Collect residue entries over the same injected roots the inventory uses.
pub fn scan_residue_with(roots: &crate::cliglobals::CliRoots) -> CliResidueReport {
    let mut entries = Vec::new();
    let mut roots_scanned = 0u32;
    let mut failed_roots = Vec::new();
    if crate::scan_task::cancelled() {
        return CliResidueReport {
            completeness: crate::scan_contract::ScanCompleteness::cancelled(0),
            entries,
        };
    }

    // npm: prefix root, shims, empty scope dirs, PATH, npmrc keys.
    if let Some(npm) = roots.npm_prefix.as_deref() {
        if npm.is_dir() {
            roots_scanned += 1;
            push(
                &mut entries,
                CliResidueEntry {
                    manager: "npm".into(),
                    kind: "prefix_root".into(),
                    name: "npm".into(),
                    detail: npm.to_string_lossy().into_owned(),
                    redacted: false,
                },
            );
            let nm = npm.join("node_modules");
            if let Ok(read) = std::fs::read_dir(npm) {
                for entry in read.flatten() {
                    // Cancel lands between packages: large global trees must
                    // not outlive an abandoned scan.
                    if crate::scan_task::cancelled() {
                        return CliResidueReport {
                            entries,
                            completeness: crate::scan_contract::ScanCompleteness::cancelled(
                                roots_scanned,
                            ),
                        };
                    }
                    if entry.file_type().is_ok_and(|t| t.is_dir()) {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if !name.starts_with('.')
                            && !name.starts_with('@')
                            && name != "node_modules"
                        {
                            let manifest = entry.path().join("package.json");
                            if !manifest.is_file() {
                                push(
                                    &mut entries,
                                    CliResidueEntry {
                                        manager: "npm".into(),
                                        kind: "pkg_no_manifest".into(),
                                        name,
                                        detail: entry.path().to_string_lossy().into_owned(),
                                        redacted: false,
                                    },
                                );
                            }
                        }
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let lower = name.to_lowercase();
                    if lower.ends_with(".cmd") || lower.ends_with(".ps1") {
                        let stem = name
                            .trim_end_matches(".cmd")
                            .trim_end_matches(".ps1")
                            .to_string();
                        let kind = if shim_target_missing(npm, &stem) {
                            "shim_orphan"
                        } else {
                            "shim"
                        };
                        push(
                            &mut entries,
                            CliResidueEntry {
                                manager: "npm".into(),
                                kind: kind.into(),
                                name: stem,
                                detail: entry.path().to_string_lossy().into_owned(),
                                redacted: false,
                            },
                        );
                    }
                }
            }
            if let Ok(read) = std::fs::read_dir(&nm) {
                for entry in read.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !name.starts_with('@') {
                        continue;
                    }
                    let Ok(scope_read) = std::fs::read_dir(entry.path()) else {
                        continue;
                    };
                    for scoped in scope_read.flatten() {
                        let scoped_name = scoped.file_name().to_string_lossy().into_owned();
                        if scoped_name.starts_with('.') {
                            continue;
                        }
                        // A package directory without package.json is not a
                        // working package: leftover or a broken install.
                        let manifest = scoped.path().join("package.json");
                        if !manifest.is_file() {
                            push(
                                &mut entries,
                                CliResidueEntry {
                                    manager: "npm".into(),
                                    kind: "pkg_no_manifest".into(),
                                    name: format!("{name}/{scoped_name}"),
                                    detail: scoped.path().to_string_lossy().into_owned(),
                                    redacted: false,
                                },
                            );
                        }
                    }
                    let empty = std::fs::read_dir(entry.path())
                        .map(|mut d| d.next().is_none())
                        .unwrap_or(false);
                    if empty {
                        push(
                            &mut entries,
                            CliResidueEntry {
                                manager: "npm".into(),
                                kind: "empty_scope_dir".into(),
                                name,
                                detail: entry.path().to_string_lossy().into_owned(),
                                redacted: false,
                            },
                        );
                    }
                }
            }
            let root_lower = npm.to_string_lossy().to_lowercase().replace('/', "\\");
            path_entries_for(&mut entries, "npm", root_lower.trim_end_matches('\\'));
        } else {
            failed_roots.push("npm".into());
        }
    }

    // pnpm/yarn roots: prefix-only visibility (shims already in the inventory).
    for (manager, root) in [
        ("pnpm", roots.pnpm_home.as_deref()),
        ("yarn", roots.yarn_global.as_deref()),
    ] {
        if let Some(root) = root {
            if root.is_dir() {
                roots_scanned += 1;
                push(
                    &mut entries,
                    CliResidueEntry {
                        manager: manager.into(),
                        kind: "prefix_root".into(),
                        name: manager.into(),
                        detail: root.to_string_lossy().into_owned(),
                        redacted: false,
                    },
                );
            } else {
                failed_roots.push(manager.into());
            }
        }
    }

    // .npmrc keys — key names only; secret values never leave this module.
    if let Some(npmrc) = std::env::var_os("USERPROFILE").map(|home| Path::new(&home).join(".npmrc"))
    {
        if let Ok(raw) = std::fs::read_to_string(&npmrc) {
            for line in raw.lines() {
                let line = line.trim().trim_start_matches('\u{feff}');
                if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let key = key.trim();
                let key_lower = key.to_lowercase();
                let interesting = ["prefix", "registry", "global-bin-dir", "store-dir"]
                    .iter()
                    .any(|k| key_lower == *k || key_lower.ends_with(k))
                    || is_secret_key(&key_lower);
                if !interesting {
                    continue;
                }
                let redacted = is_secret_key(&key_lower);
                let expanded = crate::cliglobals::expand_env_vars_public(value);
                // Environment expansion can resolve a variable to a value
                // carrying a credential: re-run the heuristic on the result
                // before it leaves the module.
                let redacted = redacted || value_looks_secret(&expanded);
                let detail = if redacted {
                    "<redacted>".into()
                } else if key_lower.contains("registry") {
                    redact_registry_url(&expanded)
                } else {
                    expanded
                };
                push(
                    &mut entries,
                    CliResidueEntry {
                        manager: "npm".into(),
                        kind: "npmrc_key".into(),
                        name: key.to_string(),
                        detail,
                        redacted,
                    },
                );
            }
        }
    }

    let completeness = if failed_roots.is_empty() {
        crate::scan_contract::ScanCompleteness {
            status: crate::scan_contract::ScanStatus::Complete,
            roots_scanned,
            failed_roots,
        }
    } else {
        crate::scan_contract::ScanCompleteness::partial(roots_scanned, failed_roots)
    };
    CliResidueReport {
        entries,
        completeness,
    }
}

pub fn scan_cli_residue() -> CliResidueReport {
    scan_residue_with(&crate::cliglobals::roots_from_env())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_keys_are_redacted_and_registry_urls_keep_the_host() {
        assert!(is_secret_key("authtoken"));
        assert!(is_secret_key("//registry.npmjs.org/:_authToken"));
        // Path and query are stripped too: a registry URL can carry
        // `?token=` style secrets in its query.
        assert_eq!(
            redact_registry_url("https://user:pass@registry.example.com/path?token=x"),
            "https://registry.example.com"
        );
        assert_eq!(
            redact_registry_url("registry.npmjs.org"),
            "registry.npmjs.org"
        );
        // Schemeless values with userinfo keep only the host.
        assert_eq!(
            redact_registry_url("user:pass@registry.example.com"),
            "registry.example.com"
        );
    }

    #[test]
    fn command_exe_path_handles_quoted_and_bare_tokens() {
        assert_eq!(
            command_exe_path(r#""C:\Tools\gone app.exe" "%*""#),
            Some(r"C:\Tools\gone app.exe".into())
        );
        assert_eq!(
            command_exe_path(r"C:\Tools\gone.exe --flag"),
            Some(r"C:\Tools\gone.exe".into())
        );
        assert_eq!(command_exe_path(""), None);
    }

    #[test]
    fn shim_targets_missing_requires_bin_evidence() {
        let tmp = crate::test_support::TempDir::new("cli-residue");
        let prefix = tmp.path().join("npm");
        let present = prefix.join("node_modules").join("present");
        std::fs::create_dir_all(&present).unwrap();
        std::fs::write(
            present.join("package.json"),
            r#"{"name":"present","bin":{"present":"cli.js"}}"#,
        )
        .unwrap();
        // Scoped package whose bin names a different command.
        let scoped = prefix.join("node_modules").join("@scope").join("tool");
        std::fs::create_dir_all(&scoped).unwrap();
        std::fs::write(
            scoped.join("package.json"),
            r#"{"name":"@scope/tool","bin":{"tool-cli":"cli.js"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(prefix.join("node_modules").join("@scope")).unwrap();
        assert!(!shim_target_missing(&prefix, "present"));
        assert!(
            !shim_target_missing(&prefix, "tool-cli"),
            "scoped bin evidence counts"
        );
        assert!(shim_target_missing(&prefix, "absent"));
        assert!(
            shim_target_missing(&prefix, "some-cli"),
            "no bin evidence anywhere"
        );
        // An unreadable tree never claims orphan.
        let missing = tmp.path().join("nope");
        assert!(!shim_target_missing(&missing, "anything"));
    }
}
