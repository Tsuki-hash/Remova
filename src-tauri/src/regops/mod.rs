//! Registry delete / value delete (real cleanup).

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SAM_FLAGS,
};

fn parse(key: &str) -> Option<(HKEY, String, REG_SAM_FLAGS)> {
    crate::fsutil::validate_native_name(key).ok()?;
    let (alias, rest) = key.split_once('\\')?;
    let rest = rest.to_string();
    match alias.to_uppercase().as_str() {
        "HKLM64" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_64KEY)),
        "HKLM32" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_32KEY)),
        "HKLM" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_64KEY)),
        "HKCU" => Some((HKEY_CURRENT_USER, rest, KEY_WOW64_64KEY)),
        _ => None,
    }
}

// Shared Windows string helper lives in `fsutil`.
#[cfg(windows)]
use crate::fsutil::{to_wide, to_wide_name};

/// Delete registry key tree. Caller must have run safety checks.
/// Opens the parent with the correct WOW64 view, then deletes the leaf via RegDeleteTreeW.
/// Parent needs DELETE + enumerate/query/set rights (MSDN RegDeleteTreeW).
/// intrinsic secondary gate — protected registry trees are refused here even if
/// the caller skipped `is_safe_to_delete_registry`.
///
/// `Ok(false)` means the key was already gone — the vendor uninstaller may have
/// removed it — which is not a failure and must not be reported as one.
pub fn delete_key(key_path: &str) -> Result<bool, String> {
    if let Err(e) = crate::safety::is_safe_to_delete_registry(key_path) {
        return Err(format!("registry delete blocked: {e}"));
    }
    #[cfg(not(windows))]
    {
        let _ = key_path;
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{
            KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_SET_VALUE,
        };
        // DELETE (0x00010000) is not exported by this windows crate build.
        const DELETE_RIGHT: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0001_0000);
        let (hive, sub, access) = parse(key_path).ok_or_else(|| "bad key".to_string())?;
        // A trailing separator makes the effective leaf empty — RegDeleteTreeW
        // with an empty subkey operates on the PARENT's contents. Refuse the
        // shape here as defense in depth behind the key gate.
        if sub.is_empty() || sub.ends_with('\\') {
            return Err("registry key leaf must not be empty".into());
        }
        unsafe {
            // Split into parent + leaf so RegDeleteTreeW can target the child under a view-aware handle.
            let (parent, leaf) = match sub.rsplit_once('\\') {
                Some((p, l)) => (p.to_string(), l.to_string()),
                None => (String::new(), sub.clone()),
            };
            let parent_w = if parent.is_empty() {
                Vec::new()
            } else {
                to_wide_name(&parent)?
            };
            let rights =
                DELETE_RIGHT | KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE | KEY_SET_VALUE | access;
            let leaf_w = to_wide_name(&leaf)?;
            let mut parent_hk = HKEY::default();
            let open = if parent.is_empty() {
                RegOpenKeyExW(hive, PCWSTR::null(), 0, rights, &mut parent_hk)
            } else {
                RegOpenKeyExW(hive, PCWSTR(parent_w.as_ptr()), 0, rights, &mut parent_hk)
            };
            // A parent that is already gone means the leaf is gone with it.
            if open == ERROR_FILE_NOT_FOUND || open == ERROR_PATH_NOT_FOUND {
                return Ok(false);
            }
            if open.is_err() {
                return Err(format!("open parent failed for {key_path}"));
            }
            let st = RegDeleteTreeW(parent_hk, PCWSTR(leaf_w.as_ptr()));
            let _ = RegCloseKey(parent_hk);
            if st == ERROR_FILE_NOT_FOUND || st == ERROR_PATH_NOT_FOUND {
                return Ok(false);
            }
            if st != ERROR_SUCCESS {
                return Err(format!("RegDeleteTreeW failed for {key_path}"));
            }
        }
        Ok(true)
    }
}

/// Delete a value under `key` (Run|Name).
/// intrinsic secondary gate — protected registry trees/values are refused here
/// even if the caller skipped `is_safe_to_delete_registry`.
///
/// `Ok(false)` means the key or the value was already gone, which is not a
/// failure and must not be reported as one.
pub fn delete_value(key_path: &str, value_name: &str) -> Result<bool, String> {
    let combined = if value_name.trim().is_empty() {
        key_path.to_string()
    } else {
        format!("{key_path}|{value_name}")
    };
    if let Err(e) = crate::safety::is_safe_to_delete_registry(&combined) {
        return Err(format!("registry delete blocked: {e}"));
    }
    #[cfg(not(windows))]
    {
        let _ = (key_path, value_name);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        let (hive, sub, access) = parse(key_path).ok_or_else(|| "bad key".to_string())?;
        let vw = to_wide_name(value_name)?;
        unsafe {
            let w = to_wide(&sub);
            let mut hk = HKEY::default();
            let opened =
                RegOpenKeyExW(hive, PCWSTR(w.as_ptr()), 0, KEY_SET_VALUE | access, &mut hk);
            // A key that is already gone takes its values with it.
            if opened == ERROR_FILE_NOT_FOUND || opened == ERROR_PATH_NOT_FOUND {
                return Ok(false);
            }
            opened.ok().map_err(|_| format!("open failed {key_path}"))?;
            let st = RegDeleteValueW(hk, PCWSTR(vw.as_ptr()));
            let _ = RegCloseKey(hk);
            if st == ERROR_FILE_NOT_FOUND || st == ERROR_PATH_NOT_FOUND {
                return Ok(false);
            }
            if st != ERROR_SUCCESS {
                return Err(format!("delete value failed {key_path}!{value_name}"));
            }
        }
        Ok(true)
    }
}

pub fn split_value_path(path: &str) -> Option<(&str, &str)> {
    let i = path.rfind('|')?;
    if i == 0 || i + 1 >= path.len() {
        return None;
    }
    Some((&path[..i], &path[i + 1..]))
}

/// Absolute path to a system tool (avoid PATH hijack and mutable
/// `SystemRoot` environment input). The OS API returns the trusted System32
/// directory even when the process environment is attacker-controlled.
pub fn system32_dir() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
        let mut buf = [0u16; 260];
        let n = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
        if n > 0 && n < buf.len() {
            return String::from_utf16_lossy(&buf[..n]);
        }
    }
    // API failure must not restore the untrusted environment fallback.
    String::new()
}

