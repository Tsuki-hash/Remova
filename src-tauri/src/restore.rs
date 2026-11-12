//! Restore from backup session.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
mod payload;
#[cfg(windows)]
mod publish;

mod registry_budget;
pub(crate) use registry_budget::RegistryRestoreBudget;
mod session_metadata;

struct RestoreInputs {
    seal: crate::path_seal::PathMapSeal,
    #[cfg(windows)]
    inventory: Option<payload::Inventory>,
    map: std::collections::BTreeMap<String, String>,
    path_snapshot: Option<crate::backup::PathSnapshot>,
    imports: Vec<(PathBuf, PathBuf, String, &'static str)>,
}

fn validate_session(session: &Path, cancelled: impl Fn() -> bool) -> Result<RestoreInputs, String> {
    if cancelled() {
        return Err("restore cancelled before any changes".into());
    }
    if !session.is_dir() {
        return Err(crate::error::restore_err("session not found").to_ipc());
    }
    // Authorize the whole session before any file, PATH or registry write.
    // Missing/legacy/invalid seals never enter an automatic restore path.
    let seal = crate::path_seal::verified_seal(session)?;
    session_metadata::check_session_metadata(session, &seal, &cancelled)?;
    let complete_manifest = seal.file_manifest_digest.is_some();
    #[cfg(windows)]
    let inventory = payload::Inventory::read(session, &seal, &cancelled)?;
    if cancelled() {
        return Err("restore cancelled before any changes".into());
    }
    let map_path = session.join("files/path_map.json");
    let map: std::collections::BTreeMap<String, String> = if map_path.exists() {
        let (bytes, _) =
            crate::backup::read_backup_metadata(&map_path, "seal:path_map_too_large", &cancelled)?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?
    } else {
        std::collections::BTreeMap::new()
    };
    if !crate::path_seal::map_matches(&seal, &map) {
        return Err("seal:map_mismatch".into());
    }
    // Read and bind PATH bytes once, before restoring even a sibling file.
    let path = session.join("path.json");
    let path_recorded = seal.reg_digests.contains_key("path.json");
    let path_bytes = if path_recorded || (!complete_manifest && path.exists()) {
        let (bytes, _) =
            crate::backup::read_backup_metadata(&path, "seal:path_snapshot_too_large", &cancelled)?;
        if !crate::path_seal::snapshot_matches(&seal, "path.json", &bytes) {
            return Err("seal:path_snapshot_mismatch".into());
        }
        Some(bytes)
    } else {
        None
    };
    let path_snapshot = path_bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|error| format!("seal:path_snapshot_json: {error}"))?;
    let mut imports = vec![];
    // Every registry export is held in memory until the whole batch validates.
    // A bounded total keeps a bloated session from pinning memory inside the
    // cleanup lock; exceeding it fails closed rather than restoring partially.
    let mut registry_budget = RegistryRestoreBudget::default();
    let recorded: std::collections::BTreeSet<_> = seal
        .reg_digests
        .keys()
        .filter(|key| key.starts_with("registry/"))
        .cloned()
        .collect();
    // A complete inventory is the restore allowlist. Unrecorded exports can
    // neither authorize imports nor make the recorded contents unrestorable.
    let selected = if complete_manifest {
        recorded.clone()
    } else {
        let mut selected = std::collections::BTreeSet::new();
        let root = session.join("registry");
        if root.is_dir() {
            for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                if let Some(target) = pick_import_target(&entry.path()) {
                    let filename = target.file_name().ok_or("seal:bad_export_name")?;
                    selected.insert(format!(
                        "registry/{}/{}",
                        entry.file_name().to_string_lossy(),
                        filename.to_string_lossy()
                    ));
                }
            }
        }
        if !selected.is_subset(&recorded) {
            return Err("seal:registry_snapshot_mismatch".into());
        }
        if selected != recorded {
            return Err("seal:registry_snapshot_missing".into());
        }
        selected
    };
    for relative in selected {
        let target = session.join(&relative);
        let bytes = registry_budget.read(&relative, &target, &cancelled)?;
        if !crate::path_seal::snapshot_matches(&seal, &relative, &bytes) {
            return Err("seal:registry_snapshot_mismatch".into());
        }
        let (text, view) = validated_registry_import(bytes)?;
        let entry = target.parent().ok_or("seal:bad_export_name")?.to_owned();
        imports.push((entry, target, text, view));
    }
    Ok(RestoreInputs {
        seal,
        #[cfg(windows)]
        inventory,
        map,
        path_snapshot,
        imports,
    })
}

fn restore_order(map: &std::collections::BTreeMap<String, String>) -> Vec<(&String, &String)> {
    let mut ordered: Vec<_> = map.iter().collect();
    ordered.sort_by_cached_key(|(_, original)| original.to_ascii_lowercase());
    ordered
}

pub fn restore_session(session: &Path) -> Result<Vec<String>, String> {
    restore_with_before_copy(session, |_| {})
}

