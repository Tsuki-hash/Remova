//! Conservative Windows npm global removal. Packages live in prefix/node_modules;
//! global shims live directly in prefix. Unknown shim formats fail closed.
//! Authorization, backup and deletion all retain the same immutable file handles.

use crate::fsutil::PinnedLocalDeleteTree;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpmPackageRef {
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpmCleanupReport {
    pub removed: Vec<String>,
    pub failed: Vec<String>,
    pub backup_dir: String,
}

fn safe_bin_stem(stem: &str) -> bool {
    if stem.is_empty()
        || stem.ends_with(['.', ' '])
        || stem.starts_with('.')
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return false;
    }
    let base = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        && !(base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

fn package_path(node_modules: &Path, name: &str) -> Result<PathBuf, String> {
    let parts: Vec<_> = name.split('/').collect();
    let valid = match parts.as_slice() {
        [pkg] => safe_bin_stem(pkg),
        [scope, pkg] => scope.strip_prefix('@').is_some_and(safe_bin_stem) && safe_bin_stem(pkg),
        _ => false,
    };
    if !valid {
        return Err(format!("invalid npm package name: {name}"));
    }
    Ok(parts
        .iter()
        .fold(node_modules.to_path_buf(), |p, part| p.join(part)))
}

fn bin_entries(raw: &str, name: &str) -> Result<Vec<(String, String)>, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid manifest: {e}"))?;
    if !v.is_object() {
        return Err("manifest must be an object".into());
    }
    let mut entries = match v.get("bin") {
        None => vec![],
        Some(serde_json::Value::String(target)) => {
            vec![(name.rsplit('/').next().unwrap().into(), target.clone())]
        }
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .map(|(stem, target)| {
                target
                    .as_str()
                    .map(|t| (stem.clone(), t.to_string()))
                    .ok_or("bin target must be a string".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("invalid manifest bin field".into()),
    };
    for (stem, target) in &mut entries {
        // npm normalizes a leading ./ before it generates the global shims.
        if let Some(relative) = target.strip_prefix("./") {
            *target = relative.to_string();
        }
        if !safe_bin_stem(stem) || target.is_empty() || target.split('/').any(|s| !safe_bin_stem(s))
        {
            return Err(format!("unsafe bin entry: {stem}"));
        }
    }
    Ok(entries)
}

/// Recognize complete cmd-shim templates, not a substring (which could be a
/// comment in another package's shim). Only the standard no-argument node and
/// direct-exec layouts are supported. Custom/legacy formats remain untouched
/// and the request reports an error, never a successful partial uninstall.
fn shim_templates(relative: &str, node: bool) -> [String; 3] {
    let win = relative.replace('/', "\\");
    let head = "@ECHO off\nGOTO start\n:find_dp0\nSET dp0=%~dp0\nEXIT /b\n:start\nSETLOCAL\nCALL :find_dp0\n";
    let cmd = if node {
        format!("{head}\nIF EXIST \"%dp0%\\node.exe\" (\n  SET \"_prog=%dp0%\\node.exe\"\n) ELSE (\n  SET \"_prog=node\"\n  SET PATHEXT=%PATHEXT:;.JS;=;%\n)\n\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\{win}\" %*\n")
    } else {
        format!("{head}\"%dp0%\\{win}\"   %*\n")
    };
    let sh_head = "#!/bin/sh\nbasedir=$(dirname \"$(echo \"$0\" | sed -e 's,\\\\,/,g')\")\n\ncase `uname` in\n    *CYGWIN*|*MINGW*|*MSYS*)\n        if command -v cygpath > /dev/null 2>&1; then\n            basedir=`cygpath -w \"$basedir\"`\n        fi\n    ;;\nesac\n\n";
    let sh = if node {
        format!("{sh_head}if [ -x \"$basedir/node\" ]; then\n  exec \"$basedir/node\"  \"$basedir/{relative}\" \"$@\"\nelse \n  exec node  \"$basedir/{relative}\" \"$@\"\nfi\n")
    } else {
        format!("{sh_head}exec \"$basedir/{relative}\"   \"$@\"\n")
    };
    let ps_head = "#!/usr/bin/env pwsh\n$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent\n\n$exe=\"\"\nif ($PSVersionTable.PSVersion -lt \"6.0\" -or $IsWindows) {\n  # Fix case when both the Windows and Linux builds of Node\n  # are installed in the same directory\n  $exe=\".exe\"\n}\n";
    let ps = if node {
        format!("{ps_head}$ret=0\nif (Test-Path \"$basedir/node$exe\") {{\n  # Support pipeline input\n  if ($MyInvocation.ExpectingInput) {{\n    $input | & \"$basedir/node$exe\"  \"$basedir/{relative}\" $args\n  }} else {{\n    & \"$basedir/node$exe\"  \"$basedir/{relative}\" $args\n  }}\n  $ret=$LASTEXITCODE\n}} else {{\n  # Support pipeline input\n  if ($MyInvocation.ExpectingInput) {{\n    $input | & \"node$exe\"  \"$basedir/{relative}\" $args\n  }} else {{\n    & \"node$exe\"  \"$basedir/{relative}\" $args\n  }}\n  $ret=$LASTEXITCODE\n}}\nexit $ret\n")
    } else {
        format!("{ps_head}# Support pipeline input\nif ($MyInvocation.ExpectingInput) {{\n  $input | & \"$basedir/{relative}\"   $args\n}} else {{\n  & \"$basedir/{relative}\"   $args\n}}\nexit $LASTEXITCODE\n")
    };
    [cmd, ps, sh]
}

struct PackageTargets {
    name: String,
    // Package last: if a shim delete fails, do not remove the package.
    files: Vec<PinnedLocalDeleteTree>,
}

fn collect_targets(
    prefix: &Path,
    node_modules: &Path,
    wanted: &[NpmPackageRef],
) -> Result<Vec<PackageTargets>, String> {
    if node_modules != prefix.join("node_modules") {
        return Err("not a Windows npm global layout".into());
    }
    let _prefix = crate::fsutil::pin_local_scan_path(prefix).map_err(|e| e.to_string())?;
    let mut seen_names = std::collections::HashSet::new();
    let mut seen_paths = std::collections::HashSet::new();
    let mut targets = vec![];
    for w in wanted {
        if !seen_names.insert(w.name.to_ascii_lowercase()) {
            return Err("duplicate package selection".into());
        }
        let dir = package_path(node_modules, &w.name)?;
        let pkg = PinnedLocalDeleteTree::new(&dir)
            .map_err(|e| format!("not an admitted npm global package {}: {e}", w.name))?;
        if !pkg.is_dir() {
            return Err("package is not an ordinary directory".into());
        }
        let raw = pkg
            .read_text(&dir.join("package.json"))
            .map_err(|e| format!("manifest: {e}"))?;
        let mut files = vec![];
        for (stem, target) in bin_entries(&raw, &w.name)? {
            // The entry itself must be an ordinary file in this held package.
            if !pkg.contains_file(&dir.join(target.replace('/', std::path::MAIN_SEPARATOR_STR))) {
                return Err("bin entry is not an ordinary file in the selected package".into());
            }
            let relative = format!("node_modules/{}/{target}", w.name);
            let node_templates = shim_templates(&relative, true);
            let direct_templates = shim_templates(&relative, false);
            for (i, ext) in [".cmd", ".ps1", ""].iter().enumerate() {
                let path = prefix.join(format!("{stem}{ext}"));
                // Prefix remains held; metadata of the final link never follows it.
                match crate::fsutil::local_scan_exists(&path) {
                    Ok(false) => continue,
                    Err(e) => return Err(format!("shim metadata: {e}")),
                    Ok(true) => {}
                }
                if !seen_paths.insert(path.to_string_lossy().to_ascii_lowercase()) {
                    return Err("overlapping shim claims".into());
                }
                let shim =
                    PinnedLocalDeleteTree::new(&path).map_err(|e| format!("shim snapshot: {e}"))?;
                let text = shim
                    .read_text(&path)
                    .map_err(|e| format!("shim read: {e}"))?
                    .replace("\r\n", "\n");
                if text != node_templates[i] && text != direct_templates[i] {
                    return Err(format!(
                        "shim ownership/format not proven: {}",
                        path.display()
                    ));
                }
                files.push(shim);
            }
        }
        files.push(pkg);
        targets.push(PackageTargets {
            name: w.name.clone(),
            files,
        });
    }
    if targets.is_empty() {
        return Err("no packages selected".into());
    }
    Ok(targets)
}

/// Dedicated handle-backed backup: generic backup_items reopens sources and its
/// copy_dir creates source markers, incompatible with immutable sharing pins.
fn backup_targets(targets: &[PackageTargets], session: &Path) -> Result<(), String> {
    backup_targets_with(targets, session, |_, _| Ok(()), |_| Ok(()))
}

fn backup_targets_with(
    targets: &[PackageTargets],
    session: &Path,
    before_copy: impl FnOnce(&Path, &Path) -> Result<(), String>,
    before_seal: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = (targets, session, before_copy, before_seal);
        Err("Windows backup required".into())
    }
    #[cfg(windows)]
    {
        let stage = crate::path_seal::backup_stage()?;
        let mut map = std::collections::BTreeMap::new();
        std::fs::create_dir_all(stage.path().join("files")).map_err(|e| e.to_string())?;
        let mut index = 0;
        for target in targets {
            for file in &target.files {
                let rel = format!("{index}_npm");
                index += 1;
                map.insert(rel, file.path().to_string_lossy().into_owned());
            }
        }
        before_copy(stage.path(), session)?;
        for target in targets {
            for file in &target.files {
                let rel = map
                    .iter()
                    .find(|(_, p)| Path::new(p.as_str()) == file.path())
                    .unwrap()
                    .0;
                file.copy_to(&stage.path().join("files").join(rel))
                    .map_err(|e| format!("copy: {e}"))?;
            }
        }
        stage.verify_snapshots()?;
        std::fs::create_dir(stage.path().join("registry")).map_err(|e| e.to_string())?;
        crate::backup::publish_verified_stage(
            stage.path(),
            session,
            &map,
            &std::collections::BTreeMap::new(),
            before_seal,
        )
    }
}

fn remove_with(
    prefix: &Path,
    node_modules: &Path,
    wanted: &[NpmPackageRef],
) -> Result<NpmCleanupReport, String> {
    remove_with_backup(prefix, node_modules, wanted, backup_targets)
}

fn remove_with_backup(
    prefix: &Path,
    node_modules: &Path,
    wanted: &[NpmPackageRef],
    backup: impl FnOnce(&[PackageTargets], &Path) -> Result<(), String>,
) -> Result<NpmCleanupReport, String> {
    let targets = collect_targets(prefix, node_modules, wanted)?;
    let session = crate::backup::create_session("npm-globals")
        .map_err(|e| format!("backup session failed: {e}"))?;
    backup(&targets, &session).map_err(|e| format!("backup failed; npm packages kept: {e}"))?;
    #[cfg(windows)]
    let (seal, _seal_pin) = crate::path_seal::hold_verified_seal(&session)?;
    #[cfg(windows)]
    let copies = crate::backup::PublishedFiles::open(&session, &seal)?;
    let mut report = NpmCleanupReport {
        backup_dir: session.to_string_lossy().into_owned(),
        ..Default::default()
    };
    for target in targets {
        // A package and all its shims need recoverable copies before any of
        // them are removed. Keep the copies open for the entire group.
        #[cfg(windows)]
        let _held_copies = match target
            .files
            .iter()
            .map(|tree| copies.hold(copies.key_for(tree.path())?))
            .collect::<std::io::Result<Vec<_>>>()
        {
            Ok(held) => held,
            Err(error) => {
                report.failed.push(format!(
                    "{}: backup copy could not be verified: {error}",
                    target.name
                ));
                continue;
            }
        };
        let mut error = None;
        for file in target.files {
            if crate::executor::cleanup_cancelled() {
                error = Some("cleanup cancelled".into());
                break;
            }
            if let Err(e) = file.remove_backed_up() {
                error = Some(e.to_string());
                break;
            }
        }
        if let Some(e) = error {
            report.failed.push(format!("{}: {e}", target.name));
        } else {
            report.removed.push(target.name);
        }
    }
    Ok(report)
}

/// Re-derive roots and inventory rather than accepting frontend paths.
pub fn remove_npm_globals(wanted: &[NpmPackageRef]) -> Result<NpmCleanupReport, String> {
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::executor::begin_cleanup_control();
    if crate::executor::reboot::any_pending() {
        return Err(crate::executor::reboot::PENDING_MESSAGE.to_string());
    }
    let roots = crate::cliglobals::roots_from_env();
    let fresh = crate::cliglobals::scan_with(&roots);
    let prefix = roots.npm_prefix.ok_or("npm global folder not found")?;
    for w in wanted {
        if !fresh
            .items
            .iter()
            .any(|i| i.manager == "npm" && i.name == w.name)
        {
            return Err(format!(
                "package is no longer listed by the fresh scan: {}",
                w.name
            ));
        }
    }
    remove_with(&prefix, &prefix.join("node_modules"), wanted)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn seed(root: &Path, name: &str) -> (PathBuf, PathBuf) {
        let prefix = root.join("prefix");
        let nm = prefix.join("node_modules");
        let pkg = package_path(&nm, name).unwrap();
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("package.json"), r#"{"bin":{"tool":"cli.js"}}"#).unwrap();
        std::fs::write(
            pkg.join("cli.js"),
            b"#!/usr/bin/env node\nconsole.log('fixture');\n",
        )
        .unwrap();
        for (ext, text) in [".cmd", ".ps1", ""]
            .iter()
            .zip(shim_templates(&format!("node_modules/{name}/cli.js"), true))
        {
            std::fs::write(
                prefix.join(format!("tool{ext}")),
                text.replace('\n', "\r\n"),
            )
            .unwrap();
        }
        (prefix, nm)
    }
    fn wanted(name: &str) -> Vec<NpmPackageRef> {
        vec![NpmPackageRef { name: name.into() }]
    }
    fn intact(prefix: &Path, nm: &Path, name: &str) {
        assert_eq!(
            std::fs::read(nm.join(name).join("cli.js")).unwrap(),
            b"#!/usr/bin/env node\nconsole.log('fixture');\n"
        );
        for ext in [".cmd", ".ps1", ""] {
            assert!(prefix.join(format!("tool{ext}")).is_file());
        }
    }

    #[test]
    fn npm_backup_refuses_named_streams_and_keeps_the_package() {
        let _env = crate::backup::lock_backup_env();
        let tmp = crate::test_support::TempDir::new("npm-named-streams");
        let _store = crate::path_seal::TestStore::new(tmp.join("seals"));
        let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", tmp.join("vault"));
        let (prefix, nm) = seed(tmp.path(), "fixture");
        let stream = nm.join("fixture/cli.js:FixtureMetadata");
        std::fs::write(&stream, b"keep this stream").unwrap();

        let error = remove_with(&prefix, &nm, &wanted("fixture")).unwrap_err();
        assert!(error.contains("named data streams"), "{error}");
        intact(&prefix, &nm, "fixture");
        assert_eq!(std::fs::read(stream).unwrap(), b"keep this stream");
    }

    #[test]
    fn generated_cmd_shim_matches_supported_templates() {
        // Opt-in compatibility fixture generated by an installed cmd-shim.
        // The standard suite does not require npm/node to be installed.
        let Some(generator) = std::env::var_os("REMOVA_TEST_CMD_SHIM") else {
            return;
        };
        let _env = crate::backup::lock_backup_env();
        let tmp = crate::test_support::TempDir::new("npm-real-shims");
        let _store = crate::path_seal::TestStore::new(tmp.path().join("seals"));
        let _backup =
            crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", tmp.path().join("vault"));
        for node in [true, false] {
            let root = tmp.subdir(if node { "node" } else { "direct" });
            let (prefix, nm) = seed(&root, "@scope/good");
            // Exercise npm's ordinary leading ./ manifest spelling.
            std::fs::write(
                nm.join("@scope/good/package.json"),
                r#"{"bin":{"tool":"./cli.js"}}"#,
            )
            .unwrap();
            if !node {
                std::fs::write(nm.join("@scope/good/cli.js"), b"plain executable fixture").unwrap();
            }
            let status = std::process::Command::new("node")
                .arg("-e").arg("require(process.argv[1])(process.argv[2], process.argv[3]).catch(e=>{console.error(e);process.exitCode=1})")
                .arg(&generator).arg(nm.join("@scope/good/./cli.js")).arg(prefix.join("tool"))
                .status().unwrap();
            assert!(status.success());
            let held = collect_targets(&prefix, &nm, &wanted("@scope/good")).unwrap();
            assert_eq!(held[0].files.len(), 4);
            let originals: std::collections::BTreeMap<_, _> = [".cmd", ".ps1", ""]
                .iter()
                .map(|ext| {
                    let path = prefix.join(format!("tool{ext}"));
                    (
                        path.to_string_lossy().into_owned(),
                        std::fs::read(path).unwrap(),
                    )
                })
                .collect();
            drop(held);
            let manifest_bytes = std::fs::read(nm.join("@scope/good/package.json")).unwrap();
            let error =
                remove_with_backup(&prefix, &nm, &wanted("@scope/good"), |targets, session| {
                    backup_targets_with(
                        targets,
                        session,
                        |stage, _| {
                            std::fs::write(stage.join("files/0_npm"), b"copy collision").unwrap();
                            Ok(())
                        },
                        |_| panic!("copy collision must fail before sealing"),
                    )
                })
                .unwrap_err();
            assert!(error.contains("copy:"), "{error}");
            assert_eq!(
                std::fs::read(nm.join("@scope/good/package.json")).unwrap(),
                manifest_bytes
            );
            for (original, bytes) in &originals {
                assert_eq!(std::fs::read(original).unwrap(), *bytes);
            }
            let report =
                remove_with_backup(&prefix, &nm, &wanted("@scope/good"), |targets, session| {
                    backup_targets(targets, session)?;
                    let files = session.join("files");
                    let map: std::collections::BTreeMap<String, String> = serde_json::from_slice(
                        &std::fs::read(files.join("path_map.json")).unwrap(),
                    )
                    .unwrap();
                    let shim = map
                        .iter()
                        .find(|(_, original)| Path::new(original) == prefix.join("tool.cmd"));
                    let (key, _) = shim.expect("command shim is backed up");
                    std::fs::remove_file(files.join(key)).unwrap();
                    Ok(())
                })
                .unwrap();
            assert!(report.removed.is_empty());
            assert_eq!(report.failed.len(), 1);
            assert!(report.failed[0].contains("backup copy could not be verified"));
            assert_eq!(
                std::fs::read(nm.join("@scope/good/package.json")).unwrap(),
                manifest_bytes
            );
            for (original, bytes) in &originals {
                assert_eq!(std::fs::read(original).unwrap(), *bytes);
            }
            let report = remove_with(&prefix, &nm, &wanted("@scope/good")).unwrap();
            assert!(report.failed.is_empty(), "{:?}", report.failed);
            assert_eq!(report.removed, ["@scope/good"]);
            let session = Path::new(&report.backup_dir);
            assert!(session.starts_with(tmp.path()));
            let seal = crate::path_seal::verified_seal(session).unwrap();
            let manifest = crate::path_seal::read_file_manifest(
                session,
                seal.file_manifest_digest
                    .as_deref()
                    .expect("npm payload seal"),
            )
            .unwrap();
            let map: std::collections::BTreeMap<String, String> = serde_json::from_slice(
                &std::fs::read(session.join("files/path_map.json")).unwrap(),
            )
            .unwrap();
            // Restore only this fixture's shim bytes by the exported path map;
            // this is not a claim that the public restore IPC was executed.
            for (rel, original) in map {
                assert!(manifest.contains_key(&format!("files/{rel}")));
                if let Some(expected) = originals.get(&original) {
                    let bytes = std::fs::read(session.join("files").join(rel)).unwrap();
                    assert_eq!(&bytes, expected);
                    std::fs::write(&original, bytes).unwrap();
                    assert_eq!(std::fs::read(original).unwrap(), *expected);
                }
            }
        }
    }

    #[test]
    fn bin_keys_are_single_safe_file_stems() {
        for stem in [
            "../outside",
            r"..\outside",
            r"C:\outside",
            "C:outside",
            "shim:stream",
            "CON",
            "nul.js",
            "COM1",
            "lpt9.cmd",
            "tool.",
            "tool ",
            ".",
            "..",
            "",
        ] {
            assert!(!safe_bin_stem(stem), "unsafe key: {stem}");
        }
        for stem in ["tool", "tool-cli", "tool_2", "tool.js", "COM10"] {
            assert!(safe_bin_stem(stem));
        }
    }

    #[test]
    fn manifest_and_foreign_shim_fail_closed() {
        let tmp = crate::test_support::TempDir::new("npm-admission");
        let (prefix, nm) = seed(tmp.path(), "good");
        std::fs::write(
            prefix.join("tool.cmd"),
            shim_templates("node_modules/other/cli.js", true)[0].as_bytes(),
        )
        .unwrap();
        assert!(collect_targets(&prefix, &nm, &wanted("good"))
            .err()
            .unwrap()
            .contains("ownership"));
        assert!(collect_targets(&prefix, &nm, &wanted("../good")).is_err());
        for raw in [
            "{",
            r#"{"bin":{"../escape":"cli.js"}}"#,
            r#"{"bin":{"CON":"cli.js"}}"#,
            r#"{"bin":{"tool":"../other/cli.js"}}"#,
            r#"{"bin":{"tool":3}}"#,
        ] {
            std::fs::write(nm.join("good/package.json"), raw).unwrap();
            assert!(collect_targets(&prefix, &nm, &wanted("good")).is_err());
        }
        intact(&prefix, &nm, "good");
    }

    #[test]
    fn isolated_backup_then_exact_handle_delete() {
        let _env = crate::backup::lock_backup_env();
        let tmp = crate::test_support::TempDir::new("npm-remove");
        let _store = crate::path_seal::TestStore::new(tmp.path().join("seals"));
        let _backup =
            crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", tmp.path().join("vault"));
        let (prefix, nm) = seed(tmp.path(), "@scope/good");
        std::fs::create_dir_all(nm.join(".bin")).unwrap();
        std::fs::write(nm.join(".bin/tool.cmd"), b"unrelated local layout").unwrap();
        let report = remove_with(&prefix, &nm, &wanted("@scope/good")).unwrap();
        assert_eq!(report.removed, ["@scope/good"]);
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert!(!nm.join("@scope/good").exists());
        assert!(!prefix.join("tool.cmd").exists());
        assert!(nm.join(".bin/tool.cmd").exists());
        let session = Path::new(&report.backup_dir);
        assert!(session.starts_with(tmp.path()));
        assert!(crate::path_seal::verified_seal(session).is_ok());
        let map: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&std::fs::read(session.join("files/path_map.json")).unwrap())
                .unwrap();
        assert_eq!(map.len(), 4);
        for (rel, original) in map {
            let saved = session.join("files").join(rel);
            if original.ends_with("good") {
                assert!(saved.join("cli.js").is_file());
            } else {
                assert!(saved.is_file());
            }
        }
    }

    #[test]
    fn admitted_package_survives_session_copy_and_seal_failures() {
        let _env = crate::backup::lock_backup_env();
        let tmp = crate::test_support::TempDir::new("npm-backup-fail");
        let seals = tmp.path().join("seals");
        let _store = crate::path_seal::TestStore::new(seals.clone());
        let (prefix, nm) = seed(tmp.path(), "keepme");
        let snapshot = || {
            let mut paths = vec![nm.join("keepme/package.json"), nm.join("keepme/cli.js")];
            paths.extend(
                [".cmd", ".ps1", ""]
                    .iter()
                    .map(|ext| prefix.join(format!("tool{ext}"))),
            );
            paths
                .into_iter()
                .map(|p| (p.clone(), std::fs::read(p).unwrap()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let original = snapshot();
        let blocked = tmp.path().join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        {
            let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", &blocked);
            let e = remove_with(&prefix, &nm, &wanted("keepme")).unwrap_err();
            assert!(e.contains("backup session failed"), "{e}");
            assert_eq!(snapshot(), original);
            intact(&prefix, &nm, "keepme");
        }
        let _backup =
            crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", tmp.path().join("vault"));
        let e = remove_with_backup(&prefix, &nm, &wanted("keepme"), |targets, session| {
            backup_targets_with(
                targets,
                session,
                |stage, _| {
                    std::fs::write(stage.join("files/0_npm"), b"copy collision").unwrap();
                    Ok(())
                },
                |_| panic!("copy failure must precede seal"),
            )
        })
        .unwrap_err();
        assert!(e.contains("backup failed") && e.contains("copy:"), "{e}");
        assert_eq!(snapshot(), original);
        intact(&prefix, &nm, "keepme");
        let e = remove_with_backup(&prefix, &nm, &wanted("keepme"), |targets, session| {
            backup_targets_with(
                targets,
                session,
                |_, _| Ok(()),
                |session| {
                    assert!(
                        session.join("files/path_map.json").is_file(),
                        "copy reached publication"
                    );
                    std::fs::create_dir(seals.join(format!(
                        "{}.seal.json",
                        session.file_name().unwrap().to_string_lossy()
                    )))
                    .unwrap();
                    Ok(())
                },
            )
        })
        .unwrap_err();
        assert!(e.contains("backup failed"), "{e}");
        assert_eq!(snapshot(), original);
        intact(&prefix, &nm, "keepme");
    }

    #[test]
    fn no_bin_preserves_unrelated_prefix_shims_and_links_are_refused() {
        let _env = crate::backup::lock_backup_env();
        let tmp = crate::test_support::TempDir::new("npm-no-bin");
        let _store = crate::path_seal::TestStore::new(tmp.path().join("seals"));
        let _backup =
            crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", tmp.path().join("vault"));
        let (prefix, nm) = seed(tmp.path(), "quiet");
        std::fs::write(nm.join("quiet/package.json"), b"{}").unwrap();
        let bytes = std::fs::read(prefix.join("tool.cmd")).unwrap();
        let report = remove_with(&prefix, &nm, &wanted("quiet")).unwrap();
        assert_eq!(report.removed, ["quiet"]);
        assert_eq!(std::fs::read(prefix.join("tool.cmd")).unwrap(), bytes);
        let target = tmp.subdir("elsewhere");
        std::fs::write(target.join("package.json"), b"{}").unwrap();
        let link = nm.join("linked");
        assert!(
            std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
                .args(["/c", "mklink", "/J"])
                .arg(&link)
                .arg(&target)
                .status()
                .unwrap()
                .success()
        );
        assert!(collect_targets(&prefix, &nm, &wanted("linked")).is_err());
        assert_eq!(std::fs::read(target.join("package.json")).unwrap(), b"{}");
    }

    #[test]
    fn held_targets_refuse_ancestor_file_and_manifest_replacement() {
        let tmp = crate::test_support::TempDir::new("npm-pins");
        let (prefix, nm) = seed(tmp.path(), "good");
        let held = collect_targets(&prefix, &nm, &wanted("good")).unwrap();
        for path in [
            &prefix,
            &nm,
            &nm.join("good"),
            &nm.join("good/package.json"),
            &prefix.join("tool.cmd"),
        ] {
            assert!(
                std::fs::rename(path, path.with_extension("moved")).is_err(),
                "{}",
                path.display()
            );
        }
        assert!(std::fs::write(prefix.join("tool.cmd"), b"replacement").is_err());
        assert!(std::fs::write(nm.join("good/package.json"), b"{}").is_err());
        drop(held);
        intact(&prefix, &nm, "good");
    }
}
