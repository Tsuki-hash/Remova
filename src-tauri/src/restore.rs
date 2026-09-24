//! Restore from backup session.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn restore_session(session: &Path) -> Result<Vec<String>, String> {
    if !session.is_dir() {
        return Err(crate::error::restore_err("session not found").to_ipc());
    }
    let mut messages = vec![];

    // Files via path_map.json
    let map_path = session.join("files").join("path_map.json");
    let files_root = session.join("files");
    if map_path.exists() {
        let raw = fs::read_to_string(&map_path).map_err(|e| e.to_string())?;
        let map: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        for (rel, original) in &map {
            let src = files_root.join(rel);
            if !src.exists() {
                continue;
            }
            let dest = PathBuf::from(&original);
            // S-R6-03: never restore into red-line / system-shaped destinations.
            // A tampered path_map.json must not become an arbitrary-write primitive.
            if original.trim().is_empty()
                || !crate::safety::is_safe_restore_target(&dest)
                || crate::safety::looks_like_sync_conflict(&original)
            {
                return Err(crate::error::restore_err(format!(
                    "refusing to restore into protected path: {original}"
                ))
                .to_ipc());
            }
            // Library subpaths require a matching out-of-session seal (N-risk):
            // only destinations recorded at backup time may write back under Documents/….
            if crate::safety::is_user_library_path(&original)
                && !crate::path_seal::target_sealed(session, &map, &original)
            {
                return Err(crate::error::restore_err(format!(
                    "refusing unsealed library restore: {original}"
                ))
                .to_ipc());
            }
            if src.is_dir() {
                copy_dir(&src, &dest).map_err(|_| format!("restore:copy::{original}"))?;
            } else {
                if let Some(p) = dest.parent() {
                    fs::create_dir_all(p).map_err(|_| "restore:io".to_string())?;
                }
                fs::copy(&src, &dest).map_err(|_| format!("restore:copy::{original}"))?;
            }
            messages.push(format!("restored {original}"));
        }
    }

    // PATH leftovers (path.json) — merge missing segments back; never whole-env overwrite.
    let path_snap = session.join("path.json");
    if path_snap.exists() {
        let raw = fs::read_to_string(&path_snap).map_err(|e| e.to_string())?;
        let snap: crate::backup::PathSnapshot =
            serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        for it in &snap.items {
            let scopes = path_restore_scopes(it);
            let scope_refs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
            match crate::regops::restore_path_entry(&it.entry, &scope_refs) {
                Ok(true) => messages.push(format!("restored PATH entry {}", it.entry)),
                Ok(false) => messages.push(format!("PATH entry already present: {}", it.entry)),
                Err(_) => {
                    return Err(crate::error::restore_path_err(format!(
                        "PATH restore failed for {}",
                        it.entry
                    ))
                    .to_ipc())
                }
            }
        }
    }

    // Registry exports
    let reg_root = session.join("registry");
    if reg_root.is_dir() {
        for e in fs::read_dir(&reg_root).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            // AR-05: prefer single-value restore when value.reg exists (Run values etc.).
            let value_reg = e.path().join("value.reg");
            let export = e.path().join("export.reg");
            let target = if value_reg.exists() {
                Some(value_reg)
            } else if export.exists() {
                Some(export)
            } else {
                None
            };
            if let Some(target) = target {
                // S-R7-03: key-shape whitelist before `reg import` (Uninstall / Run value-level / …).
                validate_reg_import(&target)?;
                let mut cmd = Command::new(crate::regops::sys_tool("reg.exe"));
                cmd.args(["import", &target.to_string_lossy()]);
                crate::regops::hide_console(&mut cmd);
                let out = cmd.output().map_err(|e| e.to_string())?;
                if out.status.success() {
                    messages.push(format!("imported {}", target.display()));
                } else {
                    return Err(crate::error::restore_reg_err(format!(
                        "reg import failed {}",
                        target.display()
                    ))
                    .to_ipc());
                }
            }
        }
    }

    Ok(messages)
}

/// S-R7-03: reject `.reg` files whose key shapes fall outside the restore whitelist.
/// Allowed: Uninstall / App Paths / Services / TaskCache vendor trees (via
/// `is_safe_to_delete_registry`), plus Run/RunOnce **value-level** restores only.
fn validate_reg_import(path: &Path) -> Result<(), String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    reg_content_allowed(&raw)
}

