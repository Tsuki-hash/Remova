//! File / registry backup before cleanup.

use crate::scanner::{CleanupItem, ItemKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// tmp + fsync + rename — a crash mid-write must not leave a half map/seal.
fn write_bytes_atomic(p: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    let tmp = p.with_extension("remova.tmp");
    let result = (|| -> Result<(), String> {
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        fs::rename(&tmp, p).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn backup_root() -> PathBuf {
    // the override is compile-time test-only — a production
    // parent process must not relocate the backup root (the seal binding
    // assumes the default layout).
    #[cfg(test)]
    {
        if let Ok(v) = std::env::var("REMOVA_BACKUP_DIR") {
            if !v.trim().is_empty() {
                return PathBuf::from(v);
            }
        }
    }
    crate::path_seal::program_data()
        .join("Remova")
        .join("Backup")
}

/// Serializes tests that mutate process-wide `REMOVA_BACKUP_DIR` so parallel
/// suites cannot steal each other's backup root ().
#[cfg(test)]
pub(crate) fn lock_backup_env() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn create_session(app_name: &str) -> std::io::Result<PathBuf> {
    // charset must match `is_session_name` (alnum + `-` `_` only).
    let safe: String = app_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
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
    // Claim the session dir with create_dir — never exists-then-reuse (race).
    let root = backup_root();
    fs::create_dir_all(&root)?;
    let mut claimed = None;
    for n in 0..100u32 {
        let dir = if n == 0 {
            root.join(format!("{ts}_{safe}"))
        } else {
            root.join(format!("{ts}_{safe}_{n}"))
        };
        match fs::create_dir(&dir) {
            Ok(()) => {
                claimed = Some(dir);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 99 => continue,
            Err(e) => return Err(e),
        }
    }
    let dir = claimed.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "session name exhausted under backup root",
        )
    })?;
    let _pins = crate::fsutil::create_dirs_pinned(&dir.join("files"))?;
    let _pins_reg = crate::fsutil::create_dirs_pinned(&dir.join("registry"))?;
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
    let base = path
        .replace(['\\', '/'], "__")
        .replace(':', "")
        .replace(['*', '?', '"', '<', '>', '|'], "_");
    let truncated = base.chars().count() > 180;
    let name: String = base.chars().take(180).collect();
    // Any lossy fold, truncation, or Win32 trailing-dot fold needs a digest
    // so two distinct registry keys never share one backup directory.
    let lossy = path.contains(['*', '?', '"', '<', '>', '|'])
        || truncated
        || name.ends_with(['.', ' '])
        || path.ends_with(['.', ' ']);
    if lossy {
        return format!(
            "{}_{}",
            name.trim_end_matches(['.', ' ']),
            crate::fsutil::fnv1a64(path)
        );
    }
    name
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
    if matches!(item.kind, ItemKind::File | ItemKind::Dir) && !Path::new(&item.path).exists() {
        return Ok(());
    }
    let (_, fail, errors) = backup_items(std::slice::from_ref(item), session);
    if fail > 0 {
        return Err(errors.join("; "));
    }
    Ok(())
}

pub fn backup_items(items: &[CleanupItem], session: &Path) -> (u32, u32, Vec<String>) {
    match backup_batch(items, session) {
        Ok(result) => result,
        Err(e) => (0, items.len().max(1) as u32, vec![e]),
    }
}

fn backup_batch(items: &[CleanupItem], session: &Path) -> Result<(u32, u32, Vec<String>), String> {
    // One-shot sessions only. Never read and re-sign an existing public map/snapshot.
    if session.join("files/path_map.json").exists()
        || session.join("path.json").exists()
        || fs::read_dir(session.join("registry"))
            .into_iter()
            .flatten()
            .next()
            .is_some()
    {
        return Err("session snapshots pre-exist; refusing to merge".into());
    }
    #[cfg(windows)]
    let stage = crate::path_seal::backup_stage()?;
    #[cfg(windows)]
    let private = stage.path();
    #[cfg(not(windows))]
    let private: &Path = return Err("seal:windows_required".into());
    fs::create_dir_all(private.join("files")).map_err(|e| e.to_string())?;
    fs::create_dir_all(private.join("registry")).map_err(|e| e.to_string())?;
    let mut map = std::collections::BTreeMap::new();
    let (mut ok, mut fail) = (0, 0);
    let mut errors = vec![];
    for item in items {
        match backup_item_with_map(item, private, &mut map) {
            Ok(()) => ok += 1,
            Err(e) => {
                fail += 1;
                errors.push(format!("{}: {e}", item.path));
            }
        }
    }
    // All inputs are produced in the privileged private stage, never re-read
    // from the user-writable session. Capture digests before publishing.
    if fail > 0 {
        // Partial batches must not publish: half-written registry exports or
        // missing files would be sealed and later restored as if complete.
        return Err(format!(
            "backup incomplete ({fail} failed); session not published: {}",
            errors.join("; ")
        ));
    }
    #[cfg(windows)]
    stage.verify_snapshots()?;
    let digests = crate::path_seal::collect_reg_digests(private)?;
    let json = serde_json::to_string_pretty(&map).map_err(|e| e.to_string())?;
    write_bytes_atomic(&private.join("files/path_map.json"), json.as_bytes())?;
    crate::fsutil::copy_dir(private, session).map_err(|e| e.to_string())?;
    crate::path_seal::write_seal(session, &json, &map, &digests)?;
    Ok((ok, fail, errors))
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
                // value.reg must succeed so restore can be single-value (not whole key).
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
            // never copy through junction/mount reparse (backup exfil / restore write-through).
            if crate::fsutil::is_reparse_point(src) {
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
                // copy_dir pins each level (including this root) with a
                // non-following handle. A second outer pin that requests
                // DELETE without FILE_SHARE_DELETE conflicts with that open
                // (ERROR_SHARING_VIOLATION) — do not double-pin.
                copy_dir(src, &dest).map_err(|e| e.to_string())?;
            } else {
                if let Some(p) = dest.parent() {
                    fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                crate::fsutil::copy_file_no_reparse(src, &dest).map_err(|e| e.to_string())?;
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
    fn private_stage_backup_file_and_path_roundtrip_refuses_existing_snapshots() {
        let root = std::env::temp_dir().join(format!("remova_v2_backup_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let _path_lock = crate::regops::path_mock::lock_mock();
        crate::regops::path_mock::install(r"C:\Vendor\Tool;C:\Other", r"C:\Windows");
        let source = root.join("source/file.txt");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"backup bytes").unwrap();
        let session = root.join("1700000000_batch");
        fs::create_dir_all(session.join("files")).unwrap();
        fs::create_dir_all(session.join("registry")).unwrap();
        let mut item = CleanupItem {
            path: source.to_string_lossy().into_owned(),
            kind: ItemKind::File,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "test".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        };
        let file_item = item.clone();
        item.kind = ItemKind::Path;
        item.path = r"C:\Vendor\Tool".into();
        let (ok, fail, errors) = backup_items(&[file_item.clone(), item], &session);
        assert_eq!((ok, fail), (2, 0), "{errors:?}");
        let seal = crate::path_seal::verified_seal(&session).unwrap();
        assert!(seal.reg_digests.contains_key("path.json"));
        assert!(!fs::read_dir(root.join("seals"))
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("stage_")));
        fs::write(&source, b"changed after backup").unwrap();
        crate::regops::path_mock::install(r"C:\Other", r"C:\Windows");
        crate::restore::restore_session(&session).unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"backup bytes");
        assert!(crate::regops::path_mock::get("User").contains(r"C:\Vendor\Tool"));
        let before = fs::read(session.join("files/path_map.json")).unwrap();
        let (ok, fail, _) = backup_items(&[file_item], &session);
        assert_eq!((ok, fail), (0, 1));
        assert_eq!(
            before,
            fs::read(session.join("files/path_map.json")).unwrap()
        );
        crate::regops::path_mock::clear();
        fs::remove_dir_all(root).unwrap();
    }

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
    fn safe_name_disambiguates_lossy_folds() {
        let a = safe_name(r"HKCU\SOFTWARE\A*B");
        let b = safe_name(r"HKCU\SOFTWARE\A?B");
        assert_ne!(a, b, "folded distinct keys must not collide");
        let t1 = safe_name(&format!(r"HKCU\SOFTWARE\{}", "X".repeat(200)));
        let t2 = safe_name(&format!(r"HKCU\SOFTWARE\{}", "X".repeat(199) + "Y"));
        assert_ne!(t1, t2, "truncation collisions must not share a dir");
    }

    #[test]
    fn session_dir_shape() {
        let _guard = lock_backup_env();
        std::env::remove_var("REMOVA_BACKUP_DIR");
        let root = backup_root();
        assert!(!root.as_os_str().is_empty());
        assert!(
            root.to_string_lossy().replace('/', "\\").contains("Backup"),
            "backup root should land under a Backup folder: {root:?}"
        );
    }

    /// Partial batch must not publish a sealed session (failed items would
    /// ride along or leave half registry exports).
    #[test]
    fn backup_partial_failure_does_not_publish_seal() {
        let _guard = lock_backup_env();
        let root = std::env::temp_dir().join(format!("remova_partial_pub_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("REMOVA_BACKUP_DIR", &root);
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.join("src.txt");
        fs::write(&source, b"ok").unwrap();
        let good = CleanupItem {
            path: source.to_string_lossy().into_owned(),
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
        let bad = CleanupItem {
            path: "ZZZ\\not-a-real-hive".into(),
            kind: ItemKind::Registry,
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
        let session = create_session("partial").unwrap();
        let (ok, fail, errors) = backup_items(&[good, bad], &session);
        assert!(fail >= 1, "{errors:?}");
        assert_eq!(ok, 0, "nothing publishes on partial failure");
        assert!(
            !session.join("files/path_map.json").exists(),
            "map must not publish on partial failure"
        );
        assert!(crate::path_seal::verified_seal(&session).is_err());
        std::env::remove_var("REMOVA_BACKUP_DIR");
        let _ = fs::remove_dir_all(&root);
    }

    /// Session dir must be claimed with create_dir (no exists-then-reuse race).
    #[test]
    fn create_session_never_reuses_foreign_directory() {
        let _guard = lock_backup_env();
        let root = std::env::temp_dir().join(format!("remova_sess_claim_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("REMOVA_BACKUP_DIR", &root);
        // Occupy timestamp names with foreign non-empty directories (span a
        // few seconds so the test does not flake across a clock tick).
        let base_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        for ts in base_ts..base_ts.saturating_add(3) {
            for n in 0..100u32 {
                let name = if n == 0 {
                    format!("{ts}_raceapp")
                } else {
                    format!("{ts}_raceapp_{n}")
                };
                let occupied = root.join(name);
                fs::create_dir_all(&occupied).unwrap();
                fs::write(occupied.join("foreign.marker"), b"x").unwrap();
            }
        }
        let err = create_session("raceapp").unwrap_err();
        assert!(
            err.kind() == std::io::ErrorKind::AlreadyExists || err.to_string().contains("exist"),
            "{err}"
        );
        std::env::remove_var("REMOVA_BACKUP_DIR");
        let _ = fs::remove_dir_all(&root);
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
