//! Restore from backup session.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn restore_session(session: &Path) -> Result<Vec<String>, String> {
    if !session.is_dir() {
        return Err(crate::error::restore_err("session not found").to_ipc());
    }
    // Authorize the whole session before any file, PATH or registry write.
    // Missing/legacy/invalid seals never enter an automatic restore path.
    let seal = crate::path_seal::verified_seal(session)?;
    let map_path = session.join("files/path_map.json");
    let map: std::collections::BTreeMap<String, String> = if map_path.exists() {
        serde_json::from_slice(&fs::read(&map_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else {
        std::collections::BTreeMap::new()
    };
    if !crate::path_seal::map_matches(&seal, &map) {
        return Err("seal:map_mismatch".into());
    }
    // Read and bind PATH bytes once, before restoring even a sibling file.
    let path_bytes = if session.join("path.json").exists() {
        let bytes = fs::read(session.join("path.json")).map_err(|e| e.to_string())?;
        if !crate::path_seal::snapshot_matches(&seal, "path.json", &bytes) {
            return Err("seal:path_snapshot_mismatch".into());
        }
        Some(bytes)
    } else {
        None
    };
    if seal.reg_digests.contains_key("path.json") && path_bytes.is_none() {
        return Err("seal:path_snapshot_missing".into());
    }
    let mut imports = vec![];
    let mut selected = std::collections::BTreeSet::new();
    let reg_root = session.join("registry");
    if reg_root.is_dir() {
        for entry in fs::read_dir(&reg_root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if let Some(target) = pick_import_target(&entry.path()) {
                let rel = format!(
                    "registry/{}/{}",
                    entry.file_name().to_string_lossy(),
                    target
                        .file_name()
                        .ok_or("seal:bad_export_name")?
                        .to_string_lossy()
                );
                let bytes = fs::read(&target).map_err(|e| e.to_string())?;
                if !crate::path_seal::snapshot_matches(&seal, &rel, &bytes) {
                    return Err("seal:registry_snapshot_mismatch".into());
                }
                selected.insert(rel);
                let text = validate_reg_bytes(bytes)?;
                let view = registry_import_view(&text)?;
                imports.push((entry.path(), target, text, view));
            }
        }
    }
    let recorded: std::collections::BTreeSet<_> = seal
        .reg_digests
        .keys()
        .filter(|key| key.starts_with("registry/"))
        .cloned()
        .collect();
    if selected != recorded {
        return Err("seal:registry_snapshot_missing".into());
    }
    let mut messages = vec![];

    // Files via path_map.json
    let map_path = session.join("files").join("path_map.json");
    let files_root = session.join("files");
    if map_path.exists() {
        for (rel, original) in &map {
            // Map keys are single backup names under `files/` — never path-shaped.
            if !is_safe_map_rel(rel) {
                return Err(crate::error::restore_err(format!(
                    "refusing unsafe backup entry name: {rel}"
                ))
                .to_ipc());
            }
            let src = files_root.join(rel);
            // Defense-in-depth: joined source must stay under files_root.
            if !src.starts_with(&files_root) {
                return Err(crate::error::restore_err(format!(
                    "refusing escaped backup entry: {rel}"
                ))
                .to_ipc());
            }
            if !src.exists() {
                messages.push(format!("skipped missing backup entry: {rel}"));
                continue;
            }
            let dest = PathBuf::from(&original);
            // protected / unsafe destinations skip + warn — never abort sibling entries.
            if original.trim().is_empty()
                || !crate::safety::is_safe_restore_target(&dest)
                || crate::safety::looks_like_sync_conflict(original)
            {
                messages.push(format!("skipped protected restore target: {original}"));
                continue;
            }
            // every write-back target requires a matching out-of-session seal
            // (not only library subpaths) — a tampered path_map must not widen destinations.
            if !seal
                .targets
                .contains(&crate::path_seal::normalize_target(original))
            {
                messages.push(format!("skipped unsealed restore target: {original}"));
                continue;
            }
            // refuse copy-through of junction/mount reparse points.
            if crate::fsutil::is_reparse_point(&src) {
                messages.push(format!("skipped reparse backup entry: {rel}"));
                continue;
            }
            // The string gate above judges the *requested* path. Pin the actual
            // parent and re-run the gates on its by-handle resolved location:
            // a junction planted anywhere above the target can no longer divert
            // the write into a protected or redirected tree.
            let Some(parent) = dest.parent() else {
                messages.push(format!("skipped restore target without parent: {original}"));
                continue;
            };
            let _parent_pins = match crate::fsutil::create_dirs_pinned(parent) {
                Ok(pins) => pins,
                Err(e) => {
                    messages.push(format!("restore failed for {original}: {e}"));
                    continue;
                }
            };
            let pin = match crate::fsutil::pin_dir_resolved(parent) {
                Ok(pin) => pin,
                Err(e) => {
                    messages.push(format!("restore failed for {original}: {e}"));
                    continue;
                }
            };
            if !same_dir_path(pin.final_path(), parent) {
                drop(pin);
                messages.push(format!("skipped redirected restore target: {original}"));
                continue;
            }
            if !crate::safety::is_safe_restore_target(Path::new(pin.final_path()))
                || crate::safety::looks_like_sync_conflict(pin.final_path())
            {
                drop(pin);
                messages.push(format!("skipped protected restore target: {original}"));
                continue;
            }
            // A link planted as the leaf itself must not be written through
            // (the parent pin stops renames, not pre-existing children).
            if crate::fsutil::is_reparse_point(&dest) {
                drop(pin);
                messages.push(format!("skipped reparse restore target: {original}"));
                continue;
            }
            let copied = if src.is_dir() {
                copy_dir(&src, &dest).map(|_| ())
            } else {
                crate::fsutil::copy_file_no_reparse(&src, &dest).map(|_| ())
            };
            // Hold the parent pin until the copy is done.
            drop(pin);
            if let Err(e) = copied {
                messages.push(format!("restore failed for {original}: {e}"));
                continue;
            }
            messages.push(format!("restored {original}"));
        }
    }

    // PATH leftovers (path.json) — merge missing segments back; never whole-env overwrite.
    if let Some(bytes) = path_bytes {
        let snap: crate::backup::PathSnapshot =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
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

    // Import only the exact authenticated and validated bytes captured above.
    for (entry_dir, target, raw, view) in imports {
        // Pin every staging ancestor as well as the file. A directory swap
        // must not redirect an elevated staging write or reg.exe's reopen.
        let _stage_pins = crate::fsutil::create_dirs_pinned(&entry_dir)
            .map_err(|e| crate::error::restore_reg_err(e.to_string()).to_ipc())?;
        let pinned = entry_dir.join(format!(
            "value.import.{}.{}.reg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let mut opts = fs::OpenOptions::new();
        opts.create_new(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            opts.share_mode(0x0000_0001); // FILE_SHARE_READ only
        }
        let mut pin = opts.open(&pinned).map_err(|e| e.to_string())?;
        let staged = pin.write_all(raw.as_bytes()).and_then(|_| pin.flush());
        if let Err(write_err) = staged {
            drop(pin);
            let _ = fs::remove_file(&pinned);
            return Err(crate::error::restore_reg_err(format!(
                "reg import staging failed: {write_err}"
            ))
            .to_ipc());
        }
        let mut cmd = Command::new(crate::regops::sys_tool("reg.exe"));
        cmd.args(["import", &pinned.to_string_lossy(), view]);
        crate::regops::hide_console(&mut cmd);
        let out = cmd.output();
        let import_res = match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(_) => Err(crate::error::restore_reg_err(format!(
                "reg import failed {}",
                target.display()
            ))
            .to_ipc()),
            Err(e) => Err(e.to_string()),
        };
        drop(pin);
        let _ = fs::remove_file(&pinned);
        import_res?;
        messages.push(format!("imported {}", target.display()));
    }

    Ok(messages)
}

/// Case-/slash-insensitive directory identity check (requested parent vs the
/// by-handle resolved path). Trailing separators and `\\?\` shapes normalize.
fn same_dir_path(a: &str, b: &Path) -> bool {
    // Win32 strips per-segment trailing dots/spaces when resolving,
    // and an 8.3 short-name request resolves to its LONG name by handle —
    // normalize segments and fall back to the OS long form before declaring a
    // redirect, or every `Users\RUNNER~1\...` restore is skipped.
    let norm = |s: &str| {
        s.replace('/', "\\")
            .to_lowercase()
            .trim_end_matches('\\')
            .split('\\')
            .map(|seg| seg.trim_end_matches(['.', ' ']))
            .collect::<Vec<_>>()
            .join("\\")
    };
    let Some(bs) = b.to_str() else {
        return false;
    };
    if norm(a) == norm(bs) {
        return true;
    }
    match crate::fsutil::long_path_form(b) {
        Some(long) => norm(a) == norm(&long),
        None => false,
    }
}

/// within one backup entry directory, prefer the single-value
/// `value.reg` over the whole-key `export.reg`. A directory-shaped decoy is
/// not a target (only real files restore).
pub(crate) fn pick_import_target(entry_dir: &Path) -> Option<PathBuf> {
    let value_reg = entry_dir.join("value.reg");
    if value_reg.is_file() {
        return Some(value_reg);
    }
    let export = entry_dir.join("export.reg");
    if export.is_file() {
        return Some(export);
    }
    None
}

fn validate_reg_bytes(raw: Vec<u8>) -> Result<String, String> {
    // Whole-key exports come from `reg.exe export`, which writes UTF-16LE with a
    // BOM — a plain UTF-8 read fails outright and aborted the entire restore.
    // Transcode, then validate the very text we import.
    let text = decode_reg_text(raw)?;
    reg_content_allowed(&text)?;
    Ok(text)
}

/// Accept UTF-16LE (reg.exe native), UTF-8 with BOM and plain UTF-8; anything
/// else fails closed.
pub(crate) fn decode_reg_text(raw: Vec<u8>) -> Result<String, String> {
    if raw.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&units).map_err(|e| e.to_string());
    }
    if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(raw[3..].to_vec()).map_err(|e| e.to_string());
    }
    String::from_utf8(raw).map_err(|e| e.to_string())
}

fn registry_import_view(text: &str) -> Result<&'static str, String> {
    let views: Vec<_> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("; Remova registry view: "))
        .collect();
    match views.as_slice() {
        ["32"] => Ok("/reg:32"),
        ["64"] => Ok("/reg:64"),
        [] if !text.lines().any(|line| {
            line.trim()
                .to_ascii_uppercase()
                .starts_with("[HKEY_LOCAL_MACHINE\\")
        }) =>
        {
            Ok("/reg:64")
        }
        // Older machine exports do not bind a view. Never guess and silently
        // import a 32-bit backup into the 64-bit registry.
        [] => Err("seal:legacy_manual_restore_only".into()),
        _ => Err("seal:registry_snapshot_mismatch".into()),
    }
}