/// Parse `.reg` text and enforce the key-shape whitelist (S-R7-03).
fn reg_content_allowed(raw: &str) -> Result<(), String> {
    let mut saw_header = false;
    let mut current_key: Option<String> = None;
    let mut current_is_run = false;
    let mut current_has_value = false;
    let mut current_has_delete = false;

    let flush = |key: &Option<String>,
                 is_run: bool,
                 has_value: bool,
                 has_delete: bool|
     -> Result<(), String> {
        let Some(k) = key else {
            return Ok(());
        };
        if has_delete {
            return Err(format!("reg import refused (key deletion): {k}"));
        }
        if is_run {
            // Run roots: value-level restores only — at least one named/default write, no bare key.
            if !has_value {
                return Err(format!("reg import refused (Run key without values): {k}"));
            }
            return Ok(());
        }
        crate::safety::is_safe_to_delete_registry(k).map_err(|e| format!("reg import refused: {e}"))
    };

    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("Windows Registry Editor") || t.starts_with("REGEDIT") {
            saw_header = true;
            continue;
        }
        if t.starts_with('[') {
            flush(
                &current_key,
                current_is_run,
                current_has_value,
                current_has_delete,
            )?;
            current_has_value = false;
            current_has_delete = false;
            let body = t.trim_start_matches('[').trim_end_matches(']').trim();
            if body.is_empty() {
                return Err("reg import refused (empty key header)".into());
            }
            if let Some(stripped) = body.strip_prefix('-') {
                current_has_delete = true;
                current_key = Some(reg_hive_to_remova(stripped)?);
            } else {
                current_key = Some(reg_hive_to_remova(body)?);
            }
            let key_ref = current_key.as_deref().unwrap_or("");
            // Run/RunOnce roots are value-level restore targets only (AR-05).
            current_is_run = !key_ref.contains('|') && crate::safety::is_allowed_run_key(key_ref);
            continue;
        }
        // Value / delete lines under the current key.
        if t.starts_with('-') {
            current_has_delete = true;
        } else if t.starts_with('@') || t.starts_with('"') {
            current_has_value = true;
        }
    }
    flush(
        &current_key,
        current_is_run,
        current_has_value,
        current_has_delete,
    )?;
    if !saw_header {
        return Err("reg import refused (missing REG header)".into());
    }
    if current_key.is_none() {
        return Err("reg import refused (no keys)".into());
    }
    Ok(())
}

/// Map `HKEY_*` headers in `.reg` files onto Remova hive aliases.
fn reg_hive_to_remova(key: &str) -> Result<String, String> {
    let k = key.trim().trim_matches('"');
    let up = k.to_uppercase();
    if up.strip_prefix("HKEY_LOCAL_MACHINE\\").is_some() {
        // Preserve original casing of the subkey via the original string.
        let orig_rest = &k["HKEY_LOCAL_MACHINE\\".len()..];
        return Ok(format!(r"HKLM64\{orig_rest}"));
    }
    if let Some(_rest) = up.strip_prefix("HKEY_CURRENT_USER\\") {
        let orig_rest = &k["HKEY_CURRENT_USER\\".len()..];
        return Ok(format!(r"HKCU\{orig_rest}"));
    }
    if up.starts_with("HKLM\\") || up.starts_with("HKLM64\\") || up.starts_with("HKLM32\\") {
        return Ok(k.to_string());
    }
    if up.starts_with("HKCU\\") {
        return Ok(k.to_string());
    }
    Err(format!("reg import refused (unsupported hive): {k}"))
}

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    crate::fsutil::copy_dir(src, dest)
}

/// Prefer scopes recorded at backup; fall back to PATH strings in the snapshot; else User only.
fn path_restore_scopes(item: &crate::backup::PathSnapshotItem) -> Vec<String> {
    if !item.scopes.is_empty() {
        return item.scopes.clone();
    }
    let mut out = Vec::new();
    if crate::regops::path_contains_entry(&item.user_path, &item.entry) {
        out.push("User".into());
    }
    if crate::regops::path_contains_entry(&item.machine_path, &item.entry) {
        out.push("Machine".into());
    }
    if out.is_empty() {
        // S-R4-10: never silently write Machine PATH when backup has no scope evidence.
        out.push("User".into());
    }
    out
}

pub fn list_sessions() -> Vec<PathBuf> {
    let root = crate::backup::backup_root();
    let mut out = vec![];
    if let Ok(rd) = root.read_dir() {
        for e in rd.flatten() {
            if e.path().is_dir() {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out.reverse();
    out
}

pub fn list_session_names() -> Vec<String> {
    list_sessions()
        .iter()
        .filter_map(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .collect()
}

pub fn restore_by_name(name: &str) -> Result<Vec<String>, String> {
    if name.is_empty() || name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err("invalid session name".into());
    }
    let path = crate::backup::backup_root().join(name);
    restore_session(&path)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub size_kb: u64,
    pub created_at: String,
}

fn dir_size_kb(p: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                total = total.saturating_add(dir_size_kb(&path));
            } else if let Ok(md) = e.metadata() {
                total = total.saturating_add(md.len() / 1024);
            }
        }
    }
    total
}

