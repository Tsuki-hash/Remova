//! File / registry backup before cleanup.

use crate::scanner::{CleanupItem, ItemKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// tmp + fsync + rename via the shared helper — a crash mid-write must not
/// leave a half map/seal, and the tmp is pid/nonce-unique across processes.
fn write_bytes_atomic(p: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::fsutil::write_bytes_atomic(p, bytes).map_err(|e| e.to_string())
}

fn export_with_view(raw: &str, view: &str) -> Result<String, String> {
    let (header, body) = raw
        .split_once('\n')
        .ok_or("missing registry export header")?;
    if header.trim_end_matches('\r') != "Windows Registry Editor Version 5.00" {
        return Err("invalid registry export header".into());
    }
    Ok(format!(
        "{header}\n; Remova registry view: {view}\r\n{body}"
    ))
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
/// suites cannot steal each other's backup root.
///
/// This used to be its own `static Mutex`, separate from the one `test_support`
/// hands out — the exact defect M-03 describes: a guard protects a variable only
/// if every writer takes the *same* lock. `backup.rs`, `restore.rs`,
/// `pipeline_smoke.rs` and `commands/backup_cmd.rs` all write `REMOVA_BACKUP_DIR`,
/// so once one took the shared lock while the others took this one the suites
/// raced (`session_list_distinguishes_missing_empty_and_read_failure` passed alone
/// and failed in the full run). One lock, one variable.
#[cfg(test)]
pub(crate) fn lock_backup_env() -> crate::test_support::EnvLock {
    crate::test_support::EnvLock::new()
}

/// Apply a private DACL to the backup tree: SYSTEM, Administrators and the
/// directory owner (the invoking user) only. `%ProgramData%` inheritance grants
/// `BUILTIN\Users` read access, which exposed copied files and registry exports
/// to other local accounts.
#[cfg(windows)]
fn current_user_sid() -> Option<String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        if len == 0 {
            let _ = CloseHandle(token);
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr() as *mut _),
            len,
            &mut len,
        );
        let _ = CloseHandle(token);
        ok.ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        sid_to_string(user.User.Sid)
    }
}

#[cfg(windows)]
fn sid_to_string(sid: windows::Win32::Security::PSID) -> Option<String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    let mut raw = windows::core::PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut raw).ok()?;
        let text = raw.to_string().ok();
        let _ = LocalFree(HLOCAL(raw.0.cast()));
        text
    }
}

#[cfg(test)]
fn apply_private_acl(path: &Path) -> std::io::Result<()> {
    let sid = current_user_sid_for_acl()?;
    apply_private_acl_with_sid(path, &sid)
}

#[cfg(windows)]
fn apply_private_acl_with_sid(path: &Path, sid: &str) -> std::io::Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows::Win32::Security::{
        SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR,
    };

    let user_ace = format!("(A;OICI;FA;;;{sid})");
    let sddl = format!("O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA){user_ace}");
    let sddl_wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_wide.as_ptr()),
            1,
            &mut sd,
            None,
        )
    }
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    let wide = crate::fsutil::wide_path_for_security(path);
    let ok = unsafe {
        SetFileSecurityW(
            PCWSTR(wide.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            sd,
        )
    };
    let err = std::io::Error::last_os_error();
    unsafe {
        let _ = LocalFree(HLOCAL(sd.0.cast()));
    }
    if ok.as_bool() {
        Ok(())
    } else {
        Err(err)
    }
}

#[cfg(not(windows))]
fn apply_private_acl_with_sid(_path: &Path, _sid: &str) -> std::io::Result<()> {
    Ok(())
}

const SECURE_TREE_BUDGET: u32 = 1_000_000;

fn secure_tree(path: &Path, depth: u8, budget: &mut u32) -> std::io::Result<()> {
    let sid = current_user_sid_for_acl()?;
    secure_tree_with_sid(path, depth, budget, &sid)
}

fn secure_tree_with_sid(
    path: &Path,
    depth: u8,
    budget: &mut u32,
    sid: &str,
) -> std::io::Result<()> {
    if *budget == 0 {
        return Err(std::io::Error::other("backup ACL walk budget exhausted"));
    }
    if depth > 64 {
        return Err(std::io::Error::other("backup ACL walk depth exceeded"));
    }
    *budget -= 1;
    // A root that does not exist yet has nothing to protect and is not an
    // error: `create_session` secures the root before the first session exists.
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    // Check reparse BEFORE writing the DACL. SetFileSecurityW follows a
    // junction, so securing first would rewrite the ACL of its target (for
    // example C:\Windows) instead of the backup entry.
    if crate::fsutil::is_reparse_point(path) {
        if depth == 0 {
            return Err(std::io::Error::other("backup ACL root is a reparse point"));
        }
        return Ok(());
    }
    apply_private_acl_with_sid(path, sid)?;
    if !meta.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        secure_tree_with_sid(&entry?.path(), depth + 1, budget, sid)?;
    }
    Ok(())
}

