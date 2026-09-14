//! Restore from backup session.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn restore_session(session: &Path) -> Result<Vec<String>, String> {
    if !session.is_dir() {
        return Err("session not found".into());
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

    // Registry exports
    let reg_root = session.join("registry");
    if reg_root.is_dir() {
        for e in fs::read_dir(&reg_root).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            let export = e.path().join("export.reg");
            if export.exists() {
                let out = Command::new("reg")
                    .args(["import", &export.to_string_lossy()])
                    .output()
                    .map_err(|e| e.to_string())?;
                if out.status.success() {
                    messages.push(format!("imported {}", export.display()));
                } else {
                    return Err(format!("reg import failed {}", export.display()));
                }
            }
        }
    }

    Ok(messages)
}

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let t = e.file_type()?;
        let to = dest.join(e.file_name());
        if t.is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
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

/// List backup sessions with size (KB) and folder name as created-at hint.
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
}