fn restore_with_before_copy(
    session: &Path,
    mut before_copy: impl FnMut(&Path),
) -> Result<Vec<String>, String> {
    // Restore is a destructive transaction too: it must not interleave with a
    // cleanup, and a pending 1641 reboot must finish before files are written
    // back into a half-uninstalled product directory.
    let cancelled = || crate::executor::scoped_cleanup_cancelled().unwrap_or(false);
    let _guard = crate::executor::wait_for_cleanup_lock(cancelled)
        .ok_or("restore cancelled before any changes")?;
    if cancelled() {
        return Err("restore cancelled before any changes".into());
    }
    if crate::executor::reboot::any_pending() {
        return Err(crate::executor::reboot::PENDING_MESSAGE.to_string());
    }
    if crate::executor::scoped_cleanup_cancelled().is_none() {
        crate::executor::begin_cleanup_control();
    }
    let RestoreInputs {
        seal,
        #[cfg(windows)]
        inventory,
        map,
        path_snapshot,
        imports,
    } = validate_session(session, crate::executor::cleanup_cancelled)?;
    #[cfg(windows)]
    let authenticated = inventory
        .filter(|_| !map.is_empty())
        .map(|inventory| crate::path_seal::restore_stage().map(|stage| (inventory, stage)))
        .transpose()?;
    let mut messages = vec![];
    // Consecutive files usually share a parent. Reuse its pins instead of
    // opening the whole ancestor chain again for every file.
    let mut parent_pins: Option<(
        PathBuf,
        Vec<crate::fsutil::DirPin>,
        crate::fsutil::DirResolvedPin,
    )> = None;

    let files_root = session.join("files");
    if !map.is_empty() {
        // The backup map is keyed by a hash-shaped name, so its natural order
        // scatters files from the same destination directory. Sort by the
        // original path first so the parent-pin reuse below actually hits.
        for (rel, original) in restore_order(&map) {
            if crate::executor::cleanup_cancelled() {
                messages.push("restore cancelled".into());
                break;
            }
            // Map keys are single backup names under `files/` — never path-shaped.
            if !is_safe_map_rel(rel) {
                return Err(crate::error::restore_err(format!(
                    "refusing unsafe backup entry name: {rel}"
                ))
                .to_ipc());
            }
            let src = files_root.join(rel.as_str());
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
            // `targets` is sorted and unique, so a binary search keeps this
            // O(log n) instead of O(n) per entry (5万文件还原曾因此多花数十秒).
            if seal
                .targets
                .binary_search(&crate::path_seal::normalize_target(original))
                .is_err()
            {
                messages.push(format!("skipped unsealed restore target: {original}"));
                continue;
            }
            // refuse copy-through of junction/mount reparse points.
            if crate::fsutil::is_reparse_point(&src) {
                messages.push(format!("skipped reparse backup entry: {rel}"));
                continue;
            }
            #[cfg(windows)]
            let staged = if let Some((inventory, stage)) = &authenticated {
                match inventory.stage(stage, &src, rel) {
                    Ok(staged) => Some(staged),
                    Err(_) if crate::executor::cleanup_cancelled() => {
                        messages.push("restore cancelled".into());
                        break;
                    }
                    Err(reason) => {
                        messages.push(if reason.starts_with("skipped ") {
                            reason
                        } else {
                            format!("restore failed for {original}: {reason}")
                        });
                        continue;
                    }
                }
            } else {
                None
            };
            // The string gate above judges the *requested* path. Pin the actual
            // parent and re-run the gates on its by-handle resolved location:
            // a junction planted anywhere above the target can no longer divert
            // the write into a protected or redirected tree.
            let Some(parent) = dest.parent() else {
                messages.push(format!("skipped restore target without parent: {original}"));
                continue;
            };
            let parent_changed =
                parent_pins.as_ref().map(|(path, _, _)| path == parent) != Some(true);
            if parent_changed {
                let dirs = match crate::fsutil::create_dirs_pinned(parent) {
                    Ok(pins) => pins,
                    Err(e) => {
                        messages.push(format!("restore failed for {original}: {e}"));
                        continue;
                    }
                };
                match crate::fsutil::pin_dir_resolved(parent) {
                    Ok(resolved) => {
                        parent_pins = Some((parent.to_path_buf(), dirs, resolved));
                    }
                    Err(e) => {
                        messages.push(format!("restore failed for {original}: {e}"));
                        continue;
                    }
                }
            }
            let Some((_, _pins, pin)) = parent_pins.as_ref() else {
                messages.push(format!(
                    "skipped restore target without parent pins: {original}"
                ));
                continue;
            };
            if !same_dir_path(pin.final_path(), parent) {
                messages.push(format!("skipped redirected restore target: {original}"));
                continue;
            }
            if !crate::safety::is_safe_restore_target(Path::new(pin.final_path()))
                || crate::safety::looks_like_sync_conflict(pin.final_path())
            {
                messages.push(format!("skipped protected restore target: {original}"));
                continue;
            }
            // A link planted as the leaf itself must not be written through
            // (the parent pin stops renames, not pre-existing children).
            if crate::fsutil::is_reparse_point(&dest) {
                messages.push(format!("skipped reparse restore target: {original}"));
                continue;
            }
            // Advisory only: a late creation after this check is still caught
            // by the no-replace primitives. Directories are never merged.
            if dest.try_exists().unwrap_or(true) {
                messages.push(format!("skipped existing restore target: {original}"));
                continue;
            }
            before_copy(&src);
            #[cfg(windows)]
            {
                let (Some(parent), Some(name)) = (_pins.last(), dest.file_name()) else {
                    messages.push(format!(
                        "skipped restore target without parent pins: {original}"
                    ));
                    continue;
                };
                let source = staged
                    .as_ref()
                    .map(payload::StagedPayload::path)
                    .unwrap_or(&src);
                match publish::publish(source, parent, name) {
                    Ok(()) => messages.push(format!("restored {original}")),
                    Err(publish::Failure::Occupied) => {
                        messages.push(format!("skipped existing restore target: {original}"));
                    }
                    Err(publish::Failure::PartialDirectory(error)) => messages.push(format!(
                        "restore directory may be incomplete: {original}: {error}"
                    )),
                    Err(publish::Failure::Io(error)) => {
                        if !crate::executor::cleanup_cancelled() {
                            messages.push(format!("restore failed for {original}: {error}"));
                        }
                    }
                }
            }
            #[cfg(not(windows))]
            messages.push(format!("restore failed for {original}: Windows required"));
            if crate::executor::cleanup_cancelled() {
                messages.push("restore cancelled".into());
                break;
            }
        }
    }

    if let Some(snap) = path_snapshot {
        restore_path_snapshot(snap, &mut messages);
    }

    // Import only the exact authenticated and validated bytes captured above.
    // A failed import is aggregated and the remaining exports still run: the
    // filesystem/PATH work already happened, and aborting here used to discard
    // every earlier success message and leave the retry in the same state.
    for (entry_dir, target, raw, view) in imports {
        if crate::executor::cleanup_cancelled() {
            messages.push("restore cancelled before remaining registry imports".into());
            break;
        }
        // Pin every staging ancestor as well as the file. A directory swap
        // must not redirect an elevated staging write or reg.exe's reopen.
        let _stage_pins = match crate::fsutil::create_dirs_pinned(&entry_dir) {
            Ok(pins) => pins,
            Err(e) => {
                messages.push(format!("registry import failed {}: {e}", target.display()));
                continue;
            }
        };
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
        let mut pin = match opts.open(&pinned) {
            Ok(pin) => pin,
            Err(e) => {
                messages.push(format!("registry import failed {}: {e}", target.display()));
                continue;
            }
        };
        let staged = pin
            .write_all(&encode_reg_text(&raw))
            .and_then(|_| pin.flush());
        if let Err(write_err) = staged {
            drop(pin);
            let _ = fs::remove_file(&pinned);
            messages.push(format!(
                "registry import failed {}: reg import staging failed: {write_err}",
                target.display()
            ));
            continue;
        }
        let mut cmd = Command::new(crate::regops::sys_tool("reg.exe"));
        cmd.args(["import", &pinned.to_string_lossy(), view]);
        crate::regops::hide_console(&mut cmd);
        // A registry import can partially apply before it returns, so a timeout
        // must be reported as an incomplete import rather than retried blindly.
        let import_res = match crate::bounded_cmd::run_bounded(
            cmd,
            crate::bounded_cmd::REG_TOOL_TIMEOUT,
            crate::bounded_cmd::PROBE_OUTPUT_CAP,
        ) {
            crate::bounded_cmd::BoundedOutput::Finished { success: true, .. } => Ok(()),
            crate::bounded_cmd::BoundedOutput::Finished { .. } => Err(
                crate::error::restore_reg_err(format!(
                    "reg import failed {}",
                    target.display()
                ))
                .to_ipc(),
            ),
            crate::bounded_cmd::BoundedOutput::TimedOut => Err(crate::error::restore_reg_err(
                format!(
                    "reg import timed out {}; the registry may be partially updated, review before retrying",
                    target.display()
                ),
            )
            .to_ipc()),
            crate::bounded_cmd::BoundedOutput::Failed(e) => Err(e),
        };
        drop(pin);
        let _ = fs::remove_file(&pinned);
        match import_res {
            Ok(()) => messages.push(format!("imported {}", target.display())),
            Err(e) => messages.push(format!("registry import failed {}: {e}", target.display())),
        }
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

pub(crate) fn validated_registry_import(raw: Vec<u8>) -> Result<(String, &'static str), String> {
    let text = validate_reg_bytes(raw)?;
    let view = registry_import_view(&text)?;
    Ok((text, view))
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
        // An odd byte count cannot be a complete UTF-16LE stream. Silently
        // dropping the trailing byte would hide a truncated registry export.
        if !raw[2..].len().is_multiple_of(2) {
            return Err("registry text has a truncated UTF-16 code unit".into());
        }
        let units: Vec<u16> = raw[2..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&units).map_err(|e| e.to_string());
    }
    if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(raw[3..].to_vec()).map_err(|e| e.to_string());
    }
    String::from_utf8(raw).map_err(|e| e.to_string())
}