/// Parse `.reg` text and enforce the key-shape whitelist.
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
            // Run/RunOnce roots are value-level restore targets only.
            current_is_run = !key_ref.contains('|') && crate::safety::is_allowed_run_key(key_ref);
            continue;
        }
        // Value / delete lines under the current key.
        if t.starts_with('-') {
            current_has_delete = true;
        } else if t.starts_with('@') || t.starts_with('"') {
            // `"Name"=-` / `@=-` are value-DELETION lines, not writes — a Run
            // root must not pass the value-write gate by carrying one.
            if is_reg_value_delete_line(t) {
                current_has_delete = true;
            } else {
                current_has_value = true;
            }
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

/// `.reg` value-deletion syntax: `@=-` (default) and `"Name"=-` (named).
/// A quoted write whose data merely ends in `=-` (`"K"="x=-"`) is NOT a delete.
fn is_reg_value_delete_line(t: &str) -> bool {
    if let Some(rest) = t.strip_prefix("@=") {
        return rest == "-";
    }
    if let Some(rest) = t.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return &rest[end + 1..] == "=-";
        }
    }
    false
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
    // a tampered `scopes` field must not unlock the system PATH —
    // Machine scope is honored only when the recorded snapshot actually
    // contained the entry in the Machine value.
    let machine_ok = crate::regops::path_contains_entry(&item.machine_path, &item.entry);
    let recorded: Vec<String> = item
        .scopes
        .iter()
        .filter(|s| !s.eq_ignore_ascii_case("Machine") || machine_ok)
        .cloned()
        .collect();
    if !recorded.is_empty() {
        return recorded;
    }
    let mut out = Vec::new();
    if crate::regops::path_contains_entry(&item.user_path, &item.entry) {
        out.push("User".into());
    }
    if crate::regops::path_contains_entry(&item.machine_path, &item.entry) {
        out.push("Machine".into());
    }
    if out.is_empty() {
        // never silently write Machine PATH when backup has no scope evidence.
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
    // Same session-name predicate as delete_session_by_name.
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
    restore_session(&path)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub size_kb: u64,
    pub created_at: String,
}