fn current_user_sid_for_acl() -> std::io::Result<String> {
    #[cfg(windows)]
    {
        current_user_sid()
            .ok_or_else(|| std::io::Error::other("backup current user SID unavailable"))
    }
    #[cfg(not(windows))]
    {
        Ok(String::new())
    }
}

/// Apply the same private DACL to one file outside the backup tree (history).
/// Best-effort callers may log a failure, but a file created under a shared
/// ProgramData directory should not stay readable by every local account.
pub(crate) fn secure_private_path(path: &Path) -> std::io::Result<()> {
    let sid = current_user_sid_for_acl()?;
    apply_private_acl_with_sid(path, &sid)
}

/// Best-effort hardening for listing/preview paths. A session created by
/// another account, or an unreadable historical entry, must not make every new
/// backup fail; strict failures apply only to the session we just created.
fn secure_tree_best_effort(path: &Path, depth: u8, budget: &mut u32, sid: &str) {
    if *budget == 0 || depth > 64 {
        return;
    }
    *budget -= 1;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return,
    };
    if crate::fsutil::is_reparse_point(path) {
        return;
    }
    if apply_private_acl_with_sid(path, sid).is_err() {
        return;
    }
    if !meta.is_dir() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            secure_tree_best_effort(&entry.path(), depth + 1, budget, sid);
        }
    }
}

/// Secure pre-existing backups without turning another account's unreadable
/// session into a hard backup failure. The current session is secured strictly
/// by `create_session`.
pub(crate) fn ensure_backup_tree_secured(root: &Path) -> std::io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    if crate::fsutil::is_reparse_point(root) {
        return Err(std::io::Error::other("backup root is a reparse point"));
    }
    let sid = current_user_sid_for_acl()?;
    let mut budget = SECURE_TREE_BUDGET;
    secure_tree_best_effort(root, 0, &mut budget, &sid);
    Ok(())
}

/// Secure the backup root itself once per process. The lock is released before
/// the walk so concurrent backups do not serialize on filesystem I/O.
fn secure_root_once(root: &Path) -> std::io::Result<()> {
    static SECURED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>,
    > = std::sync::OnceLock::new();
    let seen = SECURED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
    if seen.lock().map(|g| g.contains(root)).unwrap_or(false) {
        return Ok(());
    }
    if crate::fsutil::is_reparse_point(root) {
        return Err(std::io::Error::other("backup root is a reparse point"));
    }
    let sid = current_user_sid_for_acl()?;
    apply_private_acl_with_sid(root, &sid)?;
    if let Ok(mut g) = seen.lock() {
        g.insert(root.to_path_buf());
    }
    Ok(())
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
    if crate::fsutil::is_reparse_point(&root) {
        return Err(std::io::Error::other("backup root is a reparse point"));
    }
    // The shared root may pre-date this account and refuse an ACL rewrite. The
    // session we create below is secured strictly, so a root ACL failure must
    // not block every new backup; log it and continue.
    if let Err(e) = secure_root_once(&root) {
        eprintln!("backup root ACL could not be applied: {e}");
    }
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
    // Strictly secure only the session we just created. Historical sessions
    // belong to this or another account and are hardened best-effort at list
    // time; one unreadable old session must not block every new backup.
    let mut budget = SECURE_TREE_BUDGET;
    secure_tree(&dir, 0, &mut budget)?;
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

/// The `files/`-relative backup name for one fs item.
///
/// F-13: a 255-character file name plus the digest prefix exceeded the Windows
/// component limit and aborted the whole cleanup. The limit counts UTF-16 code
/// units, so the budget is spent in units (regression fix): the 16-hex digest plus `_`
/// leave 238 for the name, and the truncation always lands on a char boundary.
fn backup_rel_name(src: &Path, original: &str) -> String {
    let digest = format!("{:x}", crate::fsutil::fnv1a64(original));
    let lossy = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "item".into());
    let mut name = String::new();
    let mut units = 0usize;
    for ch in lossy.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 238 {
            break;
        }
        name.push(ch);
        units += ch_units;
    }
    format!("{digest}_{name}")
}

