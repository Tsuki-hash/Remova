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
        for (rel, original) in map {
            let src = files_root.join(&rel);
            if !src.exists() {
                continue;
            }
            let dest = PathBuf::from(&original);
            if src.is_dir() {
                copy_dir(&src, &dest).map_err(|e| format!("{original}: {e}"))?;
            } else {
                if let Some(p) = dest.parent() {
                    fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                fs::copy(&src, &dest).map_err(|e| e.to_string())?;
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
                Err(e) => {
                    return Err(crate::error::restore_path_err(format!(
                        "PATH restore failed for {}: {e}",
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

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    crate::fsutil::copy_dir(src, dest)
}

/// Prefer scopes recorded at backup; fall back to PATH strings in the snapshot; else both.
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
        out.push("User".into());
        out.push("Machine".into());
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
pub fn delete_session_by_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err("invalid session name".into());
    }
    let path = crate::backup::backup_root().join(name);
    if !path.is_dir() {
        return Err("session not found".into());
    }
    fs::remove_dir_all(&path).map_err(|e| e.to_string())
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
        let fresh = root.join(format!("{}_prune_test_fresh", now - 1 * 24 * 3600));
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
        assert_eq!(
            super::path_restore_scopes(&none),
            vec!["User".to_string(), "Machine".to_string()]
        );
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
}