/// List backup sessions with size (KB). Does NOT prune (read path is pure).
pub fn list_session_info() -> Vec<SessionInfo> {
    list_sessions()
        .iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().to_string();
            Some(SessionInfo {
                name: name.clone(),
                size_kb: dir_size_kb(p),
                created_at: name.split('_').next().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// Delete backup sessions older than `days` (Safety Vault retention). Returns removed count.
pub fn prune_old_sessions(days: u64) -> usize {
    prune_old_sessions_at(days, None)
}

/// Same as [`prune_old_sessions`] with optional fixed `now` (unix secs) for tests.
pub fn prune_old_sessions_at(days: u64, now_override: Option<u64>) -> usize {
    let now = now_override.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    let cutoff = now.saturating_sub(days.saturating_mul(24 * 3600));
    let mut removed = 0usize;
    for p in list_sessions() {
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let ts: u64 = name.split('_').next().unwrap_or("").parse().unwrap_or(0);
        let expired = if ts > 0 {
            ts < cutoff
        } else {
            // fallback: directory mtime
            p.metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() < cutoff)
                .unwrap_or(false)
        };
        if expired && fs::remove_dir_all(&p).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Delete one backup session by name. Path-traversal guarded.
/// R-R7-02: session names must match `^[0-9]{8}-[0-9]{6}` (`YYYYMMDD-HHMMSS…`).
pub fn delete_session_by_name(name: &str) -> Result<(), String> {
    // Reject `.`, `..`, separators, and anything that is not a real session folder name
    // (`YYYYMMDD-HHMMSS-…`). `backup_root().join(".")` is the backup root itself.
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || !is_session_name(name)
    {
        return Err("invalid session name".into());
    }
    let path = crate::backup::backup_root().join(name);
    // Defense-in-depth: resolved folder must stay under backup_root and look like a session.
    let root = crate::backup::backup_root();
    if !path.starts_with(&root) || path == root {
        return Err("invalid session name".into());
    }
    if !path.is_dir() {
        return Err("session not found".into());
    }
    fs::remove_dir_all(&path).map_err(|e| e.to_string())
}

/// `YYYYMMDD-HHMMSS` prefix (digits only, fixed widths).
fn is_session_name(name: &str) -> bool {
    // Legacy `YYYYMMDD-HHMMSS…` (optionally followed by `_extra`).
    let b = name.as_bytes();
    if b.len() >= 15
        && b[..8].iter().all(|c| c.is_ascii_digit())
        && b[8] == b'-'
        && (9..15).all(|i| b[i].is_ascii_digit())
    {
        return true;
    }
    // create_session format: `{unix_secs}_{safe}` (see prune_old_sessions).
    if let Some((ts, rest)) = name.split_once('_') {
        return !ts.is_empty()
            && ts.chars().all(|c| c.is_ascii_digit())
            && !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn restore_missing_session_errors() {
        let p = std::env::temp_dir().join("remova_no_such_session_xyz");
        assert!(restore_session(&p).is_err());
    }

    #[test]
    fn prune_old_sessions_removes_aged_dirs() {
        let _guard = crate::backup::lock_backup_env();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp =
            std::env::temp_dir().join(format!("remova_prune_{}_{}", std::process::id(), nanos));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("REMOVA_BACKUP_DIR", &tmp);
        let root = crate::backup::backup_root();
        assert_eq!(root, tmp);
        let now = 1_700_000_000u64;
        let old = root.join(format!("{}_prune_test_old", now - 30 * 24 * 3600));
        let fresh = root.join(format!("{}_prune_test_fresh", now - 24 * 3600));
        let _ = fs::create_dir_all(&old);
        let _ = fs::create_dir_all(&fresh);
        let removed = super::prune_old_sessions_at(7, Some(now));
        assert!(removed >= 1);
        assert!(!old.exists());
        assert!(fresh.exists());
        // Do not remove_var here (NEW-B): the guard serializes env access; the
        // process exits after the suite and no other test needs the default root.
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn restore_file_roundtrip() {
        let tmp = std::env::temp_dir().join("remova_restore_test");
        let _ = fs::remove_dir_all(&tmp);
        let sess = tmp.join("sess");
        let files = sess.join("files");
        fs::create_dir_all(&files).unwrap();
        let orig = tmp.join("orig\\file.txt");
        fs::create_dir_all(orig.parent().unwrap()).unwrap();
        fs::write(&orig, b"hello").unwrap();
        let rel = "abc_file.txt";
        fs::write(files.join(rel), b"hello-backup").unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert(rel.to_string(), orig.to_string_lossy().to_string());
        fs::write(
            files.join("path_map.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        // restore overwrites orig from backup
        let msgs = restore_session(&sess).unwrap();
        assert!(msgs.iter().any(|m| m.contains("restored")));
        assert_eq!(fs::read_to_string(&orig).unwrap(), "hello-backup");
        let _ = fs::remove_dir_all(&tmp);
    }

    /// N-risk: library-subpath write-back requires a matching out-of-session seal.
    #[test]
    fn restore_refuses_unsealed_library_target() {
        let tmp = std::env::temp_dir().join(format!("remova_restore_seal_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let seals = tmp.join("seals");
        std::fs::create_dir_all(&seals).unwrap();
        let _g = crate::path_seal::test_lock();
        crate::path_seal::set_seals_root_for_tests(Some(seals.clone()));

        let sess = tmp.join("1700000000_sealme");
        let files = sess.join("files");
        fs::create_dir_all(&files).unwrap();
        let dest = tmp.join("Documents").join("App").join("f.txt");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(files.join("abc_f.txt"), b"payload").unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert("abc_f.txt".to_string(), dest.to_string_lossy().to_string());
        let map_json = serde_json::to_string_pretty(&map).unwrap();
        fs::write(files.join("path_map.json"), &map_json).unwrap();

        // Unsealed → refuse (even though shape gate would allow a library subpath).
        let err = restore_session(&sess).unwrap_err();
        assert!(err.contains("unsealed") || err.contains("protected"), "{err}");

        // Sealed → restore proceeds.
        crate::path_seal::write_seal(&sess, "", &map).unwrap();
        assert!(
            crate::path_seal::target_sealed(&sess, &map, &dest.to_string_lossy()),
            "seal must accept the recorded target"
        );
        let msgs = restore_session(&sess).unwrap();
        assert!(msgs.iter().any(|m| m.contains("restored")));
        assert_eq!(fs::read_to_string(&dest).unwrap(), "payload");

        crate::path_seal::set_seals_root_for_tests(None);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn path_restore_scopes_prefer_recorded() {
        use crate::backup::PathSnapshotItem;
        let item = PathSnapshotItem {
            entry: r"C:\tools\foo".into(),
            scopes: vec!["User".into()],
            user_path: r"C:\other".into(),
            machine_path: String::new(),
        };
        assert_eq!(super::path_restore_scopes(&item), vec!["User".to_string()]);

        let empty_scopes = PathSnapshotItem {
            entry: r"C:\tools\foo".into(),
            scopes: vec![],
            user_path: r"C:\tools\foo;C:\Windows".into(),
            machine_path: r"C:\Windows".into(),
        };
        assert_eq!(
            super::path_restore_scopes(&empty_scopes),
            vec!["User".to_string()]
        );

        let none = PathSnapshotItem {
            entry: r"C:\tools\foo".into(),
            scopes: vec![],
            user_path: String::new(),
            machine_path: String::new(),
        };
        // S-R4-10: no evidence → User only.
        assert_eq!(super::path_restore_scopes(&none), vec!["User".to_string()]);
    }

    #[test]
    fn restore_session_reads_path_snapshot_without_mutating_system_path() {
        // Pure merge helpers + snapshot parse — do not call restore_path_entry (would write PATH).
        use crate::regops::{merge_path_entry, path_contains_entry};
        assert!(path_contains_entry(r"C:\a;C:\b", r"c:\b\"));
        assert!(!path_contains_entry(r"C:\a", r"C:\b"));
        assert_eq!(merge_path_entry(r"C:\a;C:\b", r"C:\b"), None);
        assert_eq!(
            merge_path_entry(r"C:\a", r"C:\b"),
            Some(r"C:\a;C:\b".to_string())
        );

        let tmp = std::env::temp_dir().join(format!("remova_path_snap_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let snap = crate::backup::PathSnapshot {
            items: vec![crate::backup::PathSnapshotItem {
                entry: r"C:\vendor\tool".into(),
                scopes: vec!["User".into()],
                user_path: r"C:\vendor\tool;C:\Windows".into(),
                machine_path: r"C:\Windows".into(),
            }],
        };
        fs::write(
            tmp.join("path.json"),
            serde_json::to_string_pretty(&snap).unwrap(),
        )
        .unwrap();
        let raw = fs::read_to_string(tmp.join("path.json")).unwrap();
        let back: crate::backup::PathSnapshot = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.items[0].entry, r"C:\vendor\tool");
        assert_eq!(super::path_restore_scopes(&back.items[0]), vec!["User"]);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn delete_session_requires_timestamp_name() {
        // R-R7-02: only `YYYYMMDD-HHMMSS…` session folders are deletable by name.
        assert!(super::is_session_name("20260922-120000"));
        assert!(super::is_session_name("20260922-120000_extra"));
        assert!(!super::is_session_name(""));
        assert!(!super::is_session_name("2026092-120000"));
        assert!(!super::is_session_name("20260922120000"));
        assert!(!super::is_session_name("20260922-12000"));
        assert!(!super::is_session_name("abcdefgh-ijklmn"));
        assert!(crate::restore::delete_session_by_name("not-a-session").is_err());
        assert!(crate::restore::delete_session_by_name("20260922-abc").is_err());
    }

    // S-R7-03: `.reg` import key-shape whitelist.
    #[test]
    fn reg_import_allows_uninstall_and_run_values() {
        let uninstall = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\DemoApp]\r\n\
            \"DisplayName\"=\"Demo\"\r\n";
        assert!(super::reg_content_allowed(uninstall).is_ok());

        let run_value = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n\
            \"Demo\"=\"C:\\\\Tools\\\\demo.exe\"\r\n";
        assert!(super::reg_content_allowed(run_value).is_ok());
    }

    #[test]
    fn reg_import_rejects_protected_shapes() {
        // Bare Run key with no values must not import.
        let run_empty = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n";
        assert!(super::reg_content_allowed(run_empty).is_err());

        // Key deletion is never restorable via import whitelist.
        let del = "Windows Registry Editor Version 5.00\r\n\r\n\
            [-HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Demo]\r\n";
        assert!(super::reg_content_allowed(del).is_err());

        // Unrelated / protected tree.
        let evil = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\WinDefend]\r\n\
            \"Start\"=dword:00000004\r\n";
        assert!(super::reg_content_allowed(evil).is_err());

        // Missing header.
        let no_hdr = "[HKEY_CURRENT_USER\\Software\\Demo]\r\n\"A\"=\"B\"\r\n";
        assert!(super::reg_content_allowed(no_hdr).is_err());
    }

    // S-R7-05: empty scopes must not silently write Machine PATH.
    #[test]
    fn restore_path_entry_empty_scopes_writes_user_only() {
        let _lock = crate::regops::path_mock::lock_mock();
        crate::regops::path_mock::install(r"C:\Vendor\Tool;C:\Other", r"C:\Windows\System32");
        let changed = crate::regops::restore_path_entry(r"C:\Vendor\Tool", &[]).unwrap();
        // Entry already in User → no change; Machine must remain untouched.
        assert!(!changed);
        assert_eq!(
            crate::regops::path_mock::get("Machine"),
            r"C:\Windows\System32"
        );

        crate::regops::path_mock::install(r"C:\Other", r"C:\Windows\System32");
        let changed = crate::regops::restore_path_entry(r"C:\Vendor\Tool", &[]).unwrap();
        assert!(changed);
        assert!(crate::regops::path_mock::get("User").contains(r"C:\Vendor\Tool"));
        assert_eq!(
            crate::regops::path_mock::get("Machine"),
            r"C:\Windows\System32",
            "empty scopes must never write Machine"
        );
        crate::regops::path_mock::clear();
    }

    // S-R7-01 / S-R7-02 adversarial: tampered path_map must not write system dirs.
    #[test]
    fn restore_refuses_tampered_path_map_system_targets() {
        let tmp = std::env::temp_dir().join(format!("remova_restore_adv_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let sess = tmp.join("sess");
        let files = sess.join("files");
        fs::create_dir_all(&files).unwrap();
        let rel = "evil.txt";
        fs::write(files.join(rel), b"pwn").unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert(rel.to_string(), r"C:\Windows\System32\evil.dll".to_string());
        fs::write(
            files.join("path_map.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        let err = restore_session(&sess).unwrap_err();
        assert!(err.contains("protected"), "got: {err}");
        assert!(!std::path::Path::new(r"C:\Windows\System32\evil.dll").exists());
        let _ = fs::remove_dir_all(&tmp);
    }
}