fn safe_name(path: &str) -> String {
    let base = path
        .replace(['\\', '/'], "__")
        .replace(':', "")
        .replace(['*', '?', '"', '<', '>', '|'], "_");
    let name: String = base.chars().take(180).collect();
    // `\`→`__` and `:` deletion are inherently lossy (a literal `C__A__B`
    // key folds the same), so every backup directory carries a digest
    // suffix — two distinct registry keys can never share one directory.
    format!(
        "{}_{}",
        name.trim_end_matches(['.', ' ']),
        crate::fsutil::fnv1a64(path)
    )
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
    let user = crate::regops::read_path_scope_public("User")
        .map_err(|e| crate::error::backup_path_err(e).to_ipc())?;
    let machine = crate::regops::read_path_scope_public("Machine")
        .map_err(|e| crate::error::backup_path_err(e).to_ipc())?;
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
    // Atomic: a crash mid-backup must not leave a torn meta line.
    write_bytes_atomic(&meta, entry.as_bytes())?;

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
        // Atomic: path.json is seal-digested — a torn write would poison the
        // whole session on restore.
        write_bytes_atomic(
            &pj,
            serde_json::to_string_pretty(&snap)
                .unwrap_or_default()
                .as_bytes(),
        )?;
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

/// Publish the verified private stage into the session. A same-volume rename
/// avoids rewriting every backed-up byte; a copy is the fallback for filesystems
/// or antivirus locks that refuse the move. Either way the seal is written only
/// after the complete tree is in place.
fn publish_stage(private: &Path, session: &Path) -> Result<(), String> {
    for sub in ["files", "registry"] {
        let _ = fs::remove_dir(session.join(sub));
    }
    let entries = fs::read_dir(private).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let src = entry.path();
        let dest = session.join(entry.file_name());
        if fs::rename(&src, &dest).is_ok() {
            continue;
        }
        let meta = fs::symlink_metadata(&src).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            crate::fsutil::copy_dir(&src, &dest).map_err(|e| e.to_string())?;
        } else {
            crate::fsutil::copy_file_no_reparse(&src, &dest).map_err(|e| e.to_string())?;
        }
    }
    // Moved files keep the private stage's SYSTEM/Administrators-only DACL;
    // grant the invoking owner access so restore can read them.
    let mut budget = SECURE_TREE_BUDGET;
    secure_tree(session, 0, &mut budget).map_err(|e| format!("backup ACL securing failed: {e}"))?;
    Ok(())
}