fn dir_size_kb(p: &Path) -> u64 {
    // bounded, reparse-safe walk — a huge or junction-planted session
    // must not turn the listing path into an unbounded traversal.
    crate::dirsize::walk_size_kb_limited(p).unwrap_or(0)
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
/// Session names must match `^[0-9]{8}-[0-9]{6}` (`YYYYMMDD-HHMMSS…`).
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

/// Backup map keys are opaque `{digest}_{name}` tokens written by backup — one path segment only.
fn is_safe_map_rel(rel: &str) -> bool {
    if rel.is_empty() || rel == "." || rel == ".." {
        return false;
    }
    if rel.contains("..") || rel.contains('/') || rel.contains('\\') || rel.contains(':') {
        return false;
    }
    if rel.starts_with('~') {
        return false;
    }
    true
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
    #[test]
    fn registry_import_requires_authenticated_machine_view() {
        let machine =
            "Windows Registry Editor Version 5.00\n[HKEY_LOCAL_MACHINE\\SOFTWARE\\Vendor]";
        assert!(super::registry_import_view(machine).is_err());
        for (n, expected) in [("32", "/reg:32"), ("64", "/reg:64")] {
            assert_eq!(
                super::registry_import_view(&format!("; Remova registry view: {n}\n{machine}"))
                    .unwrap(),
                expected
            );
        }
        assert!(super::registry_import_view(
            "; Remova registry view: 32\n; Remova registry view: 64"
        )
        .is_err());
        assert_eq!(
            super::registry_import_view("[HKEY_CURRENT_USER\\Software\\Vendor]").unwrap(),
            "/reg:64"
        );
    }
    use super::*;
    use std::fs;

    #[test]
    fn validate_reg_import_accepts_utf16le_export() {
        let tmp = std::env::temp_dir().join(format!("remova_reg16_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let p = tmp.join("export.reg");
        let text = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n\"RemovaTest\"=\"C:\\\\x.exe\"\r\n";
        // reg.exe native form: UTF-16LE with BOM — must decode and validate.
        let mut bytes: Vec<u8> = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
        std::fs::write(&p, &bytes).unwrap();
        let out = super::validate_reg_bytes(std::fs::read(&p).unwrap()).unwrap();
        assert!(out.starts_with("Windows Registry Editor"), "{out}");
        // UTF-8 with BOM is accepted too.
        let mut utf8bom: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
        utf8bom.extend_from_slice(text.as_bytes());
        std::fs::write(&p, &utf8bom).unwrap();
        assert!(super::validate_reg_bytes(std::fs::read(&p).unwrap()).is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn map_rel_rejects_traversal_and_absolute() {
        assert!(super::is_safe_map_rel("abc123_file.txt"));
        assert!(!super::is_safe_map_rel(""));
        assert!(!super::is_safe_map_rel(".."));
        assert!(!super::is_safe_map_rel("../evil"));
        assert!(!super::is_safe_map_rel("a/b"));
        assert!(!super::is_safe_map_rel(r"a\b"));
        assert!(!super::is_safe_map_rel(r"C:\Windows\evil"));
    }

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
        // Do not remove_var here : the guard serializes env access; the
        // process exits after the suite and no other test needs the default root.
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn restore_file_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("remova_restore_rt_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let seals = tmp.join("seals");
        fs::create_dir_all(&seals).unwrap();
        let _g = crate::path_seal::test_lock();
        crate::path_seal::set_seals_root_for_tests(Some(seals));
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
        // every write-back needs a seal (same as production backup path).
        crate::path_seal::write_seal(&sess, "", &map, &std::collections::BTreeMap::new()).unwrap();
        // restore overwrites orig from backup
        let msgs = restore_session(&sess).unwrap();
        assert!(msgs.iter().any(|m| m.contains("restored")), "{msgs:?}");
        assert_eq!(fs::read_to_string(&orig).unwrap(), "hello-backup");
        crate::path_seal::set_seals_root_for_tests(None);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// missing seal skips the entry (partial restore), never silently writes.
    #[test]
    fn restore_skips_unsealed_library_target() {
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

        // Unsealed → skip + warn (Ok), file must not appear.
        assert!(restore_session(&sess).unwrap_err().contains("seal:missing"));
        assert!(!dest.exists(), "unsealed target must not be written");

        // Sealed → restore proceeds.
        crate::path_seal::write_seal(&sess, "", &map, &std::collections::BTreeMap::new()).unwrap();
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
        // no evidence → User only.
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
        // Only `YYYYMMDD-HHMMSS…` session folders are deletable by name.
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

    // `.reg` import key-shape whitelist.
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

    /// `"Name"=-` / `@=-` are value deletions — a Run root must
    /// not pass the value-write gate by carrying one.
    #[test]
    fn reg_import_rejects_value_delete_lines_on_run_root() {
        let named = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n\
            \"Demo\"=-\r\n";
        assert!(super::reg_content_allowed(named).is_err());
        let default = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n\
            @=-\r\n";
        assert!(super::reg_content_allowed(default).is_err());
        // A quoted write whose data merely ends in `=-` stays a write.
        let lookalike = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n\
            \"Demo\"=\"x=-\"\r\n";
        assert!(super::reg_content_allowed(lookalike).is_ok());
        // Value deletions are refused on ordinary keys too.
        let plain = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Demo]\r\n\
            \"DisplayName\"=-\r\n";
        assert!(super::reg_content_allowed(plain).is_err());
    }

    /// target selection: value.reg preferred, export.reg fallback,
    /// directories never count.
    #[test]
    fn pick_import_target_prefers_value_reg() {
        let tmp = std::env::temp_dir().join(format!("remova_pick_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let dir = tmp.join("entry");
        fs::create_dir_all(&dir).unwrap();
        // Neither file → None.
        assert!(super::pick_import_target(&dir).is_none());
        // export.reg only → export.
        fs::write(dir.join("export.reg"), b"hdr").unwrap();
        assert_eq!(
            super::pick_import_target(&dir),
            Some(dir.join("export.reg"))
        );
        // Both → value.reg wins.
        fs::write(dir.join("value.reg"), b"hdr").unwrap();
        assert_eq!(super::pick_import_target(&dir), Some(dir.join("value.reg")));
        // A directory named value.reg is not a target; export.reg still is.
        fs::remove_file(dir.join("value.reg")).unwrap();
        fs::create_dir_all(dir.join("value.reg")).unwrap();
        assert_eq!(
            super::pick_import_target(&dir),
            Some(dir.join("export.reg"))
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    // empty scopes must not silently write Machine PATH.
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

    // / adversarial: tampered path_map must not write system dirs.
    // protected targets skip + warn — the session still returns Ok (partial).
    #[test]
    fn restore_skips_tampered_path_map_system_targets() {
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
        assert!(restore_session(&sess).is_err());
        assert!(!std::path::Path::new(r"C:\Windows\System32\evil.dll").exists());
        let _ = fs::remove_dir_all(&tmp);
    }
    /// a recorded Machine scope without snapshot evidence must not
    /// unlock the system PATH — it degrades to the evidence-based fallback.
    #[test]
    fn path_restore_scopes_machine_requires_evidence() {
        use crate::backup::PathSnapshotItem;
        let forged = PathSnapshotItem {
            entry: r"C:\vendor\tool".into(),
            scopes: vec!["Machine".into()],
            user_path: String::new(),
            machine_path: String::new(),
        };
        assert_eq!(
            super::path_restore_scopes(&forged),
            vec!["User".to_string()]
        );

        let evidence = PathSnapshotItem {
            entry: r"C:\vendor\tool".into(),
            scopes: vec!["Machine".into(), "User".into()],
            user_path: r"C:\vendor\tool;C:\Windows".into(),
            machine_path: r"C:\Windows;C:\vendor\tool".into(),
        };
        assert_eq!(
            super::path_restore_scopes(&evidence),
            vec!["Machine".to_string(), "User".to_string()]
        );

        let partial = PathSnapshotItem {
            entry: r"C:\vendor\tool".into(),
            scopes: vec!["Machine".into(), "User".into()],
            user_path: r"C:\vendor\tool".into(),
            machine_path: r"C:\Windows".into(),
        };
        assert_eq!(
            super::path_restore_scopes(&partial),
            vec!["User".to_string()]
        );
    }

    /// short-name requests and trailing-dot segments must not be
    /// misread as redirects. Vacuous when the volume has 8.3 generation off.
    #[cfg(windows)]
    #[test]
    fn same_dir_path_matches_short_and_normalized_forms() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!(
            "remova_LongDirName_{}_{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&base).unwrap();
        let pin = crate::fsutil::pin_dir_resolved(&base).unwrap();
        let resolved = pin.final_path().to_string();
        drop(pin);
        assert!(super::same_dir_path(&resolved, &base));
        if let Some(short) = crate::fsutil::short_path_form(&base) {
            if short != resolved {
                // Production direction: a = by-handle resolved (long), b = the
                // short-name request.
                assert!(
                    super::same_dir_path(&resolved, Path::new(&short)),
                    "resolved form must match the short-name request: {short}"
                );
            }
        } else {
            eprintln!("skip: volume has no 8.3 aliases");
        }
        assert!(super::same_dir_path(
            &format!("{}.", base.to_string_lossy()),
            &base
        ));
        let _ = fs::remove_dir_all(&base);
    }
}