pub fn sys_tool(name: &str) -> String {
    if system32_dir().is_empty() {
        return String::new();
    }
    format!(r"{}\{name}", system32_dir())
}

pub fn explorer_exe() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
        let mut buf = [0u16; 260];
        let n = unsafe { GetWindowsDirectoryW(Some(&mut buf)) } as usize;
        if n > 0 && n < buf.len() {
            return format!(r"{}\explorer.exe", String::from_utf16_lossy(&buf[..n]));
        }
    }
    String::new()
}

/// CREATE_NO_WINDOW — avoid flashing a console for child tools (schtasks/sc/reg/…).
#[cfg(windows)]
pub fn hide_console(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut std::process::Command) {}

/// Stop or start a Windows service via sc.exe (process state, not Start type).
pub fn sc_set_service_running(svc_name: &str, run: bool) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = (svc_name, run);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        if svc_name.is_empty() || svc_name.contains('\\') || svc_name.contains('"') {
            return Err(crate::error::manage_err("bad_name", "service name").to_ipc());
        }
        // Intrinsic gate: a service the write gate protects must not be
        // stopped/started through a caller that skips manage.rs either.
        if crate::safety::is_protected_service_name(svc_name) {
            return Err(crate::error::manage_err("protected", svc_name).to_ipc());
        }
        use std::process::Command;
        let sc = sys_tool("sc.exe");
        let arg = if run { "start" } else { "stop" };
        let mut cmd = Command::new(&sc);
        cmd.args([arg, svc_name]);
        hide_console(&mut cmd);
        match cmd.output() {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => {
                let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
                let msg = if msg.is_empty() {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                } else {
                    msg
                };
                Err(msg)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Best-effort: stop/delete Windows service via sc.exe.
pub fn sc_delete_service(svc_name: &str) -> bool {
    if svc_name.is_empty() || svc_name.contains('\\') || svc_name.contains('"') {
        return false;
    }
    // Intrinsic gate: critical or Microsoft-family services are never
    // natively deleted, even if a future caller routes around the pipeline's
    // prefix anchoring — the write gate refuses the same family.
    if crate::safety::is_protected_service_name(svc_name) {
        return false;
    }
    use std::process::Command;
    let sc = sys_tool("sc.exe");
    let mut stop = Command::new(&sc);
    stop.args(["stop", svc_name]);
    hide_console(&mut stop);
    let _ = stop.output();
    let mut del = Command::new(&sc);
    del.args(["delete", svc_name]);
    hide_console(&mut del);
    let out = del.output();
    matches!(out, Ok(o) if o.status.success())
}

/// Best-effort: schtasks /delete for a task path or leaf name.
/// Prefer full TaskCache tree remainder (`\Vendor\Foo\Task`) when provided.
pub fn schtasks_delete(task_name: &str) -> bool {
    if task_name.trim().is_empty()
        || task_name
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '*' | '?'))
    {
        return false;
    }
    // Intrinsic gate: the whole \Microsoft\ tree is system-managed — the same
    // rule the enable/disable gate enforces. Native deletion stays best-effort
    // for vendor tasks only.
    if task_name
        .to_uppercase()
        .replace('/', "\\")
        .starts_with("\\MICROSOFT\\")
    {
        return false;
    }
    use std::process::Command;
    let mut cmd = Command::new(sys_tool("schtasks.exe"));
    cmd.args(["/delete", "/tn", task_name, "/f"]);
    hide_console(&mut cmd);
    let out = cmd.output();
    matches!(out, Ok(o) if o.status.success())
}

/// Normalize a PATH segment for exact comparison.
///
/// Delegates to `fsutil::path_key`, the single definition of path identity, so a
/// PATH entry compares the same way the red-line checks and the allow-lists
/// compare. This used to be a fourth private copy — identical to the two
/// allow-list normalizers, and like them missing Win32's per-segment trailing
/// dots and spaces, so `C:\Vendor\Tool.` in PATH did not match the entry the
/// scanner reported. `None` for an entry that cannot be named.
fn normalize_path_entry(s: &str) -> Option<String> {
    crate::fsutil::path_key(std::path::Path::new(s))
}

/// True if PATH still contains this exact entry (verify checklist).
pub fn scrub_path_entry_ok(entry: &str) -> bool {
    path_entry_presence(entry).unwrap_or(false)
}

pub(crate) fn path_entry_presence(entry: &str) -> Result<bool, String> {
    path_entry_presence_with(entry, read_path_scope)
}

fn path_entry_presence_with(
    entry: &str,
    mut read: impl FnMut(&str) -> Result<String, String>,
) -> Result<bool, String> {
    let Some(needle) = normalize_path_entry(entry) else {
        return Ok(false);
    };
    let mut error = None;
    for scope in ["User", "Machine"] {
        match read(scope) {
            Ok(current) => {
                if current
                    .split(';')
                    .filter_map(normalize_path_entry)
                    .any(|p| p == needle)
                {
                    return Ok(true);
                }
            }
            Err(e) => error = Some(e),
        }
    }
    error.map_or(Ok(false), Err)
}

/// Whether a PATH string already contains `entry` (normalized exact match).
pub fn path_contains_entry(path_value: &str, entry: &str) -> bool {
    let Some(needle) = normalize_path_entry(entry) else {
        return false;
    };
    path_value
        .split(';')
        .filter_map(normalize_path_entry)
        .any(|p| p == needle)
}

/// Append `entry` to a PATH value if missing. Returns the new value, or None if already present.
pub fn merge_path_entry(path_value: &str, entry: &str) -> Option<String> {
    let trimmed = entry.trim().trim_matches('"');
    if trimmed.is_empty() || path_contains_entry(path_value, entry) {
        return None;
    }
    let parts: Vec<&str> = path_value
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let mut out = parts;
    out.push(trimmed);
    Some(out.join(";"))
}