fn backup_batch(items: &[CleanupItem], session: &Path) -> Result<(u32, u32, Vec<String>), String> {
    if crate::executor::cleanup_cancelled() {
        return Err("cleanup cancelled during backup".into());
    }
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
        if crate::executor::cleanup_cancelled() {
            return Err("cleanup cancelled during backup".into());
        }
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
    publish_stage(private, session)?;
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
            // The view belongs inside the authenticated export bytes; an
            // unsigned sidecar could redirect a sealed backup to another hive view.
            let raw = crate::restore::decode_reg_text(fs::read(&dest).map_err(|e| e.to_string())?)?;
            write_bytes_atomic(
                &dest,
                export_with_view(&raw, reg_view_flag(export_path).trim_start_matches("/reg:"))?
                    .as_bytes(),
            )?;
            // Record the specific value name for Run items so restore knows what was targeted.
            if let Some((key, vname)) = item.path.split_once('|') {
                let dir = session.join("registry").join(safe_name(&item.path));
                let meta = dir.join("value.txt");
                write_bytes_atomic(&meta, item.path.as_bytes())
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
            let rel = backup_rel_name(src, &item.path);
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

    /// A1: the truncated name must survive the real backup path, including the
    /// atomic temp file. A 238-unit ASCII name (rel = 255 units) and a
    /// 115-emoji name both used to fail at the temp component.
    #[test]
    fn long_names_backup_end_to_end() {
        use crate::scanner::ItemKind;
        for (label, name) in [
            ("ascii238", "a".repeat(238)),
            ("emoji115", "\u{1F980}".repeat(115)),
        ] {
            let root = crate::test_support::TempDir::new(&format!("bk_long_{label}"));
            let src = root.join(&name);
            std::fs::write(&src, b"payload").unwrap();
            let _store = crate::path_seal::TestStore::new(root.join("seals"));
            let session = root.join("sess");
            std::fs::create_dir_all(session.join("files")).unwrap();
            let item = CleanupItem {
                path: src.to_string_lossy().into_owned(),
                kind: ItemKind::File,
                score: 90,
                confidence: Confidence::Confirmed,
                risk: RiskLevel::Low,
                reason: "t".into(),
                evidence: vec![],
                shared: false,
                service_bound: false,
                user_data: false,
                user_library: false,
                size_kb: None,
                size_capped: false,
                bucket: None,
            };
            backup_item(&item, &session).unwrap_or_else(|e| panic!("{label} must back up: {e}"));
            let files: Vec<_> = std::fs::read_dir(session.join("files"))
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name() != "path_map.json")
                .collect();
            assert_eq!(files.len(), 1, "{label}");
            assert!(
                files[0]
                    .file_name()
                    .to_string_lossy()
                    .encode_utf16()
                    .count()
                    <= 255,
                "{label}: backup component must fit the limit"
            );
        }
    }

    /// regression fix: the component limit counts UTF-16 units. 130 astral chars are
    /// 260 units — a `chars().take(180)` budget kept them all and pushed the
    /// digest-prefixed name past 255, aborting the whole cleanup again.
    #[test]
    fn backup_name_budget_is_spent_in_utf16_units() {
        let astral = "\u{1F980}".repeat(130);
        let rel = backup_rel_name(Path::new(&format!(r"C:\t\{astral}")), r"C:\t\name");
        assert!(
            rel.encode_utf16().count() <= 255,
            "rel must fit the component limit, got {} units",
            rel.encode_utf16().count()
        );
        // The truncation is why it fits: the name itself stopped at 238 units.
        assert_eq!(rel.encode_utf16().count(), 17 + 238);
        // A plain ASCII name fits without truncation.
        let plain = backup_rel_name(Path::new(r"C:\t\setup.exe"), r"C:\t\setup.exe");
        assert_eq!(
            plain,
            format!("{:x}_setup.exe", crate::fsutil::fnv1a64(r"C:\t\setup.exe"))
        );
    }

    #[test]
    #[cfg(windows)]
    fn view_annotation_keeps_native_import_header_valid() {
        // No keys or values: native format acceptance without registry writes.
        let raw = "Windows Registry Editor Version 5.00\r\n\r\n";
        let path = std::env::temp_dir().join(format!(
            "remova-header-{}.reg",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let text = export_with_view(raw, "32").unwrap();
        assert_eq!(
            text.lines().next(),
            Some("Windows Registry Editor Version 5.00")
        );
        let bytes: Vec<u8> = std::iter::once(0xfeffu16)
            .chain(text.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        fs::write(&path, bytes).unwrap();
        let result = Command::new(crate::regops::sys_tool("reg.exe"))
            .args(["import", path.to_str().unwrap(), "/reg:32"])
            .output();
        fs::remove_file(&path).unwrap();
        assert!(result.unwrap().status.success());
        assert!(export_with_view("; comment\nWindows Registry Editor Version 5.00", "32").is_err());
    }

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
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        };
        let file_item = item.clone();
        item.kind = ItemKind::Path;
        item.path = r"C:\Vendor\Tool".into();
        crate::regops::path_mock::set_fail_scope_read(Some("Machine"));
        let (ok, fail, errors) = backup_items(std::slice::from_ref(&item), &session);
        assert_eq!((ok, fail), (0, 1), "{errors:?}");
        assert!(!session.join("path.json").exists());
        assert!(crate::path_seal::verified_seal(&session).is_err());
        assert!(crate::regops::path_mock::get("User").contains(r"C:\Vendor\Tool"));
        crate::regops::path_mock::set_fail_scope_read(None);
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
        // Digest-suffixed form is deterministic for the same input.
        assert_eq!(
            safe_name(r"HKCU\SOFTWARE\A"),
            format!(
                "HKCU__SOFTWARE__A_{}",
                crate::fsutil::fnv1a64(r"HKCU\SOFTWARE\A")
            )
        );
        assert_eq!(safe_name(r"HKCU\SOFTWARE\A"), safe_name(r"HKCU\SOFTWARE\A"));
    }

    #[test]
    fn safe_name_disambiguates_lossy_folds() {
        // The `\`→`__` and `:` folds must not collide with literal keys.
        let folded = safe_name(r"HKCU\SOFTWARE\A B");
        let literal = safe_name(r"HKCU\SOFTWARE\A__B");
        assert_ne!(
            folded, literal,
            "space fold must not collide with literal __"
        );
        let colon_folded = safe_name(r"C:\Tools");
        let colon_literal = safe_name(r"C__Tools");
        assert_ne!(
            colon_folded, colon_literal,
            "colon deletion must not collide"
        );
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
        // Self-restoring: a bare remove_var used to delete whatever override
        // another test had installed and never put it back, so the next test saw
        // the real ProgramData root.
        let _clear = crate::test_support::EnvVar::remove("REMOVA_BACKUP_DIR");
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
        let _set = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", &root);
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
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
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
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
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
        // The override restores itself on drop; a bare remove_var here destroyed
        // whatever another test had installed.
        let _ = fs::remove_dir_all(&root);
    }

    /// Session dir must be claimed with create_dir (no exists-then-reuse race).
    #[test]
    fn create_session_never_reuses_foreign_directory() {
        let _guard = lock_backup_env();
        let root = std::env::temp_dir().join(format!("remova_sess_claim_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _set = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", &root);
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
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_missing_file_ok() {
        let tmp = std::env::temp_dir().join(format!("remova_bk_test_{}", std::process::id()));
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
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        };
        assert!(backup_item(&item, &tmp).is_ok());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cancelled_backup_does_not_start_or_publish_any_item() {
        let tmp = crate::test_support::TempDir::new("backup-cancel");
        let session = tmp.path().join("session");
        fs::create_dir_all(session.join("files")).unwrap();
        fs::create_dir_all(session.join("registry")).unwrap();
        let source = tmp.path().join("source.bin");
        fs::write(&source, b"payload").unwrap();
        let item = CleanupItem {
            path: source.to_string_lossy().to_string(),
            kind: ItemKind::File,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        };
        crate::executor::begin_cleanup_control();
        crate::executor::cancel_cleanup_operation();
        let (ok, fail, errors) = backup_items(std::slice::from_ref(&item), &session);
        assert_eq!(ok, 0);
        assert!(fail > 0);
        assert!(errors.iter().any(|e| e.contains("cancelled")), "{errors:?}");
        assert!(!session.join("files/path_map.json").exists());
        crate::executor::begin_cleanup_control();
    }

    #[test]
    fn path_backup_writes_snapshot_not_tree_copy() {
        let _guard = lock_backup_env();
        let _path_guard = crate::regops::path_mock::lock_mock();
        struct ClearPath;
        impl Drop for ClearPath {
            fn drop(&mut self) {
                crate::regops::path_mock::clear();
            }
        }
        let _clear = ClearPath;
        crate::regops::path_mock::install("", "");
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
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
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

#[cfg(all(test, windows))]
mod acl_tests {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        GetSecurityDescriptorControl, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
    };

    /// Read the object's real DACL back and return (protected, allow SIDs).
    fn read_dacl(path: &Path) -> (bool, Vec<String>) {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut dacl: *mut ACL = std::ptr::null_mut();
            let mut sd = PSECURITY_DESCRIPTOR::default();
            GetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                &mut sd,
            )
            .ok()
            .expect("GetNamedSecurityInfoW must succeed on a fixture path");
            assert!(!dacl.is_null(), "fixture must carry an explicit DACL");

            let mut control = 0u16;
            let mut revision = 0u32;
            GetSecurityDescriptorControl(sd, &mut control, &mut revision).unwrap();
            let protected = control & SE_DACL_PROTECTED.0 != 0;

            let mut sids = Vec::new();
            for i in 0..(*dacl).AceCount {
                let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
                windows::Win32::Security::GetAce(dacl, i.into(), &mut ace).unwrap();
                let header = &*(ace as *const windows::Win32::Security::ACE_HEADER);
                if header.AceType != 0 {
                    continue;
                }
                let allow = &*(ace as *const windows::Win32::Security::ACCESS_ALLOWED_ACE);
                let sid = PSID(std::ptr::addr_of!(allow.SidStart).cast_mut().cast());
                if let Some(text) = sid_to_string(sid) {
                    sids.push(text);
                }
            }
            let _ = LocalFree(HLOCAL(sd.0.cast()));
            (protected, sids)
        }
    }

    #[test]
    fn private_acl_can_be_applied_to_a_backup_directory() {
        let tmp = crate::test_support::TempDir::new("backup-acl");
        let dir = tmp.path().join("Backup");
        std::fs::create_dir_all(dir.join("files")).unwrap();
        std::fs::write(dir.join("files").join("sample.bin"), b"x").unwrap();
        super::apply_private_acl(&dir).unwrap();
        super::apply_private_acl(&dir.join("files")).unwrap();
    }

    #[test]
    fn the_applied_acl_is_read_back_with_the_current_user_and_no_inheritance() {
        let tmp = crate::test_support::TempDir::new("backup-acl-read");
        let dir = tmp.path().join("Backup");
        std::fs::create_dir_all(dir.join("files")).unwrap();
        std::fs::write(dir.join("files").join("sample.bin"), b"payload").unwrap();
        super::apply_private_acl(&dir).unwrap();

        let (protected, sids) = read_dacl(&dir);
        assert!(protected, "the backup DACL must not inherit BUILTIN\\Users");
        let me = current_user_sid().expect("current user SID must be resolvable");
        assert!(
            sids.iter().any(|s| s == &me),
            "the invoking user must be able to read back its own backup: {sids:?}"
        );
        assert!(
            sids.iter().any(|s| s == "S-1-5-18"),
            "SYSTEM must retain access: {sids:?}"
        );
        assert!(
            sids.iter().any(|s| s == "S-1-5-32-544"),
            "Administrators must retain access: {sids:?}"
        );
        assert!(
            !sids.iter().any(|s| s == "S-1-5-32-545"),
            "BUILTIN\\Users must not be granted access: {sids:?}"
        );
    }

    #[test]
    fn a_missing_root_is_tolerated_but_a_real_failure_is_reported() {
        let tmp = crate::test_support::TempDir::new("backup-acl-fail");
        // `create_session` secures the root before the first session exists, so
        // an absent root is not an error.
        let missing = tmp.path().join("does-not-exist");
        let mut budget = 10u32;
        assert!(secure_tree(&missing, 0, &mut budget).is_ok());

        // A path that exists but cannot be re-secured must surface the error
        // rather than being silently skipped. A file with an invalid SDDL target
        // is hard to fabricate portably, so the guaranteed failure used here is
        // an exhausted budget on a real tree.
        let dir = tmp.path().join("Backup");
        std::fs::create_dir_all(&dir).unwrap();
        let mut exhausted = 0u32;
        assert!(secure_tree(&dir, 0, &mut exhausted).is_err());
    }

    #[test]
    fn an_exhausted_budget_stops_the_walk_with_an_error() {
        let tmp = crate::test_support::TempDir::new("backup-acl-budget");
        let dir = tmp.path().join("Backup");
        std::fs::create_dir_all(dir.join("files")).unwrap();
        let mut budget = 0u32;
        assert!(secure_tree(&dir, 0, &mut budget).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_child_is_never_followed_by_the_acl_walk() {
        let tmp = crate::test_support::TempDir::new("backup-acl-junction");
        let root = tmp.path().join("Backup");
        let target = tmp.path().join("outside-target");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.bin"), b"must not be resecured").unwrap();
        let link = root.join("session-link");
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success(), "junction fixture creation failed");
        assert!(crate::fsutil::is_reparse_point(&link));

        let before = read_dacl(&target);
        ensure_backup_tree_secured(&root).unwrap();
        let after = read_dacl(&target);
        assert_eq!(
            before, after,
            "the backup ACL walk must skip a junction instead of rewriting its target"
        );
        assert!(target.join("keep.bin").exists());
    }

    #[test]
    fn deep_backup_trees_are_fully_secured() {
        let tmp = crate::test_support::TempDir::new("backup-acl-deep");
        let mut dir = tmp.path().join("Backup");
        std::fs::create_dir_all(&dir).unwrap();
        // Deeper than the old depth cap, so the walk must not stop early.
        for i in 0..12 {
            dir = dir.join(format!("d{i}"));
        }
        std::fs::create_dir_all(&dir).unwrap();
        let leaf = dir.join("payload.bin");
        std::fs::write(&leaf, b"deep").unwrap();

        let root = tmp.path().join("Backup");
        ensure_backup_tree_secured(&root).unwrap();
        let (protected, sids) = read_dacl(&leaf);
        assert!(protected, "a deep payload must not keep inherited access");
        let me = current_user_sid().unwrap();
        assert!(sids.iter().any(|s| s == &me), "deep payload SIDs: {sids:?}");
    }
}
