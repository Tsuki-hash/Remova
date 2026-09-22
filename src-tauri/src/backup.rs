//! File / registry backup before cleanup.

use crate::scanner::{CleanupItem, ItemKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn backup_root() -> PathBuf {
    if let Ok(v) = std::env::var("REMOVA_BACKUP_DIR") {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    let pd = std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Remova").join("Backup")
}

/// Serializes tests that mutate process-wide `REMOVA_BACKUP_DIR` so parallel
/// suites cannot steal each other's backup root (NEW-B).
#[cfg(test)]
pub(crate) fn lock_backup_env() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn create_session(app_name: &str) -> std::io::Result<PathBuf> {
    let safe: String = app_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = backup_root().join(format!("{ts}_{safe}"));
    fs::create_dir_all(dir.join("files"))?;
    fs::create_dir_all(dir.join("registry"))?;
    Ok(dir)
}

fn reg_view_flag(key_path: &str) -> &'static str {
    let low = key_path.to_uppercase().replace('/', "\\");
    if low.starts_with("HKLM32\\") || low.contains("\\WOW6432NODE\\") {
        "/reg:32"
    } else {
        "/reg:64"
    }
}

fn safe_name(path: &str) -> String {
    path.replace(['\\', '/'], "__")
        .replace(':', "")
        .replace(['*', '?', '"', '<', '>', '|'], "_")
        .chars()
        .take(180)
        .collect()
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PathSnapshot {
    #[serde(default)]
    pub items: Vec<PathSnapshotItem>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PathSnapshotItem {
    pub entry: String,
    /// Scopes that contained the entry at backup time (User / Machine).
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub user_path: String,
    #[serde(default)]
    pub machine_path: String,
}

fn path_snapshot_file(session: &Path) -> PathBuf {
    session.join("path.json")
}

/// Snapshot a PATH leftover into the session. Never copies directories.
fn backup_path_entry(item: &CleanupItem, session: &Path) -> Result<(), String> {
    let entry = item.path.trim();
    if entry.is_empty() {
        return Err(crate::error::backup_path_err("empty path entry").to_ipc());
    }
    let user = crate::regops::read_path_scope_public("User").unwrap_or_default();
    let machine = crate::regops::read_path_scope_public("Machine").unwrap_or_default();
    let mut scopes = Vec::new();
    if crate::regops::path_contains_entry(&user, entry) {
        scopes.push("User".to_string());
    }
    if crate::regops::path_contains_entry(&machine, entry) {
        scopes.push("Machine".to_string());
    }

    let meta_dir = session.join("files").join("_path");
    fs::create_dir_all(&meta_dir).map_err(|e| e.to_string())?;
    let meta = meta_dir.join(format!("{}.txt", safe_name(entry)));
    fs::write(&meta, entry.as_bytes()).map_err(|e| e.to_string())?;

    let pj = path_snapshot_file(session);
    let mut snap: PathSnapshot = fs::read_to_string(&pj)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    // De-dupe by entry (normalized later on restore).
    if !snap
        .items
        .iter()
        .any(|i| i.entry.eq_ignore_ascii_case(entry))
    {
        snap.items.push(PathSnapshotItem {
            entry: entry.to_string(),
            scopes,
            user_path: user,
            machine_path: machine,
        });
        fs::write(&pj, serde_json::to_string_pretty(&snap).unwrap_or_default())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn backup_item(item: &CleanupItem, session: &Path) -> Result<(), String> {
    let mut map = std::collections::BTreeMap::new();
    backup_item_with_map(item, session, &mut map)?;
    // Persist map for file/dir items when called as a one-shot API.
    if !map.is_empty() {
        let map_path = session.join("files").join("path_map.json");
        let mut existing: std::collections::BTreeMap<String, String> =
            fs::read_to_string(&map_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
        existing.extend(map);
        if let Some(p) = map_path.parent() {
            let _ = fs::create_dir_all(p);
        }
        fs::write(
            &map_path,
            serde_json::to_string_pretty(&existing).unwrap_or_default(),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn backup_items(items: &[CleanupItem], session: &Path) -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut fail = 0u32;
    let mut errors = vec![];
    // Load path_map once; write once at end (PERF-6).
    let map_path = session.join("files").join("path_map.json");
    let mut path_map: std::collections::BTreeMap<String, String> = fs::read_to_string(&map_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for it in items {
        match backup_item_with_map(it, session, &mut path_map) {
            Ok(()) => ok += 1,
            Err(e) => {
                fail += 1;
                errors.push(format!("{}: {e}", it.path));
            }
        }
    }
    if !path_map.is_empty() {
        // BE-07: path_map write failure must abort cleanup (restore depends on it).
        if let Err(e) = fs::write(
            &map_path,
            serde_json::to_string_pretty(&path_map).unwrap_or_default(),
        ) {
            fail += 1;
            errors.push(format!("path_map write failed: {e}"));
        }
    }
    (ok, fail, errors)
}

fn backup_item_with_map(
    item: &CleanupItem,
    session: &Path,
    path_map: &mut std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    match item.kind {
        ItemKind::Path => backup_path_entry(item, session),
        ItemKind::Registry => {
            // Run/RunOnce values (`key|ValueName`): export the parent key so restore can recreate the value.
            let export_path = if let Some((parent, _val)) = item.path.split_once('|') {
                parent
            } else {
                item.path.as_str()
            };
            let dest = session
                .join("registry")
                .join(safe_name(&item.path))
                .join("export.reg");
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let (alias, rest) = export_path
                .split_once('\\')
                .ok_or_else(|| crate::error::backup_reg_err("bad key").to_ipc())?;
            let hive = match alias.to_uppercase().as_str() {
                "HKLM64" | "HKLM32" | "HKLM" => "HKLM",
                "HKCU" => "HKCU",
                _ => {
                    return Err(
                        crate::error::backup_reg_err(format!("unsupported hive {alias}")).to_ipc(),
                    )
                }
            };
            let mut cmd = Command::new(crate::regops::sys_tool("reg.exe"));
            cmd.args([
                "export",
                &format!("{hive}\\{rest}"),
                &dest.to_string_lossy(),
                "/y",
                reg_view_flag(export_path),
            ]);
            crate::regops::hide_console(&mut cmd);
            let out = cmd.output().map_err(|e| {
                crate::error::backup_reg_err(format!("reg export failed for {}: {e}", item.path))
                    .to_ipc()
            })?;
            if !out.status.success() {
                return Err(crate::error::backup_reg_err(format!(
                    "reg export failed for {}",
                    item.path
                ))
                .to_ipc());
            }
            // Record the specific value name for Run items so restore knows what was targeted.
            if let Some((key, vname)) = item.path.split_once('|') {
                let dir = session.join("registry").join(safe_name(&item.path));
                let meta = dir.join("value.txt");
                fs::write(&meta, &item.path)
                    .map_err(|e| crate::error::backup_reg_err(e.to_string()).to_ipc())?;
                // S-04: value.reg must succeed so restore can be single-value (not whole key).
                match crate::regops::export_reg_value(key, vname, &dir.join("value.reg")) {
                    Ok(true) => {}
                    Ok(false) => {
                        return Err(crate::error::backup_value_reg_err(format!(
                            "value.reg export missing for {}",
                            item.path
                        ))
                        .to_ipc());
                    }
                    Err(e) => {
                        return Err(crate::error::backup_value_reg_err(format!(
                            "value.reg export failed: {e}"
                        ))
                        .to_ipc())
                    }
                }
            }
            Ok(())
        }
        ItemKind::File | ItemKind::Dir => {
            let src = Path::new(&item.path);
            if !src.exists() {
                return Ok(());
            }
            let digest = format!("{:x}", crate::fsutil::fnv1a64(&item.path));
            let name = src
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "item".into());
            let rel = format!("{digest}_{name}");
            let dest = session.join("files").join(&rel);
            if src.is_dir() {
                copy_dir(src, &dest).map_err(|e| e.to_string())?;
            } else {
                if let Some(p) = dest.parent() {
                    fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                fs::copy(src, &dest).map_err(|e| e.to_string())?;
            }
            path_map.insert(rel, item.path.clone());
            Ok(())
        }
    }
}

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    crate::fsutil::copy_dir(src, dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{Confidence, RiskLevel};

    #[test]
    fn reg_view_flags() {
        assert_eq!(reg_view_flag(r"HKLM32\SOFTWARE\Foo"), "/reg:32");
        assert_eq!(reg_view_flag(r"HKLM\SOFTWARE\WOW6432Node\Foo"), "/reg:32");
        assert_eq!(reg_view_flag(r"HKCU\SOFTWARE\Foo"), "/reg:64");
    }

    #[test]
    fn safe_name_stable() {
        assert_eq!(safe_name(r"HKCU\SOFTWARE\A"), "HKCU__SOFTWARE__A");
    }

    #[test]
    fn session_dir_shape() {
        // do not create on disk in unit test 鈥?just path builder logic via create
        let _ = backup_root();
    }

    #[test]
    fn backup_missing_file_ok() {
        let tmp = std::env::temp_dir().join("remova_bk_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let item = CleanupItem {
            path: tmp.join("no_such_file_xyz").to_string_lossy().to_string(),
            kind: ItemKind::File,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        };
        assert!(backup_item(&item, &tmp).is_ok());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn path_backup_writes_snapshot_not_tree_copy() {
        let _guard = lock_backup_env();
        let tmp = std::env::temp_dir().join(format!("remova_path_bk_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("files")).unwrap();
        fs::create_dir_all(tmp.join("registry")).unwrap();

        // A PATH-shaped leftover that is also an existing directory (must NOT be tree-copied).
        let path_dir = tmp.join("some_path_dir");
        fs::create_dir_all(path_dir.join("nested")).unwrap();
        fs::write(path_dir.join("nested/f.bin"), b"x").unwrap();

        let item = CleanupItem {
            path: path_dir.to_string_lossy().to_string(),
            kind: ItemKind::Path,
            score: 40,
            confidence: Confidence::Suspected,
            risk: RiskLevel::Medium,
            reason: "path".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        };
        let mut map = std::collections::BTreeMap::new();
        backup_item_with_map(&item, &tmp, &mut map).unwrap();
        assert!(map.is_empty(), "PATH kind must not enter path_map");
        let pj = tmp.join("path.json");
        assert!(pj.exists(), "path.json snapshot missing");
        let raw = fs::read_to_string(&pj).unwrap();
        let snap: PathSnapshot = serde_json::from_str(&raw).unwrap();
        assert_eq!(snap.items.len(), 1);
        assert_eq!(snap.items[0].entry, item.path);
        // No recursive copy of the directory into files/
        let files = tmp.join("files");
        let copied: Vec<_> = fs::read_dir(&files)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            !copied.iter().any(|n| n.contains("nested")),
            "PATH backup must not tree-copy directory contents: {copied:?}"
        );
        let _ = fs::remove_dir_all(&tmp);
    }
}