/// Restore one PATH segment into the given scopes when missing (Safety Vault).
/// Returns Ok(true) if at least one scope was updated.
/// empty scopes restore to User only — never silently write Machine PATH.
pub fn restore_path_entry(entry: &str, scopes: &[&str]) -> Result<bool, String> {
    // symmetric gate with scrub_path_entry — the entry comes from
    // the session's user-writable path.json and must never be a dangerous one.
    if crate::policy::is_dangerous_path_entry(entry) {
        return Err("protected PATH entry".into());
    }
    let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut changed = false;
    let mut errors: Vec<String> = vec![];
    let targets: Vec<&str> = if scopes.is_empty() {
        vec!["User"]
    } else {
        scopes.to_vec()
    };
    for scope in targets {
        // Aggregate per-scope errors instead of aborting on the first one: a
        // User write that already succeeded must still broadcast, and the
        // caller must learn about the partial failure.
        let outcome = (|| -> Result<bool, String> {
            let (raw, expand) = read_path_scope_raw(scope)?;
            // Presence is judged on the expanded form; the raw bytes are what get
            // written back, so `%VAR%` segments survive a REG_EXPAND_SZ restore.
            let current = if expand {
                expand_env_string(&raw)
            } else {
                raw.clone()
            };
            if path_contains_entry(&current, entry) {
                return Ok(false);
            }
            let trimmed = entry.trim().trim_matches('"');
            if trimmed.is_empty() {
                return Ok(false);
            }
            let next = if raw.trim().is_empty() {
                trimmed.to_string()
            } else {
                format!("{raw};{trimmed}")
            };
            write_path_scope(scope, &next, expand)?;
            Ok(true)
        })();
        match outcome {
            Ok(wrote) => changed |= wrote,
            Err(e) => errors.push(format!("{scope}: {e}")),
        }
    }
    if changed {
        broadcast_env_change();
    }
    if errors.is_empty() {
        Ok(changed)
    } else {
        Err(errors.join("; "))
    }
}

/// Export a single registry value to a .reg file ( value-level restore).
/// `key_path` uses Remova aliases (HKCU / HKLM64 / HKLM32). Best-effort: Ok(false) if missing.
pub fn export_reg_value(
    key_path: &str,
    value_name: &str,
    dest: &std::path::Path,
) -> Result<bool, String> {
    if value_name.trim().is_empty() {
        return Err("empty value name".into());
    }
    // Value names are interpolated into .reg text — reject quote/newline injection.
    if value_name.contains('"') || value_name.contains('\n') || value_name.contains('\r') {
        return Err("unsafe value name".into());
    }
    let (alias, rest) = key_path
        .split_once('\\')
        .ok_or_else(|| "bad key".to_string())?;
    // key rest is interpolated into `[...]` — reject injection chars.
    if rest.is_empty() || rest.contains(['\r', '\n', ']']) {
        return Err("unsafe key path".into());
    }
    let (hive, view) = match alias.to_uppercase().as_str() {
        "HKLM64" | "HKLM" => ("HKEY_LOCAL_MACHINE", "/reg:64"),
        "HKLM32" => ("HKEY_LOCAL_MACHINE", "/reg:32"),
        "HKCU" => ("HKEY_CURRENT_USER", "/reg:64"),
        other => return Err(format!("unsupported hive {other}")),
    };
    let key_win = format!(r"{hive}\{rest}");
    let Some((native_bytes, native_type)) = read_reg_value_bytes(key_path, value_name)? else {
        return Ok(false);
    };
    let value_name = value_name.replace('\\', "\\\\");
    let reg_type = match native_type {
        1 => "REG_SZ",
        2 => "REG_EXPAND_SZ",
        3 => "REG_BINARY",
        4 => "REG_DWORD",
        7 => "REG_MULTI_SZ",
        11 => "REG_QWORD",
        _ => return Err("unsupported registry value type".into()),
    };
    let data_raw = match native_type {
        4 => format!(
            "0x{:x}",
            u32::from_le_bytes(
                native_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "invalid DWORD length")?
            )
        ),
        11 => format!(
            "0x{:x}",
            u64::from_le_bytes(
                native_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "invalid QWORD length")?
            )
        ),
        _ => native_bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
    };
    let key_reg = format!("[{key_win}]");
    let body = match reg_type {
        "REG_DWORD" => {
            // data like 0x2 — parse failure must not silently become 0.
            let n = u32::from_str_radix(data_raw.trim_start_matches("0x"), 16)
                .or_else(|_| data_raw.trim().parse::<u32>())
                .map_err(|_| format!("export_reg_value: bad DWORD data '{data_raw}'"))?;
            format!("\"{value_name}\"=dword:{n:08x}")
        }
        "REG_QWORD" => {
            let n = u64::from_str_radix(data_raw.trim_start_matches("0x"), 16)
                .map_err(|_| format!("export_reg_value: bad QWORD data '{data_raw}'"))?;
            let bytes = n.to_le_bytes();
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!("\"{value_name}\"=hex(b):{}", hex.join(","))
        }
        "REG_MULTI_SZ" => {
            // reg.exe text output joins items with spaces (lossy, type lost) —
            // read the raw UTF-16LE bytes and emit hex(7) so item structure
            // survives restore.
            let mut payload = native_bytes.clone();
            while payload.ends_with(&[0, 0]) {
                payload.truncate(payload.len() - 2);
            }
            // items are NUL-terminated; the list ends with an extra NUL.
            payload.extend_from_slice(&[0, 0, 0, 0]);
            let hex: Vec<String> = payload.iter().map(|b| format!("{b:02x}")).collect();
            format!("\"{value_name}\"=hex(7):{}", hex.join(","))
        }
        "REG_BINARY" => {
            // reg query prints hex bytes space-separated
            let hex: Vec<String> = data_raw
                .split_whitespace()
                .map(|s| s.trim_start_matches("0x").to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            format!("\"{value_name}\"=hex:{}", hex.join(","))
        }
        "REG_SZ" | "REG_EXPAND_SZ" => {
            // reg.exe text output is OEM-codepage encoded and mangles
            // non-ASCII REG_SZ data — and value.reg is the seal-digested
            // restore source. Read the raw UTF-16LE bytes natively and emit
            // hex the import restores byte-exact (hex(1)=REG_SZ,
            // hex(2)=REG_EXPAND_SZ).
            let kind = if reg_type == "REG_EXPAND_SZ" {
                "hex(2)"
            } else {
                "hex(1)"
            };
            let bytes = &native_bytes;
            // registry strings arrive NUL-terminated — normalize to exactly one.
            let mut core = bytes.as_slice();
            while core.len() >= 2 && core[core.len() - 2..] == [0, 0] {
                core = &core[..core.len() - 2];
            }
            let mut payload = core.to_vec();
            payload.extend_from_slice(&[0, 0]);
            let hex: Vec<String> = payload.iter().map(|b| format!("{b:02x}")).collect();
            format!("\"{value_name}\"={kind}:{}", hex.join(","))
        }
        _ => {
            // Unknown string-ish types — previous text-based escape path.
            let esc = data_raw.replace('\\', "\\\\").replace('"', "\\\"");
            if reg_type == "REG_EXPAND_SZ" {
                format!("\"{value_name}\"=hex(2):{}", utf16_hex_expand(&data_raw))
            } else {
                format!("\"{value_name}\"=\"{esc}\"")
            }
        }
    };
    let reg = format!("Windows Registry Editor Version 5.00\r\n; Remova registry view: {}\r\n\r\n{key_reg}\r\n{body}\r\n", view.trim_start_matches("/reg:"));
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(dest, crate::restore::encode_reg_text(&reg)).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Parse `reg query` output for a single value: locate the known-type token
/// (a value NAME containing "REG_" must not confuse it), then accumulate
/// multi-line hex continuations until the next key header — long REG_BINARY
/// data used to be silently truncated at the first line (data corruption in
/// the backup safety net). Returns (type, data) or None.
#[cfg(test)]
fn parse_reg_query(text: &str) -> Option<(String, String)> {
    const KNOWN: &[&str] = &[
        "REG_SZ",
        "REG_EXPAND_SZ",
        "REG_BINARY",
        "REG_DWORD",
        "REG_QWORD",
        "REG_MULTI_SZ",
    ];
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("HKEY_") {
            continue;
        }
        let mut hit: Option<(usize, usize, &str)> = None;
        for ty in KNOWN {
            if let Some(idx) = t.find(ty) {
                let before_ok = idx == 0 || t[..idx].ends_with(char::is_whitespace);
                let after = &t[idx + ty.len()..];
                let after_ok = after.is_empty() || after.starts_with(char::is_whitespace);
                if before_ok && after_ok {
                    hit = Some((idx, ty.len(), ty));
                    break;
                }
            }
        }
        let Some((idx, ty_len, ty)) = hit else {
            continue;
        };
        let mut data = t[idx + ty_len..].trim().to_string();
        for cont in lines.by_ref() {
            let ct = cont.trim();
            if ct.is_empty() || ct.starts_with("HKEY_") || ct.starts_with('[') {
                break;
            }
            if !data.is_empty() {
                data.push(' ');
            }
            data.push_str(ct);
        }
        return Some((ty.to_string(), data));
    }
    None
}

