//! File / registry backup before cleanup.

use crate::scanner::{CleanupItem, ItemKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn backup_root() -> PathBuf {
    let pd = std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Remova").join("Backup")
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

pub fn backup_item(item: &CleanupItem, session: &Path) -> Result<(), String> {
    match item.kind {
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
                .ok_or_else(|| "bad key".to_string())?;
            let hive = match alias.to_uppercase().as_str() {
                "HKLM64" | "HKLM32" | "HKLM" => "HKLM",
                "HKCU" => "HKCU",
                _ => return Err(format!("unsupported hive {alias}")),
            };
            let out = Command::new(crate::regops::sys_tool("reg.exe"))
                .args([
                    "export",
                    &format!("{hive}\\{rest}"),
                    &dest.to_string_lossy(),
                    "/y",
                    reg_view_flag(export_path),
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!("reg export failed for {}", item.path));
            }
            // Record the specific value name for Run items so restore knows what was targeted.
            if item.path.contains('|') {
                let meta = session
                    .join("registry")
                    .join(safe_name(&item.path))
                    .join("value.txt");
                let _ = fs::write(&meta, &item.path);
            }
            Ok(())
        }
        _ => {
            let src = Path::new(&item.path);
            if !src.exists() {
                return Ok(());
            }
            let digest = format!("{:x}", fnv1a64(&item.path));
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
            // path map
            let map_path = session.join("files").join("path_map.json");
            let mut map: std::collections::BTreeMap<String, String> = fs::read_to_string(&map_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            map.insert(rel, item.path.clone());
            fs::write(
                &map_path,
                serde_json::to_string_pretty(&map).unwrap_or_default(),
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        }
    }
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
        let _ = fs::write(
            &map_path,
            serde_json::to_string_pretty(&path_map).unwrap_or_default(),
        );
    }
    (ok, fail, errors)
}

fn backup_item_with_map(
    item: &CleanupItem,
    session: &Path,
    path_map: &mut std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    match item.kind {
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
                .ok_or_else(|| "bad key".to_string())?;
            let hive = match alias.to_uppercase().as_str() {
                "HKLM64" | "HKLM32" | "HKLM" => "HKLM",
                "HKCU" => "HKCU",
                _ => return Err(format!("unsupported hive {alias}")),
            };
            let out = Command::new(crate::regops::sys_tool("reg.exe"))
                .args([
                    "export",
                    &format!("{hive}\\{rest}"),
                    &dest.to_string_lossy(),
                    "/y",
                    reg_view_flag(export_path),
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!("reg export failed for {}", item.path));
            }
            // Record the specific value name for Run items so restore knows what was targeted.
            if item.path.contains('|') {
                let meta = session
                    .join("registry")
                    .join(safe_name(&item.path))
                    .join("value.txt");
                let _ = fs::write(&meta, &item.path);
            }
            Ok(())
        }
        _ => {
            let src = Path::new(&item.path);
            if !src.exists() {
                return Ok(());
            }
            let digest = format!("{:x}", fnv1a64(&item.path));
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

fn fnv1a64(s: &str) -> u64 {
    // FNV-1a 64 — unique enough for backup names (not crypto)
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
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
        // do not create on disk in unit test — just path builder logic via create
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
        };
        assert!(backup_item(&item, &tmp).is_ok());
        let _ = fs::remove_dir_all(&tmp);
    }
}