/// Encode `.reg` text the way `reg.exe` writes it: UTF-16LE with a BOM.
/// Without the BOM, `reg import` falls back to the ANSI code page and silently
/// corrupts non-ASCII key/value names while still reporting success.
pub(crate) fn encode_reg_text(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + text.len() * 2);
    out.extend_from_slice(&[0xFF, 0xFE]);
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
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
    let mut current_values: Vec<String> = Vec::new();

    let flush = |key: &Option<String>,
                 is_run: bool,
                 has_value: bool,
                 has_delete: bool,
                 values: &[String]|
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
        // Value-level restores on keys whose whole-key deletion is protected by
        // `is_safe_to_delete_registry`. A `value.reg` carries a bare key header,
        // so validating it like a whole-key export used to abort every session
        // that contained one MuiCache or firewall value. The value names are
        // checked against the same rules the delete gate applies.
        if value_level_restore_allowed(k, values) {
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
                &current_values,
            )?;
            current_has_value = false;
            current_has_delete = false;
            current_values.clear();
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
                if let Some(name) = reg_value_name(t) {
                    current_values.push(name);
                }
            }
        }
    }
    flush(
        &current_key,
        current_is_run,
        current_has_value,
        current_has_delete,
        &current_values,
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

/// Value name of a `.reg` write line: `@` for the default value, or the
/// quoted name. `None` for a line the parser does not recognize as a write.
fn reg_value_name(t: &str) -> Option<String> {
    if t.starts_with('@') {
        return Some("@".into());
    }
    let rest = t.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Value-level restore allowance for keys whose *whole key* must never be
/// treated as deletable. Mirrors `safety::is_safe_to_delete_registry`: a bare
/// key header is valid only when the file carries named writes and none of
/// them is a deletion.
fn value_level_restore_allowed(key: &str, values: &[String]) -> bool {
    const MUICACHE: &str =
        r"HKCU\SOFTWARE\CLASSES\LOCAL SETTINGS\SOFTWARE\MICROSOFT\WINDOWS\SHELL\MUICACHE";
    const FIREWALL_SUFFIX: &str =
        r"\SYSTEM\CURRENTCONTROLSET\SERVICES\SHAREDACCESS\PARAMETERS\FIREWALLPOLICY\FIREWALLRULES";
    if values.is_empty() || values.iter().any(|v| v.trim().is_empty() || v == "@") {
        return false;
    }
    if key.eq_ignore_ascii_case(MUICACHE) {
        return values
            .iter()
            .all(|v| v.trim().to_lowercase().ends_with(".friendlyappname"));
    }
    ["HKLM", "HKLM64", "HKLM32"].iter().any(|hive| {
        let full = format!(r"{hive}{FIREWALL_SUFFIX}");
        key.eq_ignore_ascii_case(&full)
    })
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

fn restore_path_snapshot(snapshot: crate::backup::PathSnapshot, messages: &mut Vec<String>) {
    if let Some(plan) = snapshot.cleanup_plan {
        use crate::regops::BackedPathRestore;
        messages.extend(plan.restore().into_iter().map(|result| match result {
            BackedPathRestore::Restored(scope) => format!("restored PATH scope: {scope}"),
            BackedPathRestore::AlreadyOriginal(scope) => {
                format!("PATH scope already matches backup: {scope}")
            }
            BackedPathRestore::Preserved(scope) => format!("skipped changed PATH scope: {scope}"),
            BackedPathRestore::Failed(scope, error) => {
                format!("PATH scope restore failed for {scope}: {error}")
            }
            BackedPathRestore::Cancelled => {
                "restore cancelled before remaining PATH entries".into()
            }
        }));
        return;
    }
    // Old snapshots did not record a typed raw value or removal positions.
    // Preserve their merge semantics and aggregate per-entry failures.
    for item in &snapshot.items {
        if crate::executor::cleanup_cancelled() {
            messages.push("restore cancelled before remaining PATH entries".into());
            break;
        }
        let scopes = path_restore_scopes(item);
        let scope_refs: Vec<_> = scopes.iter().map(String::as_str).collect();
        match crate::regops::restore_path_entry(&item.entry, &scope_refs) {
            Ok(true) => messages.push(format!("restored PATH entry {}", item.entry)),
            Ok(false) => messages.push(format!("PATH entry already present: {}", item.entry)),
            Err(error) => messages.push(format!("PATH restore failed for {}: {error}", item.entry)),
        }
    }
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

pub fn list_sessions() -> Result<Vec<PathBuf>, String> {
    list_sessions_at(&crate::backup::backup_root())
}

fn list_sessions_at(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = vec![];
    match std::fs::symlink_metadata(root) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(crate::error::restore_err(e.to_string()).to_ipc()),
    }
    let _pins = crate::fsutil::pin_local_scan_path(root)
        .map_err(|e| crate::error::restore_err(e.to_string()).to_ipc())?;
    let rd = match root.read_dir() {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(crate::error::restore_err(e.to_string()).to_ipc()),
    };
    for entry in rd {
        // Another account's session may be unreadable (ACL/ownership). One
        // such entry must not hide every other restorable session.
        let Ok(entry) = entry else { continue };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && !crate::fsutil::is_reparse_point(&entry.path()) {
            out.push(entry.path());
        }
    }
    out.sort();
    out.reverse();
    Ok(out)
}

pub fn list_session_names() -> Result<Vec<String>, String> {
    list_session_names_at(&crate::backup::backup_root())
}

/// Restorable sessions only. A directory left behind by an aborted backup has
/// no seal, so offering it here produced a session the user could pick and
/// that then always failed with `seal:missing` for the whole 7-day retention.
fn list_session_names_at(root: &Path) -> Result<Vec<String>, String> {
    Ok(list_sessions_at(root)?
        .iter()
        .filter(|p| session_is_sealed(p))
        .filter_map(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .collect())
}

pub fn restore_by_name(name: &str) -> Result<Vec<String>, String> {
    restore_session(&session_path(name)?)
}

fn session_path(name: &str) -> Result<PathBuf, String> {
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
    Ok(crate::backup::backup_root().join(name))
}

#[derive(Debug, serde::Serialize)]
pub struct RestorePreviewEntry {
    pub target: String,
    pub status: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub struct RestorePreview {
    pub name: String,
    pub files: usize,
    pub registry: usize,
    pub path_entries: usize,
    pub existing: usize,
    pub unavailable: usize,
    pub entries: Vec<RestorePreviewEntry>,
    pub size_kb: Option<u64>,
}

/// Advisory, bounded preview. Authorization and snapshots are checked using
/// the same inputs as restore; file contents are not authenticated by the seal.
/// Never create destination directories or stage/import registry data here.
pub(crate) const PREVIEW_CANCELLED: &str = "backup preview cancelled";

pub(crate) fn preview_by_name_with_cancel(
    name: &str,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<RestorePreview, String> {
    use std::sync::atomic::Ordering::Acquire;
    let is_cancelled = || cancelled.load(Acquire);
    if is_cancelled() {
        return Err(PREVIEW_CANCELLED.into());
    }
    let result = preview_session_with_cancel(name, &session_path(name)?, is_cancelled);
    if is_cancelled() {
        return Err(PREVIEW_CANCELLED.into());
    }
    result
}

#[cfg(test)]
fn preview_by_name(name: &str) -> Result<RestorePreview, String> {
    preview_by_name_with_cancel(name, &Default::default())
}

#[cfg(test)]
fn preview_session(name: &str, session: &Path) -> Result<RestorePreview, String> {
    preview_session_with_cancel(name, session, || false)
}

fn preview_session_with_cancel(
    name: &str,
    session: &Path,
    cancelled: impl Fn() -> bool,
) -> Result<RestorePreview, String> {
    let inputs = validate_session(session, &cancelled)?;
    let mut preview = RestorePreview {
        name: name.into(),
        size_kb: crate::dirsize::walk_size_kb_unbudgeted_with_cancel(session, &cancelled),
        files: inputs.map.len(),
        registry: inputs.imports.len(),
        path_entries: inputs
            .path_snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.items.len()),
        existing: 0,
        unavailable: 0,
        entries: vec![],
    };
    for (rel, target) in inputs.map {
        if cancelled() {
            return Err(PREVIEW_CANCELLED.into());
        }
        if !is_safe_map_rel(&rel) {
            return Err("seal:bad_backup_entry".into());
        }
        let source = session.join("files").join(&rel);
        let dest = Path::new(&target);
        let status =
            if crate::fsutil::is_reparse_point(&source) || crate::fsutil::is_reparse_point(dest) {
                "blocked"
            } else if !source.try_exists().unwrap_or(false) {
                // Stat errors (unplugged drive, denying ACL) degrade to
                // "missing" — same swallow-as-absent semantics as the
                // restore loop's `src.exists()` skip, so one unreadable
                // entry never disables the whole preview.
                "missing"
            } else if !crate::safety::is_safe_restore_target(dest)
                || crate::safety::looks_like_sync_conflict(&target)
                || inputs
                    .seal
                    .targets
                    .binary_search(&crate::path_seal::normalize_target(&target))
                    .is_err()
            {
                "blocked"
            } else if dest.try_exists().unwrap_or(true) {
                // An unreadable destination is reported as a conflict, never
                // as a free target — restore rechecks before writing.
                "existing"
            } else {
                "available"
            };
        preview.existing += usize::from(status == "existing");
        preview.unavailable += usize::from(status == "blocked" || status == "missing");
        if preview.entries.len() < 200 {
            preview.entries.push(RestorePreviewEntry { target, status });
        }
    }
    Ok(preview)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    pub name: String,
    /// Older consumers can still read the field. New lists defer directory size
    /// until the selected session's preview.
    pub size_kb: Option<u64>,
    pub created_at: String,
    /// Kept for callers that used the original restorable flag.
    pub sealed: bool,
    pub status: SessionStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Verified,
    PendingVerification,
    Unsealed,
    RequiresAdmin,
    MissingMetadata,
    Unknown,
    Invalid,
    Legacy,
}

/// Legacy name-only callers still expect these sessions to pass full preflight.
fn session_is_sealed(dir: &Path) -> bool {
    validate_session(dir, || false).is_ok()
}

fn session_status(dir: &Path) -> SessionStatus {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() && !crate::fsutil::is_reparse_metadata(&meta) => {}
        Ok(_) => return SessionStatus::Invalid,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return SessionStatus::RequiresAdmin
        }
        Err(_) => return SessionStatus::Unknown,
    }
    let check = crate::path_seal::verified_seal(dir)
        .and_then(|seal| session_metadata::check_session_metadata(dir, &seal, || false));
    match check.as_ref().map_err(String::as_str) {
        Ok(()) => SessionStatus::PendingVerification,
        Err("seal:missing") => match session_metadata::is_empty_unsealed_session(dir) {
            Ok(true) => SessionStatus::Unsealed,
            Ok(false) => SessionStatus::MissingMetadata,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                SessionStatus::RequiresAdmin
            }
            Err(_) => SessionStatus::Unknown,
        },
        Err(
            "seal:map_missing"
            | "seal:file_manifest_missing"
            | "seal:path_snapshot_missing"
            | "seal:registry_snapshot_missing",
        ) => SessionStatus::MissingMetadata,
        Err("seal:legacy_manual_restore_only") => SessionStatus::Legacy,
        Err("seal:key_unavailable_requires_admin" | "seal:read_requires_admin") => {
            SessionStatus::RequiresAdmin
        }
        Err("seal:read" | "seal:metadata_unreadable") => SessionStatus::Unknown,
        Err(_) => SessionStatus::Invalid,
    }
}

/// List backup sessions with size (KB). Does NOT prune (read path is pure).
///
/// Every session directory is reported, with `sealed` telling the UI which ones
/// are restorable. `prune_old_sessions` still walks the unfiltered list, so an
/// aborted backup's leftover directory is aged out instead of accumulating.
pub fn list_session_info() -> Result<Vec<SessionInfo>, String> {
    list_session_info_at(&crate::backup::backup_root())
}

/// List backup sessions with size (KB). Does NOT prune (read path is pure).
///
/// Every session directory is reported, with `sealed` telling the UI which ones
/// are restorable. `prune_old_sessions` still walks the unfiltered list, so an
/// aborted backup's leftover directory is aged out instead of accumulating.
fn list_session_info_at(root: &Path) -> Result<Vec<SessionInfo>, String> {
    Ok(list_sessions_at(root)?
        .iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().to_string();
            let status = session_status(p);
            Some(SessionInfo {
                name: name.clone(),
                size_kb: None,
                created_at: name.split('_').next().unwrap_or("").to_string(),
                sealed: status == SessionStatus::PendingVerification,
                status,
            })
        })
        .collect())
}