fn utf16_hex_expand(s: &str) -> String {
    let mut bytes: Vec<String> = Vec::new();
    for u in s.encode_utf16() {
        bytes.push(format!("{:02x}", u & 0xff));
        bytes.push(format!("{:02x}", (u >> 8) & 0xff));
    }
    bytes.push("00".into());
    bytes.push("00".into());
    bytes.join(",")
}

static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Remove one PATH segment from User and Machine environments (exact match only).
/// Returns Ok(true) if at least one scope changed.
/// intrinsic secondary gate — system PATH segments are refused here even if
/// the caller skipped `is_dangerous_path_entry`.
pub fn scrub_path_entry(entry: &str) -> Result<bool, String> {
    if crate::policy::is_dangerous_path_entry(entry) {
        return Err("protected PATH entry".into());
    }
    let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(needle) = normalize_path_entry(entry) else {
        return Err("empty path entry".into());
    };
    let mut changed = false;
    let mut errors: Vec<String> = vec![];
    for scope in ["User", "Machine"] {
        // Aggregate per-scope errors: a User scrub that already succeeded must
        // still broadcast, and Machine failures must not hide the partial win.
        let outcome = (|| -> Result<bool, String> {
            let (raw, expand) = read_path_scope_raw(scope)?;
            // Match on the expanded form, keep the raw segment text: a
            // REG_EXPAND_SZ value must not come back with `%VAR%` baked in.
            let mut kept: Vec<&str> = Vec::new();
            let mut hit = false;
            for seg in raw.split(';') {
                let expanded = if expand {
                    expand_env_string(seg)
                } else {
                    seg.to_string()
                };
                if normalize_path_entry(&expanded) == Some(needle.clone())
                    || normalize_path_entry(seg) == Some(needle.clone())
                {
                    hit = true;
                    continue;
                }
                kept.push(seg);
            }
            if !hit {
                return Ok(false);
            }
            write_path_scope(scope, &kept.join(";"), expand)?;
            Ok(true)
        })();
        match outcome {
            Ok(wrote) => changed |= wrote,
            Err(e) => errors.push(format!("{scope}: {e}")),
        }
    }
    if changed {
        broadcast_env_change();
    }
    if errors.is_empty() {
        Ok(changed)
    } else {
        Err(errors.join("; "))
    }
}

/// Notify Explorer/new processes that PATH changed (WM_SETTINGCHANGE).
fn broadcast_env_change() {
    #[cfg(windows)]
    {
        use std::process::Command;
        let mut cmd = Command::new(powershell_exe());
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Add-Type -Namespace W -Name N -MemberDefinition '[DllImport(\"user32.dll\", SetLastError=true, CharSet=CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'; [UIntPtr]$r=0; [void][W.N]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$r)",
        ]);
        hide_console(&mut cmd);
        let _ = cmd.output();
    }
}

fn powershell_exe() -> String {
    if system32_dir().is_empty() {
        return String::new();
    }
    format!(r"{}\WindowsPowerShell\v1.0\powershell.exe", system32_dir())
}

/// Test-only PATH I/O mock : production path is untouched when inactive.
#[cfg(test)]
pub(crate) mod path_mock {
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Stored as (raw value, is REG_EXPAND_SZ).
    static STATE: Mutex<Option<HashMap<String, (String, bool)>>> = Mutex::new(None);
    static FAIL_READ: Mutex<bool> = Mutex::new(false);
    static FAIL_SCOPE_READ: Mutex<Option<String>> = Mutex::new(None);
    static FAIL_MACHINE_WRITE: Mutex<bool> = Mutex::new(false);

    fn put(user: &str, machine: &str, expand: bool) {
        let mut map = HashMap::new();
        map.insert("User".to_string(), (user.to_string(), expand));
        map.insert("Machine".to_string(), (machine.to_string(), expand));
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(map);
        *FAIL_READ.lock().unwrap_or_else(|e| e.into_inner()) = false;
        *FAIL_SCOPE_READ.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn install(user: &str, machine: &str) {
        put(user, machine, false);
    }

    /// Install REG_EXPAND_SZ scopes: `read_raw` yields these bytes verbatim,
    /// `read` yields them expanded through the process environment.
    pub fn install_expand(user: &str, machine: &str) {
        put(user, machine, true);
    }

    #[allow(dead_code)]
    pub fn set_fail_read(v: bool) {
        *FAIL_READ.lock().unwrap_or_else(|e| e.into_inner()) = v;
    }

    pub fn set_fail_scope_read(scope: Option<&str>) {
        *FAIL_SCOPE_READ.lock().unwrap_or_else(|e| e.into_inner()) = scope.map(str::to_string);
    }

    pub fn clear() {
        *FAIL_MACHINE_WRITE.lock().unwrap_or_else(|e| e.into_inner()) = false;
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *FAIL_SCOPE_READ.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *FAIL_READ.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }

    /// Clears the mock on drop, so a failing assertion cannot leave
    /// `FAIL_READ`/fixture PATH installed for the rest of the test binary (A5).
    pub struct ResetGuard;

    impl Drop for ResetGuard {
        fn drop(&mut self) {
            clear();
        }
    }

    pub fn reset_on_drop() -> ResetGuard {
        ResetGuard
    }

    pub fn active() -> bool {
        STATE.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    pub fn read_raw(scope: &str) -> Result<(String, bool), String> {
        if *FAIL_READ.lock().unwrap_or_else(|e| e.into_inner())
            || FAIL_SCOPE_READ
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_deref()
                == Some(scope)
        {
            return Err(format!("read Path {scope} failed"));
        }
        let guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.as_ref().expect("path mock not installed");
        Ok(map.get(scope).cloned().unwrap_or((String::new(), false)))
    }

    pub fn write(scope: &str, value: &str, expand: bool) -> Result<(), String> {
        if scope == "Machine" && *FAIL_MACHINE_WRITE.lock().unwrap_or_else(|e| e.into_inner()) {
            return Err("injected Machine write failure".into());
        }
        let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.as_mut().expect("path mock not installed");
        map.insert(scope.to_string(), (value.to_string(), expand));
        Ok(())
    }

    pub fn get(scope: &str) -> String {
        STATE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|m| m.get(scope))
            .map(|(raw, _)| raw.clone())
            .unwrap_or_default()
    }

    pub fn fail_machine_write() {
        *FAIL_MACHINE_WRITE.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }

    /// Serialize tests that install/use/clear the process-wide PATH mock.
    pub fn lock_mock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Test-only `%VAR%` fallback source — deterministic resolution for variables
/// the (test) process environment does not define. Consulted by
/// [`fallback_var`] before the registry.
#[cfg(test)]
pub(crate) mod expand_mock {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static MAP: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

    pub fn install(entries: &[(&str, &str)]) {
        let mut m = HashMap::new();
        for (k, v) in entries {
            m.insert((*k).to_string(), (*v).to_string());
        }
        *MAP.lock().unwrap_or_else(|e| e.into_inner()) = Some(m);
    }

    pub fn get(name: &str) -> Option<String> {
        MAP.lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|m| m.get(name).cloned())
    }

    pub fn clear() {
        *MAP.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn read_path_scope(scope: &str) -> Result<String, String> {
    let (raw, expand) = read_path_scope_raw(scope)?;
    Ok(if expand { expand_env_string(&raw) } else { raw })
}

/// Raw Path text plus its type (`true` = REG_EXPAND_SZ, unexpanded bytes).
fn read_path_scope_raw(scope: &str) -> Result<(String, bool), String> {
    #[cfg(test)]
    {
        if path_mock::active() {
            return path_mock::read_raw(scope);
        }
    }
    // read Path straight from the registry — no PowerShell round-trip
    // (was two processes per analyze) and no OEM-codepage `from_utf8_lossy`
    // corruption of non-ASCII PATH entries.
    let key = match scope {
        "User" => r"HKCU\Environment",
        "Machine" => r"HKLM64\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        _ => return Err(crate::error::path_io_err(format!("unknown Path scope {scope}")).to_ipc()),
    };
    read_reg_value_raw_with_missing(key, "Path", true)
}

/// Raw Path text plus its type (`true` = REG_EXPAND_SZ). Write-back must
/// preserve these unexpanded bytes for REG_EXPAND_SZ values.
fn read_reg_value_raw(key_path: &str, value_name: &str) -> Result<(String, bool), String> {
    read_reg_value_raw_with_missing(key_path, value_name, false)
}

fn read_reg_value_raw_with_missing(
    key_path: &str,
    value_name: &str,
    missing_is_empty: bool,
) -> Result<(String, bool), String> {
    #[cfg(not(windows))]
    {
        let _ = (key_path, value_name, missing_is_empty);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{
            RegQueryValueExW, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
        };
        let (hive, sub, access) = parse(key_path).ok_or_else(|| "bad key".to_string())?;
        unsafe {
            let w = to_wide(&sub);
            let mut hk = HKEY::default();
            let opened = RegOpenKeyExW(hive, PCWSTR(w.as_ptr()), 0, KEY_READ | access, &mut hk);
            if missing_is_empty
                && (opened == ERROR_FILE_NOT_FOUND || opened == ERROR_PATH_NOT_FOUND)
            {
                return Ok((String::new(), true));
            }
            opened.ok().map_err(|_| {
                crate::error::path_io_err(format!("open failed {key_path}")).to_ipc()
            })?;
            let vname = to_wide(value_name);
            let mut typ = REG_VALUE_TYPE(0);
            let mut data = vec![0u8; 65_536];
            let mut data_len = data.len() as u32;
            let mut st = RegQueryValueExW(
                hk,
                PCWSTR(vname.as_ptr()),
                None,
                Some(&mut typ),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            if st == ERROR_MORE_DATA {
                // re-query with the reported size — long PATH
                // values must not fail the whole chain.
                let need = data_len as usize;
                if need > data.len() && need <= (1 << 20) {
                    data = vec![0u8; need];
                    data_len = data.len() as u32;
                    st = RegQueryValueExW(
                        hk,
                        PCWSTR(vname.as_ptr()),
                        None,
                        Some(&mut typ),
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    );
                }
            }
            let _ = RegCloseKey(hk);
            if missing_is_empty && (st == ERROR_FILE_NOT_FOUND || st == ERROR_PATH_NOT_FOUND) {
                return Ok((String::new(), true));
            }
            if st != ERROR_SUCCESS {
                return Err(
                    crate::error::path_io_err(format!("read Path {key_path} failed")).to_ipc(),
                );
            }
            if typ != REG_SZ && typ != REG_EXPAND_SZ {
                return Err(crate::error::path_io_err(format!(
                    "unexpected type for Path {key_path}"
                ))
                .to_ipc());
            }
            // Strict decode for the read-modify-write path: a lossy decode
            // would bake U+FFFD into the registry value on write-back.
            decode_reg_utf16(&data[..data_len as usize]).map(|text| (text, typ == REG_EXPAND_SZ))
        }
    }
}

/// Decode NUL-terminated UTF-16LE registry string data — strictly. Unpaired
/// surrogates are an error so the read-modify-write path never rewrites a
/// value containing replacement characters.
fn decode_reg_utf16(data: &[u8]) -> Result<String, String> {
    let wide: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut text = String::from_utf16(&wide)
        .map_err(|_| "registry string data is not valid UTF-16".to_string())?;
    while text.ends_with('\0') {
        text.pop();
    }
    Ok(text)
}

/// Raw registry value bytes plus the native type code. reg.exe text output is
/// OEM-codepage encoded and mangles non-ASCII string data — the seal-digested
/// .reg exporter must carry the exact registry bytes instead.
#[cfg(not(windows))]
fn read_reg_value_bytes(
    _key_path: &str,
    _value_name: &str,
) -> Result<Option<(Vec<u8>, u32)>, String> {
    Err("not windows".into())
}

#[cfg(windows)]
fn read_reg_value_bytes(
    key_path: &str,
    value_name: &str,
) -> Result<Option<(Vec<u8>, u32)>, String> {
    use windows::Win32::System::Registry::{RegQueryValueExW, REG_VALUE_TYPE};
    let (hive, sub, access) = parse(key_path).ok_or_else(|| "bad key".to_string())?;
    unsafe {
        let w = to_wide(&sub);
        let mut hk = HKEY::default();
        let opened = RegOpenKeyExW(hive, PCWSTR(w.as_ptr()), 0, KEY_READ | access, &mut hk);
        // A key that is not there has no value to read. Anything else — access
        // denied above all — is a failure the caller must see: reporting it as
        // "the value does not exist" is what let a `.reg` export silently drop a
        // value and leave the Safety Vault with nothing to restore from.
        if opened == ERROR_FILE_NOT_FOUND || opened == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        opened.ok().map_err(|_| format!("open failed {key_path}"))?;
        let vname = to_wide(value_name);
        let mut typ = REG_VALUE_TYPE(0);
        let mut data = vec![0u8; 16 * 1024];
        let mut data_len = data.len() as u32;
        let mut st = RegQueryValueExW(
            hk,
            PCWSTR(vname.as_ptr()),
            None,
            Some(&mut typ),
            Some(data.as_mut_ptr()),
            Some(&mut data_len),
        );
        if st == ERROR_MORE_DATA {
            let need = data_len as usize;
            if need > data.len() && need <= (1 << 20) {
                data = vec![0u8; need];
                data_len = data.len() as u32;
                st = RegQueryValueExW(
                    hk,
                    PCWSTR(vname.as_ptr()),
                    None,
                    Some(&mut typ),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                );
            }
        }
        let _ = RegCloseKey(hk);
        if st == ERROR_FILE_NOT_FOUND || st == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        if st != ERROR_SUCCESS {
            return Err(format!("read value {key_path}!{value_name} failed"));
        }
        data.truncate(data_len as usize);
        Ok(Some((data, typ.0)))
    }
}

/// Expand `%VAR%` references using the current environment block. Variables
/// the process environment cannot resolve fall back to the registry
/// environment (HKCU user vars, then HKLM machine vars) — service/SYSTEM or
/// stripped-env processes otherwise leave variables literal, and the
/// REG_EXPAND_SZ dedup/restore path would mis-judge what is "already there".
pub(crate) fn expand_env_string(s: &str) -> String {
    #[cfg(not(windows))]
    {
        s.to_string()
    }
    #[cfg(windows)]
    {
        let expanded = api_expand(s);
        resolve_unresolved_vars(&expanded, 0)
    }
}

/// Windows API expansion against the process environment block.
#[cfg(windows)]
fn api_expand(s: &str) -> String {
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    if !s.contains('%') {
        return s.to_string();
    }
    let wide = to_wide(s);
    unsafe {
        let needed = ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), None);
        if needed == 0 {
            return s.to_string();
        }
        let mut buf = vec![0u16; needed as usize];
        let written = ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), Some(&mut buf));
        if written == 0 {
            return s.to_string();
        }
        while buf.last().copied() == Some(0) {
            buf.pop();
        }
        String::from_utf16_lossy(&buf)
    }
}