/// Delete backup sessions older than `days` (Safety Vault retention). Returns removed count.
pub fn prune_old_sessions(days: u64) -> usize {
    prune_old_sessions_at(days, None)
}

/// Same as [`prune_old_sessions`] with optional fixed `now` (unix secs) for tests.
pub fn prune_old_sessions_at(days: u64, now_override: Option<u64>) -> usize {
    // A restore holds CLEANUP_LOCK for its whole write phase; pruning must not
    // delete the session's files underneath it.
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let now = now_override.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    let cutoff = now.saturating_sub(days.saturating_mul(24 * 3600));
    let mut removed = 0usize;
    let Ok(sessions) = list_sessions() else {
        return 0;
    };
    for p in sessions {
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
        if expired && is_session_name(&name) && remove_confirmed_session(&p, None).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Delete one backup session by name. Path-traversal guarded.
/// Session names must match `^[0-9]{8}-[0-9]{6}` (`YYYYMMDD-HHMMSS…`).
pub fn delete_session_by_name(name: &str) -> Result<(), String> {
    delete_session_with_status(name, None)
}

pub fn delete_session_with_status(
    name: &str,
    expected: Option<SessionStatus>,
) -> Result<(), String> {
    // Serialize with restore: deleting a session mid-restore leaves a partial
    // restore with no source to retry from.
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
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
    remove_confirmed_session(&path, expected)
}

/// Applies to explicit removal and retention. A missing seal cannot authorize
/// deleting any payload and a sealed session must pass its full preflight.
fn remove_confirmed_session(path: &Path, expected: Option<SessionStatus>) -> Result<(), String> {
    match (session_status(path), expected) {
        (SessionStatus::Unsealed, None | Some(SessionStatus::Unsealed)) => {
            crate::fsutil::remove_empty_backup_session(path).map_err(|error| error.to_string())
        }
        (SessionStatus::PendingVerification, None | Some(SessionStatus::Verified)) => {
            crate::fsutil::remove_verified_backup_session(path, || {
                validate_session(path, || false).map(|_| ())
            })
        }
        _ => Err("backup session status changed or could not be verified; files were kept".into()),
    }
}

/// Backup map keys are opaque `{digest}_{name}` tokens written by backup — one path segment only.
fn is_safe_map_rel(rel: &str) -> bool {
    if rel.is_empty() || rel == "." || rel == ".." {
        return false;
    }
    // Separators and drive markers are rejected outright, so a lone segment
    // can never traverse. `..` inside a name (e.g. "a..b.txt") is a legal
    // NTFS file name and must stay previewable/restorable.
    if rel.contains('/') || rel.contains('\\') || rel.contains(':') {
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
    fn preview_rejects_invalid_names_without_loading_a_session() {
        for name in ["", ".", "..", "123_../../escape", "123_bad:name"] {
            assert_eq!(
                super::preview_by_name(name).unwrap_err(),
                "invalid session name"
            );
        }
    }
    #[test]
    fn a_pending_reboot_blocks_restore_before_validation_or_writes() {
        crate::executor::reboot::reset_for_test();
        crate::executor::reboot::mark_pending("source\u{0}key\u{0}App");
        let missing = std::path::Path::new("does-not-exist");
        assert_eq!(
            super::restore_session(missing).unwrap_err(),
            crate::executor::reboot::PENDING_MESSAGE
        );
        crate::executor::reboot::reset_for_test();
    }

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
    fn reg_text_is_encoded_as_utf16le_with_bom() {
        let text = "Windows Registry Editor Version 5.00\r\n[HKEY_CURRENT_USER\\Software\\测试]\r\n\"名称\"=\"值\"\r\n";
        let bytes = super::encode_reg_text(text);
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        assert_eq!(super::decode_reg_text(bytes).unwrap(), text);
    }

    #[test]
    fn value_level_restores_are_allowed_for_muicache_and_firewall() {
        let muicache = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache]\r\n\"C:\\\\Apps\\\\Demo\\\\demo.exe.FriendlyAppName\"=\"Demo\"\r\n";
        assert!(super::reg_content_allowed(muicache).is_ok());
        // The whole key or a non-FriendlyAppName value is still refused.
        let bare = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache]\r\n";
        assert!(super::reg_content_allowed(bare).is_err());
        let wrong =
            "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache]\r\n\"C:\\\\x.exe\"=\"x\"\r\n";
        assert!(super::reg_content_allowed(wrong).is_err());

        let firewall = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\SharedAccess\\Parameters\\FirewallPolicy\\FirewallRules]\r\n\"Demo Rule\"=\"v2.10|Action=Allow\"\r\n";
        assert!(super::reg_content_allowed(firewall).is_ok());
        let firewall_bare = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\SharedAccess\\Parameters\\FirewallPolicy\\FirewallRules]\r\n";
        assert!(super::reg_content_allowed(firewall_bare).is_err());
    }

    #[test]
    fn map_rel_rejects_traversal_and_absolute() {
        assert!(super::is_safe_map_rel("abc123_file.txt"));
        assert!(super::is_safe_map_rel("abc123_a..b.txt"));
        assert!(!super::is_safe_map_rel(""));
        assert!(!super::is_safe_map_rel(".."));
        assert!(!super::is_safe_map_rel("../evil"));
        assert!(!super::is_safe_map_rel("a/b"));
        assert!(!super::is_safe_map_rel(r"a\b"));
        assert!(!super::is_safe_map_rel(r"C:\Windows\evil"));
    }

    #[test]
    fn restore_order_groups_files_by_destination_parent() {
        let mut map = std::collections::BTreeMap::new();
        map.insert("hash-z".to_string(), r"C:\App\z.bin".to_string());
        map.insert("hash-a".to_string(), r"D:\Other\a.bin".to_string());
        map.insert("hash-b".to_string(), r"C:\App\b.bin".to_string());
        let ordered: Vec<&str> = super::restore_order(&map)
            .into_iter()
            .map(|(_, original)| original.as_str())
            .collect();
        assert_eq!(
            ordered,
            vec![r"C:\App\b.bin", r"C:\App\z.bin", r"D:\Other\a.bin"]
        );
    }

    #[test]
    fn restore_missing_session_errors() {
        let p = std::env::temp_dir().join("remova_no_such_session_xyz");
        assert!(restore_session(&p).is_err());
    }

    #[test]
    fn a_cancelled_restore_leaves_the_cleanup_queue_before_the_lock_releases() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Arc};

        let held = crate::executor::CLEANUP_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let flag = Arc::new(AtomicBool::new(false));
        let worker_flag = flag.clone();
        let (started, waiting) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let outcome = crate::executor::with_operation_cancellation(worker_flag, || {
                started.send(()).unwrap();
                restore_session(Path::new("unopened-restore-fixture"))
            });
            let _ = finished.send(outcome);
        });
        waiting.recv().unwrap();
        flag.store(true, Ordering::Release);
        let before_unlock = result.recv_timeout(std::time::Duration::from_millis(500));
        drop(held);
        worker.join().unwrap();
        assert_eq!(
            before_unlock.unwrap().unwrap_err(),
            "restore cancelled before any changes"
        );
    }

    /// `backup_batch` creates the session directory and then can return
    /// `Err` before `write_seal`, leaving an unsealed directory on disk. It was
    /// offered as a restore target and then always failed with `seal:missing`
    /// for the whole 7-day retention window.
    #[test]
    fn unsealed_sessions_are_reported_but_never_offered_for_restore() {
        let root = crate::test_support::TempDir::new("unsealed");
        // The shape an aborted backup leaves behind: `files/` + `registry/`,
        // no seal.
        let aborted = root.join("1700000000_aborted");
        fs::create_dir_all(aborted.join("files")).unwrap();
        fs::create_dir_all(aborted.join("registry")).unwrap();
        fs::write(aborted.join("files/some.txt"), b"partial").unwrap();
        // A directory that is not even a session shape.
        let junk = root.join("1700000001_not_a_session");
        fs::create_dir_all(&junk).unwrap();

        let info = super::list_session_info_at(&root).unwrap();
        assert_eq!(info.len(), 2, "both directories are still reported");
        for session in &info {
            assert!(
                !session.sealed,
                "{} has no seal and must be reported as unrestorable",
                session.name
            );
        }
        // The restore selector must offer neither.
        assert!(super::list_session_names_at(&root).unwrap().is_empty());
        // Retention must still see them, or aborted leftovers would accumulate.
        // `prune_old_sessions_at` reads `list_sessions()` = `list_sessions_at(
        // backup_root())`, i.e. the same unfiltered list.
        assert_eq!(super::list_sessions_at(&root).unwrap().len(), 2);
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
        // Self-restoring. The comment that used to sit here ("do not remove_var
        // here ... no other test needs the default root") was wrong: another test
        // had already installed its own override, and leaving ours set — or
        // removing it outright — silently repointed that test at ProgramData.
        let _set = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", &tmp);
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
        delete_session_by_name(fresh.file_name().unwrap().to_str().unwrap()).unwrap();
        assert!(!fresh.exists());
        // Do not remove_var here : the guard serializes env access; the
        // process exits after the suite and no other test needs the default root.
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(windows)]
    fn session_delete_and_retention_refuse_junction_ancestors() {
        let _lock = crate::backup::lock_backup_env();
        let saved = crate::test_support::EnvVar::remove("REMOVA_BACKUP_DIR");
        let root = std::env::temp_dir().join(format!(
            "remova-session-parent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let target = root.join("target");
        let session = target.join("Backup/1700000000_fixture");
        fs::create_dir_all(&session).unwrap();
        fs::write(session.join("sentinel"), b"preserved").unwrap();
        let alias = root.join("alias");
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(&alias)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        // Self-restoring, and dropped only when this scope ends: the alias is a
        // junction, so the override must not outlive the directory it points at.
        let _alias_env =
            crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", alias.join("Backup"));
        assert!(delete_session_by_name("1700000000_fixture").is_err());
        assert_eq!(prune_old_sessions_at(0, Some(1_800_000_000)), 0);
        assert_eq!(fs::read(session.join("sentinel")).unwrap(), b"preserved");
        drop(_alias_env);
        drop(saved);
        fs::remove_dir(alias).unwrap();
        crate::fsutil::remove_tree_no_reparse(&root).unwrap();
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
        // Legacy sealed backups can still restore to a free destination.
        let msgs = restore_session(&sess).unwrap();
        assert!(msgs.iter().any(|m| m.contains("restored")), "{msgs:?}");
        assert_eq!(fs::read_to_string(&orig).unwrap(), "hello-backup");
        // a tampered map is refused wholesale; the restored file stays intact
        fs::write(files.join("path_map.json"), b"{}").unwrap();
        assert_eq!(restore_session(&sess).unwrap_err(), "seal:map_mismatch");
        assert_eq!(fs::read_to_string(&orig).unwrap(), "hello-backup");
        crate::path_seal::set_seals_root_for_tests(None);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_reports_counts_and_caps_entries() {
        let tmp = std::env::temp_dir().join(format!("remova_restore_pv_{}", std::process::id()));
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
        // both backup sources exist; aaa's destination is absent (available),
        // abc's destination exists (existing), the 205 missing_* rows have
        // no backup source at all.
        fs::write(files.join("aaa_fresh.txt"), b"fresh").unwrap();
        fs::write(files.join("abc_file.txt"), b"hello-backup").unwrap();
        let missing = tmp.join("not-created").join("missing.txt");
        let fresh = tmp.join("not-created").join("fresh.txt");
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            "aaa_fresh.txt".to_string(),
            fresh.to_string_lossy().to_string(),
        );
        map.insert(
            "abc_file.txt".to_string(),
            orig.to_string_lossy().to_string(),
        );
        for i in 0..205 {
            map.insert(
                format!("missing_{i}"),
                missing.to_string_lossy().to_string(),
            );
        }
        fs::write(
            files.join("path_map.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        crate::path_seal::write_seal(&sess, "", &map, &std::collections::BTreeMap::new()).unwrap();
        let preview = preview_session("sess", &sess).unwrap();
        assert_eq!(preview.files, 207);
        assert_eq!(preview.entries.len(), 200);
        assert_eq!(preview.existing, 1);
        assert_eq!(preview.unavailable, 205);
        // the cap keeps the dictionary-first targets: the available and
        // existing rows stay reachable ahead of the missing crowd.
        assert!(preview.entries.iter().any(|e| e.status == "available"));
        assert!(preview
            .entries
            .iter()
            .any(|e| e.status == "existing" && e.target == orig.to_string_lossy()));
        // preview is read-only: no destination parents are created.
        assert_eq!(fs::read(&orig).unwrap(), b"hello");
        assert!(!missing.parent().unwrap().exists());
        fs::write(files.join("path_map.json"), b"{}").unwrap();
        assert_eq!(
            preview_session("sess", &sess).unwrap_err(),
            "seal:map_mismatch"
        );
        assert_eq!(fs::read_to_string(&orig).unwrap(), "hello");
        crate::path_seal::set_seals_root_for_tests(None);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A legal NTFS name containing `..` must keep its session fully
    /// previewable and restorable.
    #[test]
    fn preview_and_restore_accept_dotdot_inside_names() {
        let tmp = std::env::temp_dir().join(format!("remova_restore_dd_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let seals = tmp.join("seals");
        fs::create_dir_all(&seals).unwrap();
        let _g = crate::path_seal::test_lock();
        crate::path_seal::set_seals_root_for_tests(Some(seals));
        let sess = tmp.join("1700000000_dd");
        let files = sess.join("files");
        fs::create_dir_all(&files).unwrap();
        let dest = tmp.join("dest");
        fs::create_dir_all(&dest).unwrap();
        let target = dest.join("a..b.txt");
        fs::write(&target, b"old").unwrap();
        let rel = "abc_a..b.txt";
        fs::write(files.join(rel), b"dd").unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert(rel.to_string(), target.to_string_lossy().to_string());
        fs::write(
            files.join("path_map.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        crate::path_seal::write_seal(&sess, "", &map, &std::collections::BTreeMap::new()).unwrap();
        let preview = preview_session("sess", &sess).unwrap();
        assert_eq!(preview.files, 1);
        assert_eq!(preview.entries[0].status, "existing");
        let rows = restore_session(&sess).unwrap();
        assert!(
            rows.iter()
                .any(|row| row.starts_with("skipped existing restore target: ")),
            "{rows:?}"
        );
        assert_eq!(fs::read(&target).unwrap(), b"old");
        fs::remove_file(&target).unwrap();
        let msgs = restore_session(&sess).unwrap();
        assert!(msgs.iter().any(|m| m.contains("restored")), "{msgs:?}");
        assert_eq!(fs::read(&target).unwrap(), b"dd");
        crate::path_seal::set_seals_root_for_tests(None);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A destination whose stat fails (unplugged drive, denying ACL — here a
    /// deterministic std-level stat error) must degrade to a conflict row,
    /// never abort the whole preview.
    #[test]
    fn preview_treats_unreadable_destinations_as_conflicts() {
        let tmp = std::env::temp_dir().join(format!("remova_restore_ds_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let seals = tmp.join("seals");
        fs::create_dir_all(&seals).unwrap();
        let _g = crate::path_seal::test_lock();
        crate::path_seal::set_seals_root_for_tests(Some(seals));
        let sess = tmp.join("1700000000_ds");
        let files = sess.join("files");
        fs::create_dir_all(&files).unwrap();
        // Interior NUL makes every stat return InvalidInput — a std-level
        // stand-in for stat errors that are not NotFound.
        let dest = format!("{}\u{0000}child.txt", tmp.join("blocker").to_string_lossy());
        let rel = "abc_child.txt";
        fs::write(files.join(rel), b"payload").unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert(rel.to_string(), dest);
        fs::write(
            files.join("path_map.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        crate::path_seal::write_seal(&sess, "", &map, &std::collections::BTreeMap::new()).unwrap();
        let preview = preview_session("sess", &sess).unwrap();
        assert_eq!(preview.files, 1);
        assert_eq!(preview.entries[0].status, "existing");
        assert_eq!(preview.existing, 1);
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
            ..Default::default()
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

    /// a PATH entry that cannot be restored is aggregated with its real
    /// cause and the session continues — the file write-back must never leave
    /// the registry import undone because one PATH entry failed.
    #[cfg(windows)]
    #[test]
    fn path_restore_failure_is_aggregated_and_continues() {
        let tmp =
            std::env::temp_dir().join(format!("remova_restore_pathfail_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let seals = tmp.join("seals");
        // TestStore owns the seal-root override AND its lock; taking test_lock
        // separately here would deadlock.
        let _store = crate::path_seal::TestStore::new(seals);
        let sess = tmp.join("sess");
        let files = sess.join("files");
        fs::create_dir_all(&files).unwrap();
        let orig = tmp.join("orig\\file.txt");
        fs::create_dir_all(orig.parent().unwrap()).unwrap();
        let rel = "abc_file.txt";
        fs::write(files.join(rel), b"hello-backup").unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert(rel.to_string(), orig.to_string_lossy().to_string());
        fs::write(
            files.join("path_map.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        // A PATH snapshot whose single entry cannot be restored.
        let snap = crate::backup::PathSnapshot {
            items: vec![crate::backup::PathSnapshotItem {
                entry: r"C:\RemovaNoSuchVendor\bin".into(),
                scopes: vec!["User".into()],
                user_path: String::new(),
                machine_path: String::new(),
            }],
            ..Default::default()
        };
        let path_bytes = serde_json::to_vec(&snap).unwrap();
        fs::write(sess.join("path.json"), &path_bytes).unwrap();
        let mut digests = std::collections::BTreeMap::new();
        digests.insert(
            "path.json".to_string(),
            crate::path_seal::map_digest(&path_bytes),
        );
        crate::path_seal::write_seal(&sess, "", &map, &digests).unwrap();

        // Force every PATH read to fail so restore_path_entry errors per
        // scope. The mock must be INSTALLED (not merely cleared): with no mock
        // state the reader falls through to the real registry. The reset guard
        // clears it even if an assertion below panics.
        let _mock = crate::regops::path_mock::lock_mock();
        let _reset = crate::regops::path_mock::reset_on_drop();
        crate::regops::path_mock::install(r"C:\Windows", r"C:\Windows");
        crate::regops::path_mock::set_fail_read(true);
        let msgs = restore_session(&sess).unwrap();

        assert!(
            msgs.iter().any(|m| m.contains("restored")),
            "the file write-back must still happen: {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| { m.contains(r"PATH restore failed for C:\RemovaNoSuchVendor\bin") }),
            "the PATH failure must be aggregated with its entry and real cause: {msgs:?}"
        );
        assert_eq!(fs::read_to_string(&orig).unwrap(), "hello-backup");
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
    /// An authenticated session that still names a protected target: the
    /// earlier version of this test only proved a missing seal is rejected, so
    /// the protected-target branch itself was never exercised.
    #[test]
    fn an_authenticated_session_skips_protected_targets_and_restores_siblings() {
        let tmp = std::env::temp_dir().join(format!("remova_restore_prot_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let seals = tmp.join("seals");
        fs::create_dir_all(&seals).unwrap();
        let _g = crate::path_seal::test_lock();
        crate::path_seal::set_seals_root_for_tests(Some(seals));
        let sess = tmp.join("sess");
        let files = sess.join("files");
        fs::create_dir_all(&files).unwrap();
        // One protected destination and one ordinary sibling.
        let protected = r"C:\Windows\System32\evil.dll";
        let sibling = tmp.join("sibling").join("keep.txt");
        fs::create_dir_all(sibling.parent().unwrap()).unwrap();
        fs::write(files.join("evil.txt"), b"pwn").unwrap();
        fs::write(files.join("keep.txt"), b"restored").unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert("evil.txt".to_string(), protected.to_string());
        map.insert(
            "keep.txt".to_string(),
            sibling.to_string_lossy().to_string(),
        );
        fs::write(
            files.join("path_map.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        // The seal authenticates the map, so the skip below comes from the
        // protected-target gate, not from a missing seal.
        crate::path_seal::write_seal(&sess, "", &map, &std::collections::BTreeMap::new()).unwrap();
        let msgs = restore_session(&sess).expect("the session is authenticated");
        assert!(
            msgs.iter()
                .any(|m| m.contains("skipped protected restore target")),
            "the protected target is named as skipped: {msgs:?}"
        );
        assert_eq!(
            fs::read(&sibling).unwrap(),
            b"restored",
            "the sibling is restored"
        );
        assert!(!std::path::Path::new(protected).exists());
        crate::path_seal::set_seals_root_for_tests(None);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// Payload bytes are bound into the seal: rewriting a backed-up file after
    /// the session was sealed must not restore the modified bytes.
    #[test]
    fn a_modified_backup_payload_is_refused_by_the_seal() {
        let tmp =
            std::env::temp_dir().join(format!("remova_restore_payload_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let seals = tmp.join("seals");
        fs::create_dir_all(&seals).unwrap();
        let _g = crate::path_seal::test_lock();
        crate::path_seal::set_seals_root_for_tests(Some(seals));
        let sess = tmp.join("sess");
        let files = sess.join("files");
        fs::create_dir_all(&files).unwrap();
        let original = tmp.join("target").join("keep.txt");
        fs::create_dir_all(original.parent().unwrap()).unwrap();
        let rel = "payload.txt";
        fs::write(files.join(rel), b"original bytes").unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert(rel.to_string(), original.to_string_lossy().to_string());
        fs::write(
            files.join("path_map.json"),
            serde_json::to_string(&map).unwrap(),
        )
        .unwrap();
        let digests = std::collections::BTreeMap::from([(
            format!("files/{rel}"),
            crate::path_seal::publication_file_digest(&files.join(rel)).unwrap(),
        )]);
        crate::path_seal::write_seal_with_files(
            &sess,
            "",
            &map,
            &std::collections::BTreeMap::new(),
            Some(digests),
        )
        .unwrap();
        // Rewrite the payload after sealing.
        fs::write(files.join(rel), b"tampered bytes").unwrap();
        let msgs = restore_session(&sess).expect("the session itself is authentic");
        assert!(
            msgs.iter().any(|m| m.contains("digest mismatch")),
            "the modified payload must be refused: {msgs:?}"
        );
        assert!(!original.exists(), "nothing is written back");
        crate::path_seal::set_seals_root_for_tests(None);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[cfg(windows)]
    fn restore_fixture_item(
        source: &Path,
        kind: crate::scanner::ItemKind,
    ) -> crate::scanner::CleanupItem {
        crate::scanner::CleanupItem {
            path: source.to_string_lossy().into_owned(),
            kind,
            score: 90,
            confidence: crate::scanner::Confidence::Confirmed,
            risk: crate::scanner::RiskLevel::Low,
            reason: "fixture".into(),
            evidence: vec![],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        }
    }

    #[cfg(windows)]
    fn back_up_fixture_directory(source: &Path, session: &Path) -> PathBuf {
        fs::create_dir(session).unwrap();
        let item = restore_fixture_item(source, crate::scanner::ItemKind::Dir);
        let (ok, fail, errors) = crate::backup::backup_items(&[item], session);
        assert_eq!((ok, fail), (1, 0), "{errors:?}");
        fs::read_dir(session.join("files"))
            .unwrap()
            .flatten()
            .find(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .expect("the directory payload is published")
            .path()
    }

    #[cfg(windows)]
    #[test]
    fn restore_ignores_late_named_streams_and_checks_the_authenticated_default_stream() {
        let root = crate::test_support::TempDir::new("restore-named-streams");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.subdir("source");
        fs::write(source.join("a.txt"), b"backed up").unwrap();
        let session = root.join("1700000000_dir");
        let backed = back_up_fixture_directory(&source, &session);
        fs::write(backed.join("a.txt:FixtureMetadata"), b"unsealed stream").unwrap();
        fs::remove_dir_all(&source).unwrap();

        let msgs = restore_session(&session).unwrap();
        assert!(msgs.iter().any(|m| m.starts_with("restored ")), "{msgs:?}");
        assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"backed up");
        assert!(!source.join("a.txt:FixtureMetadata").exists());
        assert_eq!(
            fs::read(backed.join("a.txt:FixtureMetadata")).unwrap(),
            b"unsealed stream"
        );

        fs::remove_dir_all(&source).unwrap();
        fs::write(backed.join("a.txt"), b"modified default stream").unwrap();
        let msgs = restore_session(&session).unwrap();
        assert!(
            msgs.iter().any(|m| m.contains("digest mismatch")),
            "{msgs:?}"
        );
        assert!(!source.exists());
    }

    #[cfg(windows)]
    #[test]
    fn legacy_directory_inventory_still_refuses_unrecorded_files_and_named_streams() {
        let root = crate::test_support::TempDir::new("restore-legacy-inventory");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.join("source");
        let session = root.subdir("1700000000_legacy");
        let files = session.join("files");
        let backed = files.join("payload");
        fs::create_dir_all(&backed).unwrap();
        let data = backed.join("a.txt");
        fs::write(&data, b"backed up").unwrap();
        let map = std::collections::BTreeMap::from([(
            "payload".into(),
            source.to_string_lossy().into_owned(),
        )]);
        fs::write(
            files.join("path_map.json"),
            serde_json::to_vec(&map).unwrap(),
        )
        .unwrap();
        let digests = std::collections::BTreeMap::from([(
            "files/payload/a.txt".into(),
            crate::path_seal::publication_file_digest(&data).unwrap(),
        )]);
        crate::path_seal::write_seal_with_files(
            &session,
            "",
            &map,
            &std::collections::BTreeMap::new(),
            Some(digests),
        )
        .unwrap();

        let extra = backed.join("injected.txt");
        fs::write(&extra, b"unexpected").unwrap();
        let rows = restore_session(&session).unwrap();
        assert!(
            rows.iter()
                .any(|row| row.contains("unrecorded backup file")),
            "{rows:?}"
        );
        fs::remove_file(extra).unwrap();
        fs::write(backed.join("a.txt:FixtureMetadata"), b"unexpected stream").unwrap();
        let rows = restore_session(&session).unwrap();
        assert!(
            rows.iter().any(|row| row.contains("named data streams")),
            "{rows:?}"
        );
        assert!(!source.exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_changed_manifest_is_deferred_by_listing_and_refused_before_restore_or_removal() {
        let _env = crate::backup::lock_backup_env();
        let root = crate::test_support::TempDir::new("restore-manifest-status");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", root.join("Backup"));
        let source = root.subdir("source");
        fs::write(source.join("a.txt"), b"backed up").unwrap();
        let backup_root = root.subdir("Backup");
        let name = "1700000000_dir";
        let session = backup_root.join(name);
        back_up_fixture_directory(&source, &session);
        let manifest = session.join(crate::path_seal::FILE_MANIFEST_NAME);
        assert_eq!(session_status(&session), SessionStatus::PendingVerification);
        assert!(preview_session(name, &session).is_ok());

        for bytes in [None, Some(b"{}".as_slice())] {
            if let Some(bytes) = bytes {
                fs::write(&manifest, bytes).unwrap();
            } else {
                fs::remove_file(&manifest).unwrap();
            }
            let status = if bytes.is_some() {
                SessionStatus::PendingVerification
            } else {
                SessionStatus::MissingMetadata
            };
            assert_eq!(session_status(&session), status);
            assert!(list_session_names_at(&backup_root).unwrap().is_empty());
            let rows = list_session_info_at(&backup_root).unwrap();
            assert_eq!((rows[0].status, rows[0].size_kb), (status, None));
            assert!(preview_session(name, &session)
                .unwrap_err()
                .contains("file_manifest"));
            assert!(restore_session(&session)
                .unwrap_err()
                .contains("file_manifest"));
            assert!(delete_session_with_status(name, Some(SessionStatus::Unsealed)).is_err());
            assert!(delete_session_with_status(name, Some(SessionStatus::Verified)).is_err());
            assert!(delete_session_by_name(name).is_err());
            assert!(session.is_dir());
        }
    }

    #[cfg(windows)]
    #[test]
    fn missing_backup_metadata_protects_payload_from_removal_and_retention() {
        let _env = crate::backup::lock_backup_env();
        let root = crate::test_support::TempDir::new("backup-session-protection");
        let seals = root.subdir("seals");
        let _store = crate::path_seal::TestStore::new(seals.clone());
        let backups = root.subdir("Backup");
        let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", &backups);
        let source = root.subdir("source");
        fs::write(source.join("a.txt"), b"payload").unwrap();

        let no_seal = "1700000000_no_seal";
        let no_map = "1700000001_no_map";
        let verified = "1700000002_complete";
        for name in [no_seal, no_map, verified] {
            back_up_fixture_directory(&source, &backups.join(name));
        }
        let empty = "1700000003_empty";
        fs::create_dir_all(backups.join(empty).join("files")).unwrap();
        fs::create_dir_all(backups.join(empty).join("registry")).unwrap();
        let legacy = "1700000004_legacy";
        fs::create_dir_all(backups.join(legacy).join("files")).unwrap();
        fs::write(
            backups.join(legacy).join("files/legacy.bin"),
            b"legacy payload",
        )
        .unwrap();
        fs::write(seals.join(format!("{legacy}.seal.json")), b"RSEAL1").unwrap();
        fs::remove_file(seals.join(format!("{no_seal}.seal.json"))).unwrap();
        fs::remove_file(backups.join(no_map).join("files/path_map.json")).unwrap();

        let rows: std::collections::BTreeMap<_, _> = list_session_info()
            .unwrap()
            .into_iter()
            .map(|row| (row.name, row.status))
            .collect();
        assert_eq!(rows[no_seal], SessionStatus::MissingMetadata);
        assert_eq!(rows[no_map], SessionStatus::MissingMetadata);
        assert_eq!(rows[verified], SessionStatus::PendingVerification);
        assert_eq!(rows[empty], SessionStatus::Unsealed);
        assert_eq!(rows[legacy], SessionStatus::Legacy);
        assert_eq!(preview_by_name(no_map).unwrap_err(), "seal:map_missing");
        for name in [no_seal, no_map, legacy] {
            assert!(delete_session_by_name(name).is_err());
            assert!(delete_session_with_status(name, Some(SessionStatus::Unsealed)).is_err());
            assert!(delete_session_with_status(name, Some(SessionStatus::Verified)).is_err());
        }
        assert_eq!(prune_old_sessions_at(7, Some(1_800_000_000)), 2);
        for name in [no_seal, no_map, legacy] {
            assert!(backups.join(name).is_dir());
        }
        for name in [verified, empty] {
            assert!(!backups.join(name).exists());
        }
    }

    #[cfg(windows)]
    #[test]
    fn complete_session_restores_files_without_using_unrecorded_snapshots() {
        let root = crate::test_support::TempDir::new("restore-recorded-only");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.subdir("source");
        fs::write(source.join("a.txt"), b"recorded file").unwrap();
        let session = root.join("1700000000_session");
        back_up_fixture_directory(&source, &session);
        fs::write(session.join("path.json"), b"unrecorded path").unwrap();
        fs::create_dir_all(session.join("registry/extra")).unwrap();
        fs::write(
            session.join("registry/extra/export.reg"),
            b"unrecorded registry bytes",
        )
        .unwrap();
        let inputs = validate_session(&session, || false).unwrap();
        assert!(inputs.path_snapshot.is_none());
        assert!(inputs.imports.is_empty());
        fs::remove_dir_all(&source).unwrap();
        let rows = restore_session(&session).unwrap();
        assert_eq!(rows, [format!("restored {}", source.display())]);
        assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"recorded file");
    }

    #[cfg(windows)]
    #[test]
    fn manifest_restores_only_the_recorded_files_from_a_directory_with_extra_content() {
        let root = crate::test_support::TempDir::new("restore-extra-member");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.subdir("source");
        fs::write(source.join("a.txt"), b"backed up").unwrap();
        let session = root.join("1700000000_dir");
        let backed = back_up_fixture_directory(&source, &session);
        fs::write(backed.join("injected.txt"), b"not backed up").unwrap();
        fs::remove_dir_all(&source).unwrap();

        let msgs = restore_session(&session).expect("the session itself is authentic");
        assert!(msgs.iter().any(|m| m.starts_with("restored ")), "{msgs:?}");
        assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"backed up");
        assert!(
            !source.join("injected.txt").exists(),
            "an unbacked file must never be written back"
        );
    }

    #[cfg(windows)]
    #[test]
    fn restore_commits_only_staged_verified_bytes_if_the_source_changes() {
        let root = crate::test_support::TempDir::new("restore-staged-race");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.subdir("source");
        fs::write(source.join("a.txt"), b"authentic").unwrap();
        let session = root.join("1700000000_dir");
        let backed = back_up_fixture_directory(&source, &session);
        fs::remove_dir_all(&source).unwrap();

        let msgs = restore_with_before_copy(&session, |public| {
            assert_eq!(public, backed);
            fs::write(public.join("a.txt"), b"tampered after verification").unwrap();
            fs::write(public.join("injected.txt"), b"added after verification").unwrap();
        })
        .unwrap();

        assert!(msgs.iter().any(|m| m.starts_with("restored ")), "{msgs:?}");
        assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"authentic");
        assert!(!source.join("injected.txt").exists());
        assert!(fs::read_dir(root.join("seals")).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("stage_")));
    }

    #[cfg(windows)]
    #[test]
    fn a_late_directory_keeps_its_files_and_streams_until_moved_aside() {
        let root = crate::test_support::TempDir::new("restore-late-directory");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.subdir("source");
        fs::write(source.join("a.txt"), b"backed up").unwrap();
        fs::create_dir(source.join("empty")).unwrap();
        let session = root.join("1700000000_dir");
        back_up_fixture_directory(&source, &session);
        fs::remove_dir_all(&source).unwrap();
        assert_eq!(
            preview_session("1700000000_dir", &session)
                .unwrap()
                .existing,
            0
        );

        let rows = restore_with_before_copy(&session, |_| {
            fs::create_dir(&source).unwrap();
            fs::write(source.join("a.txt"), b"created during restore").unwrap();
            fs::write(source.join("a.txt:NewMetadata"), b"named stream").unwrap();
        })
        .unwrap();
        assert_eq!(
            rows,
            [format!(
                "skipped existing restore target: {}",
                source.display()
            )]
        );
        assert_eq!(
            fs::read(source.join("a.txt")).unwrap(),
            b"created during restore"
        );
        assert_eq!(
            fs::read(source.join("a.txt:NewMetadata")).unwrap(),
            b"named stream"
        );
        assert!(!source.join("empty").exists());

        let preserved = root.join("preserved");
        fs::rename(&source, &preserved).unwrap();
        assert_eq!(
            restore_session(&session).unwrap(),
            [format!("restored {}", source.display())]
        );
        assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"backed up");
        assert!(source.join("empty").is_dir());
        assert_eq!(
            fs::read(preserved.join("a.txt:NewMetadata")).unwrap(),
            b"named stream"
        );
    }

    #[cfg(windows)]
    #[test]
    fn restore_preserves_an_occupied_file_and_continues_with_other_entries() {
        use crate::scanner::ItemKind;
        let root = crate::test_support::TempDir::new("restore-occupied-file");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let target = root.join("late.txt");
        let sibling = root.join("sibling.txt");
        let named = root.join("late.txt:NewMetadata");
        fs::write(&target, b"backed up target").unwrap();
        fs::write(&sibling, b"backed up sibling").unwrap();
        let session = root.subdir("1700000000_files");
        let items = [
            restore_fixture_item(&target, ItemKind::File),
            restore_fixture_item(&sibling, ItemKind::File),
        ];
        let (ok, failed, errors) = crate::backup::backup_items(&items, &session);
        assert_eq!((ok, failed), (2, 0), "{errors:?}");
        let regular_permissions = fs::metadata(&target).unwrap().permissions();
        fs::remove_file(&target).unwrap();
        fs::remove_file(&sibling).unwrap();

        let mut first = true;
        let rows = restore_with_before_copy(&session, |_| {
            if std::mem::take(&mut first) {
                fs::write(&target, b"new live file").unwrap();
                fs::write(&named, b"keep this stream").unwrap();
                let mut perms = fs::metadata(&target).unwrap().permissions();
                perms.set_readonly(true);
                fs::set_permissions(&target, perms).unwrap();
            }
        })
        .unwrap();
        assert_eq!(
            rows,
            [
                format!("skipped existing restore target: {}", target.display()),
                format!("restored {}", sibling.display()),
            ]
        );
        assert_eq!(fs::read(&target).unwrap(), b"new live file");
        assert_eq!(fs::read(&named).unwrap(), b"keep this stream");
        assert_eq!(fs::read(&sibling).unwrap(), b"backed up sibling");
        assert!(fs::metadata(&target).unwrap().permissions().readonly());
        fs::set_permissions(&target, regular_permissions).unwrap();

        // A directory at the original file's name is a conflict as well.
        fs::rename(&target, root.join("preserved.txt")).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), b"existing directory contents").unwrap();
        let rows = restore_session(&session).unwrap();
        assert!(
            rows.iter()
                .all(|row| row.starts_with("skipped existing restore target: ")),
            "{rows:?}"
        );
        assert_eq!(
            fs::read(target.join("keep.txt")).unwrap(),
            b"existing directory contents"
        );
        assert_eq!(
            fs::read(root.join("preserved.txt:NewMetadata")).unwrap(),
            b"keep this stream"
        );
    }

    #[cfg(windows)]
    #[test]
    fn restore_does_not_write_through_a_parent_junction() {
        let root = crate::test_support::TempDir::new("restore-parent-junction");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let parent = root.subdir("source");
        let target = parent.join("a.txt");
        fs::write(&target, b"backed up").unwrap();
        let session = root.subdir("1700000000_file");
        let item = restore_fixture_item(&target, crate::scanner::ItemKind::File);
        let (ok, failed, errors) = crate::backup::backup_items(&[item], &session);
        assert_eq!((ok, failed), (1, 0), "{errors:?}");
        fs::remove_dir_all(&parent).unwrap();
        let outside = root.subdir("outside");
        let junction = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(&parent)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(junction.status.success(), "{:?}", junction.stderr);
        let rows = restore_session(&session).unwrap();
        assert!(
            rows.iter()
                .any(|row| row.starts_with("restore failed for ")),
            "{rows:?}"
        );
        assert!(!outside.join("a.txt").exists());
        fs::remove_dir(&parent).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn manifest_ignores_added_directories_but_refuses_missing_or_replaced_directories() {
        let root = crate::test_support::TempDir::new("restore-directory-manifest");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.subdir("source");
        fs::create_dir(source.join("original-empty")).unwrap();
        fs::write(source.join("a.txt"), b"authentic").unwrap();
        let session = root.join("1700000000_dir");
        let backed = back_up_fixture_directory(&source, &session);
        fs::remove_dir_all(&source).unwrap();
        fs::create_dir(backed.join("added-empty")).unwrap();
        fs::write(backed.join("added-empty/injected.txt"), b"unexpected").unwrap();
        let msgs = restore_session(&session).unwrap();
        assert!(msgs.iter().any(|m| m.starts_with("restored ")), "{msgs:?}");
        assert!(!source.join("added-empty").exists());
        assert!(source.join("original-empty").is_dir());
        assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"authentic");
        fs::remove_dir_all(&source).unwrap();
        fs::remove_dir(backed.join("original-empty")).unwrap();
        let msgs = restore_session(&session).unwrap();
        assert!(
            msgs.iter().any(|m| m.contains("missing recorded file")),
            "{msgs:?}"
        );
        assert!(!source.exists());
        fs::write(backed.join("original-empty"), b"wrong type").unwrap();
        let msgs = restore_session(&session).unwrap();
        assert!(
            msgs.iter()
                .any(|m| m.contains("type differs from the manifest")),
            "{msgs:?}"
        );
        assert!(!source.exists());

        let replaced = backed.join("original-empty");
        fs::remove_file(&replaced).unwrap();
        let outside = root.subdir("outside");
        let junction = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(&replaced)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(junction.status.success(), "{:?}", junction.stderr);
        let msgs = restore_session(&session).unwrap();
        assert!(
            msgs.iter().any(|m| m.starts_with("restore failed for ")),
            "{msgs:?}"
        );
        assert!(!source.exists());
        fs::remove_dir(&replaced).unwrap();
    }

    #[cfg(windows)]
    fn large_manifest_roundtrip(file_count: usize) {
        let root = crate::test_support::TempDir::new("restore-large-manifest");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let source = root.subdir("source");
        let file_name = |i| format!("{i:05}_long-but-ordinary-产品-payload-name.txt");
        for i in 0..file_count {
            fs::write(source.join(file_name(i)), b"x").unwrap();
        }
        let session = root.join("1700000000_dir");
        back_up_fixture_directory(&source, &session);
        let manifest_path = session.join(crate::path_seal::FILE_MANIFEST_NAME);
        assert!(manifest_path.metadata().unwrap().len() > 128 * 1024);
        let seal = crate::path_seal::verified_seal(&session).unwrap();
        assert!(seal.file_digests.is_none() && seal.file_manifest_digest.is_some());
        fs::remove_dir_all(&source).unwrap();
        let msgs = restore_session(&session).unwrap();
        assert!(msgs.iter().any(|m| m.starts_with("restored ")), "{msgs:?}");
        assert_eq!(fs::read_dir(&source).unwrap().count(), file_count);
        assert_eq!(
            fs::read(source.join(file_name(file_count - 1))).unwrap(),
            b"x"
        );
        fs::write(manifest_path, b"{}").unwrap();
        fs::write(source.join(file_name(0)), b"must remain").unwrap();
        assert_eq!(
            restore_session(&session).unwrap_err(),
            "seal:file_manifest_mismatch"
        );
        assert_eq!(fs::read(source.join(file_name(0))).unwrap(), b"must remain");
    }

    #[cfg(windows)]
    #[test]
    fn a_manifest_for_1500_small_files_roundtrips_beyond_the_seal_limit() {
        large_manifest_roundtrip(1500);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "explicit large backup and restore capacity check using 10,000 temporary files"]
    fn a_manifest_for_10000_small_files_roundtrips() {
        large_manifest_roundtrip(10_000);
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