/// Resolve `%VAR%` tokens the API left literal. Depth-capped: registry values
/// may themselves be REG_EXPAND_SZ. Unresolvable tokens stay verbatim — an
/// entry whose value no source defines is genuinely unknowable.
fn resolve_unresolved_vars(s: &str, depth: usize) -> String {
    if depth >= 5 || !s.contains('%') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                out.push_str(&rest[..start]);
                let name = &after[..end];
                match fallback_var(name, depth) {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&rest[start..start + end + 2]),
                }
                rest = &after[end + 1..];
            }
            _ => {
                // no closing `%` — a literal percent, push the remainder as-is
                out.push_str(rest);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    let joined = out;
    if joined != s {
        // a resolved value may itself reference further variables
        resolve_unresolved_vars(&joined, depth + 1)
    } else {
        joined
    }
}

fn fallback_var(name: &str, depth: usize) -> Option<String> {
    #[cfg(test)]
    if let Some(v) = expand_mock::get(name) {
        return Some(v);
    }
    registry_env_var(name, depth)
}

/// Registry-defined environment variables — the machine's truth when the
/// process env lacks them (user vars first, then machine vars).
fn registry_env_var(name: &str, depth: usize) -> Option<String> {
    #[cfg(not(windows))]
    {
        let _ = name;
        let _ = depth;
        None
    }
    #[cfg(windows)]
    {
        for key in [
            r"HKCU\Environment",
            r"HKLM64\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ] {
            let Ok((text, expand)) = read_reg_value_raw(key, name) else {
                continue;
            };
            return Some(if expand {
                resolve_unresolved_vars(&api_expand(&text), depth + 1)
            } else {
                text
            });
        }
        None
    }
}

/// Public wrapper so the scanner can read true User/Machine PATH.
pub fn read_path_scope_public(scope: &str) -> Result<String, String> {
    read_path_scope(scope)
}

pub(crate) fn write_path_scope(scope: &str, value: &str, expand: bool) -> Result<(), String> {
    #[cfg(test)]
    {
        if path_mock::active() {
            return path_mock::write(scope, value, expand);
        }
    }
    if value.encode_utf16().count() > 30_000 {
        return Err(crate::error::path_io_err(
            "PATH value exceeds the environment variable size limit".to_string(),
        )
        .to_ipc());
    }
    let key = match scope {
        "User" => r"HKCU\Environment",
        "Machine" => r"HKLM64\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        _ => return Err(crate::error::path_io_err(format!("unknown Path scope {scope}")).to_ipc()),
    };
    write_path_value(key, value, expand)
}

fn write_path_value(key: &str, value: &str, expand: bool) -> Result<(), String> {
    crate::fsutil::validate_native_name(value)?;
    #[cfg(not(windows))]
    {
        let _ = (key, expand);
        Err("not windows".into())
    }
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Registry::{
            RegCreateKeyExW, RegSetValueExW, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ,
        };
        let (hive, sub, view) = parse(key).ok_or_else(|| "bad key".to_string())?;
        let sub = to_wide_name(&sub)?;
        let mut hk = HKEY::default();
        RegCreateKeyExW(
            hive,
            PCWSTR(sub.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | view,
            None,
            &mut hk,
            None,
        )
        .ok()
        .map_err(|e| crate::error::path_io_err(format!("open Path key failed: {e}")).to_ipc())?;
        let bytes: Vec<u8> = to_wide(value)
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let name = to_wide("Path");
        let status = RegSetValueExW(
            hk,
            PCWSTR(name.as_ptr()),
            0,
            if expand { REG_EXPAND_SZ } else { REG_SZ },
            Some(&bytes),
        );
        let _ = RegCloseKey(hk);
        status
            .ok()
            .map_err(|e| crate::error::path_io_err(format!("write Path failed: {e}")).to_ipc())
    }
}

pub fn leaf_name(path: &str) -> String {
    path.rsplit('\\').next().unwrap_or("").to_string()
}

/// Rename a registry value (copy data + delete old) under `key`.
pub fn rename_reg_value(key_path: &str, from: &str, to: &str) -> Result<(), String> {
    crate::fsutil::validate_native_name(from)?;
    crate::fsutil::validate_native_name(to)?;
    // intrinsic target gate — callers gate too, primitives enforce last.
    // Renames never target Services keys, so no value name is offered
    // (the Services branch of the gate only whitelists the Start value).
    crate::safety::allow_reg_value_write(key_path, None)?;
    if from.eq_ignore_ascii_case(to) {
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let _ = (key_path, from, to);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{RegQueryValueExW, RegSetValueExW, REG_VALUE_TYPE};
        let (hive, sub, access) = parse(key_path).ok_or_else(|| "bad key".to_string())?;
        let from_w = to_wide_name(from)?;
        let to_w = to_wide_name(to)?;
        unsafe {
            let w = to_wide(&sub);
            let mut hk = HKEY::default();
            RegOpenKeyExW(
                hive,
                PCWSTR(w.as_ptr()),
                0,
                KEY_READ | KEY_SET_VALUE | access,
                &mut hk,
            )
            .ok()
            .map_err(|_| format!("open failed {key_path}"))?;
            let mut typ = REG_VALUE_TYPE(0);
            let mut data = vec![0u8; 16 * 1024];
            let mut data_len = data.len() as u32;
            let mut st = RegQueryValueExW(
                hk,
                PCWSTR(from_w.as_ptr()),
                None,
                Some(&mut typ),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            // values above the initial buffer must be readable, like
            // `read_reg_value_bytes` — otherwise a rename fails forever.
            while st == ERROR_MORE_DATA && data_len as usize > data.len() {
                let need = data_len as usize;
                if need > (1 << 20) {
                    break;
                }
                data = vec![0u8; need];
                data_len = data.len() as u32;
                st = RegQueryValueExW(
                    hk,
                    PCWSTR(from_w.as_ptr()),
                    None,
                    Some(&mut typ),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                );
            }
            if st != ERROR_SUCCESS {
                let _ = RegCloseKey(hk);
                return Err(format!("query failed {from}"));
            }
            let existing = RegQueryValueExW(hk, PCWSTR(to_w.as_ptr()), None, None, None, None);
            if existing.0 != 2 {
                let _ = RegCloseKey(hk);
                return Err("registry rename destination exists or cannot be checked".into());
            }
            let st = RegSetValueExW(
                hk,
                PCWSTR(to_w.as_ptr()),
                0,
                typ,
                Some(&data[..data_len as usize]),
            );
            if st == ERROR_SUCCESS {
                let deleted = RegDeleteValueW(hk, PCWSTR(from_w.as_ptr()));
                if deleted != ERROR_SUCCESS {
                    let _ = RegCloseKey(hk);
                    return Err("registry rename copied but source deletion failed".into());
                }
            }
            let _ = RegCloseKey(hk);
            if st != ERROR_SUCCESS {
                return Err(format!("set failed {to}"));
            }
        }
        Ok(())
    }
}

/// Write REG_SZ under `key_path` (creates key tree via `reg add` fallback).
pub(crate) fn normalize_reg_exe_hive(key_path: &str) -> String {
    let (alias, rest) = match key_path.split_once('\\') {
        Some(p) => p,
        None => return key_path.to_string(),
    };
    let alias_up = alias.to_uppercase();
    let hive = match alias_up.as_str() {
        "HKLM64" | "HKLM32" | "HKLM" => "HKLM",
        "HKCU" => "HKCU",
        other => other,
    };
    // Only SOFTWARE is redirected; WOW6432Node belongs beneath that segment.
    if alias_up == "HKLM32"
        && rest
            .get(..9)
            .is_some_and(|s| s.eq_ignore_ascii_case("SOFTWARE\\"))
    {
        let tail = &rest[9..];
        if !tail.eq_ignore_ascii_case("WOW6432NODE")
            && !tail.to_uppercase().starts_with("WOW6432NODE\\")
        {
            return format!(r"HKLM\{}\WOW6432Node\{tail}", &rest[..8]);
        }
    }
    format!("{hive}\\{rest}")
}

pub fn create_reg_sz(key_path: &str, value_name: &str, data: &str) -> Result<(), String> {
    // intrinsic target gate — callers gate too, primitives enforce last.
    crate::safety::allow_reg_value_write(key_path, Some(value_name))?;
    #[cfg(not(windows))]
    {
        let _ = (key_path, value_name, data);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        // Prefer reg.exe for reliable key creation under HKCU\Software\Classes\*\shell
        // Translate Remova hive aliases (HKLM64/32 → HKLM / WOW6432NODE) for reg.exe.
        let exe_key = normalize_reg_exe_hive(key_path);
        let mut args = vec!["add".to_string(), exe_key, "/f".to_string()];
        if !value_name.is_empty() {
            args.push("/v".into());
            args.push(value_name.into());
        } else {
            args.push("/ve".into());
        }
        args.push("/t".into());
        args.push("REG_SZ".into());
        args.push("/d".into());
        args.push(data.into());
        let mut cmd = Command::new(sys_tool("reg.exe"));
        cmd.args(&args);
        hide_console(&mut cmd);
        let out = cmd.output();
        match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Write REG_BINARY under `key_path` (creates key tree via `reg add` fallback).
pub fn write_reg_binary(key_path: &str, value_name: &str, data: &[u8]) -> Result<(), String> {
    // intrinsic target gate — callers gate too, primitives enforce last.
    crate::safety::allow_reg_value_write(key_path, Some(value_name))?;
    #[cfg(not(windows))]
    {
        let _ = (key_path, value_name, data);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        // reg.exe REG_BINARY takes hex without 0x, e.g. 02000000...
        let exe_key = normalize_reg_exe_hive(key_path);
        let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
        let mut args = vec![
            "add".to_string(),
            exe_key.clone(),
            "/f".to_string(),
            "/v".into(),
            value_name.to_string(),
            "/t".into(),
            "REG_BINARY".into(),
            "/d".into(),
            hex,
        ];
        // empty value name → default value
        if value_name.is_empty() {
            args = vec![
                "add".into(),
                exe_key,
                "/f".into(),
                "/ve".into(),
                "/t".into(),
                "REG_BINARY".into(),
                "/d".into(),
                data.iter().map(|b| format!("{b:02x}")).collect(),
            ];
        }
        let mut cmd = Command::new(sys_tool("reg.exe"));
        cmd.args(&args);
        hide_console(&mut cmd);
        let out = cmd.output();
        match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Write service Start DWORD (2=auto, 3=manual, 4=disabled).
/// Errors use stable codes for the UI:
/// `manage:access_denied:<name>` | `manage:open_failed:<name>` | `manage:write_failed:<name>`
pub fn write_service_start(svc_name: &str, start: u32) -> Result<(), String> {
    crate::fsutil::validate_native_name(svc_name)?;
    // intrinsic gate — service names are single leaves under Services.
    let name = svc_name.trim();
    if name.is_empty() || name.contains('\\') || name.contains('/') || name.contains("..") {
        return Err(format!("manage:bad_name:{svc_name}"));
    }
    // Defense in depth: never start-type-write a critical service even if a
    // future caller routes around `allow_manage_service_write`.
    if crate::safety::is_critical_service(name) {
        return Err(format!("manage:protected:{svc_name}"));
    }
    crate::safety::allow_reg_value_write(
        &format!(r"HKLM64\SYSTEM\CurrentControlSet\Services\{name}"),
        Some("Start"),
    )?;
    #[cfg(not(windows))]
    {
        let _ = (svc_name, start);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
        use windows::Win32::System::Registry::{RegOpenKeyExW, RegSetValueExW, REG_DWORD};
        let key_path = format!(r"HKLM64\SYSTEM\CurrentControlSet\Services\{name}");
        let (hive, sub, access) = parse(&key_path).ok_or_else(|| "bad key".to_string())?;
        unsafe {
            let w = to_wide(&sub);
            let mut hk = HKEY::default();
            let st = RegOpenKeyExW(hive, PCWSTR(w.as_ptr()), 0, KEY_SET_VALUE | access, &mut hk);
            if st == ERROR_ACCESS_DENIED {
                return Err(format!("manage:access_denied:{svc_name}"));
            }
            if st != ERROR_SUCCESS {
                return Err(format!("manage:open_failed:{svc_name}"));
            }
            let name_w = to_wide("Start");
            let bytes = start.to_le_bytes();
            let st = RegSetValueExW(hk, PCWSTR(name_w.as_ptr()), 0, REG_DWORD, Some(&bytes));
            let _ = RegCloseKey(hk);
            if st == ERROR_ACCESS_DENIED {
                return Err(format!("manage:access_denied:{svc_name}"));
            }
            if st != ERROR_SUCCESS {
                return Err(format!("manage:write_failed:{svc_name}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
