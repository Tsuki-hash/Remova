//! Registry delete / value delete (real cleanup).

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SAM_FLAGS,
};

fn parse(key: &str) -> Option<(HKEY, String, REG_SAM_FLAGS)> {
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
use crate::fsutil::to_wide;

/// Delete registry key tree. Caller must have run safety checks.
/// Opens the parent with the correct WOW64 view, then deletes the leaf via RegDeleteTreeW.
/// Parent needs DELETE + enumerate/query/set rights (MSDN RegDeleteTreeW).
/// intrinsic secondary gate — protected registry trees are refused here even if
/// the caller skipped `is_safe_to_delete_registry`.
pub fn delete_key(key_path: &str) -> Result<(), String> {
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
                to_wide(&parent)
            };
            let rights =
                DELETE_RIGHT | KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE | KEY_SET_VALUE | access;
            let mut parent_hk = HKEY::default();
            let open = if parent.is_empty() {
                RegOpenKeyExW(hive, PCWSTR::null(), 0, rights, &mut parent_hk)
            } else {
                RegOpenKeyExW(hive, PCWSTR(parent_w.as_ptr()), 0, rights, &mut parent_hk)
            };
            if open.is_err() {
                return Err(format!("open parent failed for {key_path}"));
            }
            let leaf_w = to_wide(&leaf);
            let st = RegDeleteTreeW(parent_hk, PCWSTR(leaf_w.as_ptr()));
            let _ = RegCloseKey(parent_hk);
            if st != ERROR_SUCCESS {
                return Err(format!("RegDeleteTreeW failed for {key_path}"));
            }
        }
        Ok(())
    }
}

/// Delete a value under `key` (Run|Name).
/// intrinsic secondary gate — protected registry trees/values are refused here
/// even if the caller skipped `is_safe_to_delete_registry`.
pub fn delete_value(key_path: &str, value_name: &str) -> Result<(), String> {
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
        unsafe {
            let w = to_wide(&sub);
            let mut hk = HKEY::default();
            RegOpenKeyExW(hive, PCWSTR(w.as_ptr()), 0, KEY_SET_VALUE | access, &mut hk)
                .ok()
                .map_err(|_| format!("open failed {key_path}"))?;
            let vw = to_wide(value_name);
            let st = RegDeleteValueW(hk, PCWSTR(vw.as_ptr()));
            let _ = RegCloseKey(hk);
            if st != ERROR_SUCCESS {
                return Err(format!("delete value failed {key_path}!{value_name}"));
            }
        }
        Ok(())
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
    std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()) + r"\System32"
}

pub fn sys_tool(name: &str) -> String {
    format!(r"{}\{name}", system32_dir())
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
    if task_name.is_empty() || task_name.contains('"') {
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

/// Normalize a PATH segment for exact comparison (quotes + slashes + trailing `\`).
fn normalize_path_entry(s: &str) -> String {
    s.trim()
        .trim_matches('"')
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// True if PATH still contains this exact entry (verify checklist).
pub fn scrub_path_entry_ok(entry: &str) -> bool {
    let needle = normalize_path_entry(entry);
    if needle.is_empty() {
        return false;
    }
    for scope in ["User", "Machine"] {
        if let Ok(current) = read_path_scope(scope) {
            for p in current.split(';') {
                if normalize_path_entry(p) == needle {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether a PATH string already contains `entry` (normalized exact match).
pub fn path_contains_entry(path_value: &str, entry: &str) -> bool {
    let needle = normalize_path_entry(entry);
    if needle.is_empty() {
        return false;
    }
    path_value
        .split(';')
        .any(|p| normalize_path_entry(p) == needle)
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
    let value_name = value_name.trim();
    if value_name.is_empty() {
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
    use std::process::Command;
    let mut cmd = Command::new(sys_tool("reg.exe"));
    cmd.args(["query", &key_win, "/v", value_name, view]);
    hide_console(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(false);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let Some((reg_type, data_raw)) = parse_reg_query(&text) else {
        return Ok(false);
    };
    let key_reg = format!("[{key_win}]");
    let body = match reg_type.as_str() {
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
            let (bytes, _typ) = read_reg_value_bytes(key_path, value_name)?
                .ok_or_else(|| format!("export_reg_value: value vanished for {key_path}"))?;
            let mut payload = bytes;
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
            let (bytes, _typ) = read_reg_value_bytes(key_path, value_name)?
                .ok_or_else(|| format!("export_reg_value: value vanished for {key_path}"))?;
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
    let reg = format!("Windows Registry Editor Version 5.00\r\n\r\n{key_reg}\r\n{body}\r\n");
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(dest, reg).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Parse `reg query` output for a single value: locate the known-type token
/// (a value NAME containing "REG_" must not confuse it), then accumulate
/// multi-line hex continuations until the next key header — long REG_BINARY
/// data used to be silently truncated at the first line (data corruption in
/// the backup safety net). Returns (type, data) or None.
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
    let needle = normalize_path_entry(entry);
    if needle.is_empty() {
        return Err("empty path entry".into());
    }
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
                if normalize_path_entry(&expanded) == needle || normalize_path_entry(seg) == needle
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

    fn put(user: &str, machine: &str, expand: bool) {
        let mut map = HashMap::new();
        map.insert("User".to_string(), (user.to_string(), expand));
        map.insert("Machine".to_string(), (machine.to_string(), expand));
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(map);
        *FAIL_READ.lock().unwrap_or_else(|e| e.into_inner()) = false;
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

    pub fn clear() {
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *FAIL_READ.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }

    pub fn active() -> bool {
        STATE.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    pub fn read_raw(scope: &str) -> Result<(String, bool), String> {
        if *FAIL_READ.lock().unwrap_or_else(|e| e.into_inner()) {
            return Err(format!("read Path {scope} failed"));
        }
        let guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.as_ref().expect("path mock not installed");
        Ok(map.get(scope).cloned().unwrap_or((String::new(), false)))
    }

    pub fn write(scope: &str, value: &str, expand: bool) -> Result<(), String> {
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
    read_reg_value_raw(key, "Path")
}

/// Raw Path text plus its type (`true` = REG_EXPAND_SZ). Write-back must
/// preserve these unexpanded bytes for REG_EXPAND_SZ values.
fn read_reg_value_raw(key_path: &str, value_name: &str) -> Result<(String, bool), String> {
    #[cfg(not(windows))]
    {
        let _ = key_path;
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
            RegOpenKeyExW(hive, PCWSTR(w.as_ptr()), 0, KEY_READ | access, &mut hk)
                .ok()
                .map_err(|_| format!("open failed {key_path}"))?;
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
        RegOpenKeyExW(hive, PCWSTR(w.as_ptr()), 0, KEY_READ | access, &mut hk)
            .ok()
            .map_err(|_| format!("open failed {key_path}"))?;
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
        if st != ERROR_SUCCESS {
            return Ok(None);
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
    // A single environment variable is capped at 32,767 chars in the process
    // environment block — fail with a clear code instead of a spawn failure.
    if value.chars().count() > 30_000 {
        return Err(crate::error::path_io_err(
            "PATH value exceeds the environment variable size limit".to_string(),
        )
        .to_ipc());
    }
    use std::process::Command;
    // Explicit registry kind: SetEnvironmentVariable would silently rewrite a
    // REG_EXPAND_SZ Path as REG_SZ (and the content is raw %VAR% text here).
    let kind = if expand { "ExpandString" } else { "String" };
    let (root, subkey) = match scope {
        "User" => ("CurrentUser", r"Environment"),
        "Machine" => (
            "LocalMachine",
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
        _ => return Err(crate::error::path_io_err(format!("unknown Path scope {scope}")).to_ipc()),
    };
    let script = format!(
        "$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey('{root}','Default');\
         $k=$b.CreateSubKey('{subkey}');\
         $k.SetValue('Path',$env:REMOVA_PATH_VALUE,[Microsoft.Win32.RegistryValueKind]::{kind});\
         $k.Close()"
    );
    let mut child_cmd = Command::new(powershell_exe());
    child_cmd.env("REMOVA_PATH_VALUE", value).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &script,
    ]);
    hide_console(&mut child_cmd);
    let mut child = child_cmd.spawn().map_err(|e| e.to_string())?;
    let st = child.wait().map_err(|e| e.to_string())?;
    if !st.success() {
        return Err(format!("set Path {scope} failed"));
    }
    Ok(())
}

pub fn leaf_name(path: &str) -> String {
    path.rsplit('\\').next().unwrap_or("").to_string()
}

/// Rename a registry value (copy data + delete old) under `key`.
pub fn rename_reg_value(key_path: &str, from: &str, to: &str) -> Result<(), String> {
    // intrinsic target gate — callers gate too, primitives enforce last.
    // Renames never target Services keys, so no value name is offered
    // (the Services branch of the gate only whitelists the Start value).
    crate::safety::allow_reg_value_write(key_path, None)?;
    #[cfg(not(windows))]
    {
        let _ = (key_path, from, to);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{RegQueryValueExW, RegSetValueExW, REG_VALUE_TYPE};
        let (hive, sub, access) = parse(key_path).ok_or_else(|| "bad key".to_string())?;
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
            let from_w = to_wide(from);
            let mut typ = REG_VALUE_TYPE(0);
            let mut data = vec![0u8; 8192];
            let mut data_len = data.len() as u32;
            let st = RegQueryValueExW(
                hk,
                PCWSTR(from_w.as_ptr()),
                None,
                Some(&mut typ),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            if st != ERROR_SUCCESS {
                let _ = RegCloseKey(hk);
                return Err(format!("query failed {from}"));
            }
            let to_w = to_wide(to);
            let st = RegSetValueExW(
                hk,
                PCWSTR(to_w.as_ptr()),
                0,
                typ,
                Some(&data[..data_len as usize]),
            );
            if st == ERROR_SUCCESS {
                let _ = RegDeleteValueW(hk, PCWSTR(from_w.as_ptr()));
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
fn normalize_reg_exe_hive(key_path: &str) -> String {
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
    // HKLM32 → WOW6432NODE view under HKLM when path is not already under WOW6432NODE.
    if alias_up == "HKLM32" {
        let low = rest.to_uppercase();
        if !low.contains("WOW6432NODE") {
            return format!(r"HKLM\WOW6432NODE\{rest}");
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
mod tests {
    #[test]
    fn parse_reg_query_multi_line_binary_and_name_with_reg_token() {
        // Long REG_BINARY data wraps onto continuation lines; a value NAME may
        // itself contain "REG_" — the known-type token wins, continuations join.
        let out = concat!(
            "HKEY_CURRENT_USER\\Software\\T\r\n",
            "\r\n",
            "    My REG_Thing    REG_BINARY    0A0B\r\n",
            "        0C0D0E0F\r\n",
            "\r\n",
            "HKEY_CURRENT_USER\\Software\\T2\r\n",
            "\r\n",
            "    Other    REG_SZ    hello\r\n",
        );
        let (ty, data) = super::parse_reg_query(out).unwrap();
        assert_eq!(ty, "REG_BINARY");
        assert_eq!(data, "0A0B 0C0D0E0F");
        // Single-line REG_SZ still parses; the value name is ignored.
        let (ty, data) =
            super::parse_reg_query("    Path    REG_SZ    C:\\x y\\z.exe\r\n").unwrap();
        assert_eq!(ty, "REG_SZ");
        assert_eq!(data, "C:\\x y\\z.exe");
    }

    #[test]
    fn parse_reg_query_no_type_is_none() {
        assert!(
            super::parse_reg_query("HKEY_CURRENT_USER\\Software\\T\r\n\r\n    junk line\r\n")
                .is_none()
        );
    }

    #[test]
    fn fs_rejects_drive_root() {
        assert_eq!(
            super::normalize_reg_exe_hive(r"HKLM64\SOFTWARE\Foo"),
            r"HKLM\SOFTWARE\Foo"
        );
        assert_eq!(
            super::normalize_reg_exe_hive(r"HKLM32\SOFTWARE\Foo"),
            r"HKLM\WOW6432NODE\SOFTWARE\Foo"
        );
        assert_eq!(
            super::normalize_reg_exe_hive(r"HKCU\Software\Bar"),
            r"HKCU\Software\Bar"
        );
    }

    #[test]
    fn normalize_path_entry_strips_quotes_and_slash() {
        assert_eq!(
            super::normalize_path_entry(r#""C:\Python3\\"#),
            "c:\\python3"
        );
        assert_eq!(super::normalize_path_entry("C:/Python3/"), "c:\\python3");
    }

    #[test]
    fn scrub_uses_exact_segment_match() {
        // C:\Python3 must not equal C:\Python312
        let a = super::normalize_path_entry(r"C:\Python3");
        let b = super::normalize_path_entry(r"C:\Python312");
        let c = super::normalize_path_entry(r"C:\Python3\Scripts");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, super::normalize_path_entry(r"C:\Python3\"));
    }

    #[test]
    fn delete_key_intrinsic_gate_blocks_protected() {
        // write primitive refuses protected trees even without caller checks.
        assert!(super::delete_key(r"HKLM\SYSTEM\CurrentControlSet\Services\WinDefend").is_err());
        assert!(super::delete_key(r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run").is_err());
        assert!(
            super::delete_key(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall").is_err()
        );
    }

    /// A trailing separator makes the effective leaf empty — RegDeleteTreeW
    /// with an empty subkey would wipe the PARENT's contents. The primitive
    /// must refuse the shape before any registry access, and the key gate
    /// must refuse root shapes with a trailing separator (an allow arm
    /// matched `...\Uninstall\` as a prefix before the trim was added).
    #[test]
    fn delete_key_refuses_trailing_separator_shapes() {
        assert!(super::delete_key(r"HKCU\Software\RemovaShapeTest\").is_err());
        // Gate level: every root family must refuse the trailing-separator form.
        for shape in [
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\",
            r"HKLM\SYSTEM\CurrentControlSet\Services\",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run\",
        ] {
            assert!(
                crate::safety::is_safe_to_delete_registry(shape).is_err(),
                "root with trailing separator must be refused: {shape}"
            );
        }
    }

    #[test]
    fn scrub_path_entry_intrinsic_gate_blocks_system() {
        // PATH scrub refuses system segments even without caller checks.
        assert!(super::scrub_path_entry(r"C:\Windows\System32").is_err());
        assert!(super::scrub_path_entry(r"C:\Windows").is_err());
    }

    /// Non-ASCII REG_SZ data must survive export byte-exact: reg.exe text
    /// output is OEM-codepage encoded and used to bake mojibake into the
    /// seal-digested value.reg (the only authorized restore source).
    #[cfg(windows)]
    #[test]
    fn export_reg_value_preserves_non_ascii_bytes() {
        struct RegistryKeyCleanup(String);
        impl Drop for RegistryKeyCleanup {
            fn drop(&mut self) {
                let _ = std::process::Command::new(super::sys_tool("reg.exe"))
                    .args(["delete", &self.0, "/f"])
                    .output();
            }
        }

        let key = format!(
            r"HKCU\Software\RemovaExportTest_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let _cleanup = RegistryKeyCleanup(key.clone());
        let tmp = std::env::temp_dir().join(format!(
            "remova_value_reg_cjk_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let dest = tmp.join("value.reg");
        let add = std::process::Command::new(super::sys_tool("reg.exe"))
            .arg("add")
            .arg(&key)
            .args([
                "/v",
                "Path",
                "/t",
                "REG_SZ",
                "/d",
                r"C:\Users\张三\工具",
                "/f",
            ])
            .output()
            .unwrap();
        assert!(add.status.success(), "fixture key write failed");
        let exported = super::export_reg_value(&key, "Path", &dest).unwrap();
        assert!(exported, "test key must exist for the export");
        let text = std::fs::read_to_string(&dest).unwrap();
        let payload = text
            .split("=hex(1):")
            .nth(1)
            .expect("REG_SZ must export as hex(1)");
        let bytes: Vec<u8> = payload
            .split(',')
            .filter_map(|b| u8::from_str_radix(b.trim(), 16).ok())
            .collect();
        let wide: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let decoded = String::from_utf16(&wide)
            .expect("payload must decode cleanly")
            .trim_end_matches('\0')
            .to_string();
        assert_eq!(decoded, r"C:\Users\张三\工具");
        assert!(
            !decoded.contains('\u{FFFD}'),
            "no replacement chars allowed"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn delete_value_intrinsic_gate_blocks_protected() {
        // value delete refuses protected trees even without caller checks.
        assert!(
            super::delete_value(r"HKLM\SYSTEM\CurrentControlSet\Services\WinDefend", "Start")
                .is_err()
        );
        assert!(
            super::delete_value(r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run", "").is_err()
        );
        assert!(super::delete_value(
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            "DisplayName"
        )
        .is_err());
        // Run value-level shape is allowed by the safety gate (actual OS delete is best-effort).
        // Use a non-existent Run value name so the OS call fails safely after the gate.
        let r = super::delete_value(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            "RemovaNoSuchValue_Test",
        );
        // Gate passed (not a "registry delete blocked" error); OS may report delete failure.
        match r {
            Ok(()) => {}
            Err(e) => assert!(
                !e.contains("registry delete blocked"),
                "gate must allow Run value shape, got {e}"
            ),
        }
    }

    #[test]
    fn merge_path_entry_dedupes_and_appends() {
        assert_eq!(super::merge_path_entry(r"C:\a;C:\b", r"C:\b"), None);
        assert_eq!(
            super::merge_path_entry(r"C:\a", r"C:\b"),
            Some(r"C:\a;C:\b".to_string())
        );
        assert!(super::path_contains_entry(r"C:\a;C:\b\", r"c:\b"));
        assert!(!super::path_contains_entry(r"C:\a", r"C:\b"));
    }

    #[cfg(windows)]
    #[test]
    fn export_reg_value_rejects_unsafe_names() {
        let tmp = std::env::temp_dir().join("remova_reg_inject_test");
        let _ = std::fs::create_dir_all(&tmp);
        let dest = tmp.join("value.reg");
        assert!(super::export_reg_value(r"HKCU\SOFTWARE\RemovaTest", "bad\"name", &dest).is_err());
        assert!(
            super::export_reg_value(r"HKCU\SOFTWARE\RemovaTest", "bad\r\nname", &dest).is_err()
        );
        assert!(super::export_reg_value(r"HKCU\SOFTWARE\RemovaTest", "", &dest).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(windows)]
    #[test]
    fn export_reg_value_writes_existing_value() {
        let tmp = std::env::temp_dir().join(format!("remova_value_reg_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let dest = tmp.join("value.reg");
        // Read-only OS fixture: do not mutate the user's PATH or registry.
        let r = super::export_reg_value(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders",
            "Desktop",
            &dest,
        );
        match r {
            Ok(true) => {
                let s = std::fs::read_to_string(&dest).unwrap();
                assert!(s.contains("Windows Registry Editor"));
                assert!(s.contains("Shell Folders"));
                assert!(s.contains("\"Desktop\"="));
                assert!(s.contains("HKEY_CURRENT_USER"));
            }
            Ok(false) => panic!("Desktop registry fixture missing: export was not exercised"),
            Err(e) => panic!("export_reg_value: {e}"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// registry-native PATH read — pure pieces get direct coverage.
    /// `wstring_from_reg_data` decodes UTF-16LE and trims the trailing NUL.
    #[cfg(windows)]
    #[test]
    fn wstring_from_reg_data_decodes_utf16le() {
        let s = r"C:\Vendor 工具;C:\Other";
        let mut wide: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
        wide.extend_from_slice(&[0, 0]); // NUL terminator as the registry stores it
        assert_eq!(crate::fsutil::wstring_from_reg_data(&wide), s);
        // Odd byte count: the trailing half unit is dropped, not mis-decoded.
        assert_eq!(
            crate::fsutil::wstring_from_reg_data(&wide[..wide.len() - 1]),
            s
        );
        // Empty / short blobs decode to "".
        assert_eq!(crate::fsutil::wstring_from_reg_data(&[]), "");
        assert_eq!(crate::fsutil::wstring_from_reg_data(&[0x41]), "");
        assert_eq!(crate::fsutil::wstring_from_reg_data(&[0x41, 0x00]), "A");
    }

    /// `%VAR%` expansion used for REG_EXPAND_SZ Path values.
    #[cfg(windows)]
    #[test]
    fn expand_env_string_expands_and_passes_through() {
        // Unique name: set_var is process-global, other tests never read this.
        std::env::set_var("REMOVA_QA02_VAR", r"C:\Vendor Tool");
        assert_eq!(
            super::expand_env_string(r"%REMOVA_QA02_VAR%\bin"),
            r"C:\Vendor Tool\bin"
        );
        assert_eq!(
            super::expand_env_string("no markers here"),
            "no markers here"
        );
        // Unknown variable stays verbatim (Windows keeps the reference).
        assert_eq!(
            super::expand_env_string("%REMOVA_QA02_UNSET_VAR%/x"),
            "%REMOVA_QA02_UNSET_VAR%/x"
        );
    }

    /// real-registry integration check — `HKCU\Environment` Path
    /// reads through the native code path (mock must be inactive). Opt-in via
    /// `cargo test -- --ignored` since stock machines may have no user Path.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn read_reg_path_value_reads_real_user_environment() {
        assert!(!super::path_mock::active());
        // Must not fall back to the process env or error out on a healthy box.
        let raw = super::read_path_scope("User").expect("native user PATH read");
        // A successful read returns a string (possibly empty on a stripped
        // account) — never process-env-shaped noise, never Err.
        assert!(
            raw.contains(';') || raw.contains('\\') || raw.contains('/') || raw.is_empty(),
            "unexpected user PATH shape: {raw:?}"
        );
    }

    /// REG_EXPAND_SZ PATH values must be rewritten with their raw `%VAR%`
    /// bytes and type intact — scrub/restore must not bake in expansions.
    #[cfg(windows)]
    #[test]
    fn scrub_and_restore_preserve_expand_sz_raw_bytes() {
        use super::path_mock;
        let _lock = path_mock::lock_mock();
        path_mock::clear();
        // Expansion sources are pinned for hermeticity: the machine's
        // ProgramFiles may be relocated or absent, so the test pins its own
        // variable that only the fallback source can resolve.
        super::expand_mock::install(&[("RemovaVendorDir", r"C:\Program Files")]);
        path_mock::install_expand(
            r"%SystemRoot%\system32;C:\Tools\App;%RemovaVendorDir%\Shared",
            r"%SystemRoot%",
        );
        // Scrub a literal entry: untouched %VAR% segments survive verbatim.
        assert!(super::scrub_path_entry(r"C:\Tools\App").unwrap());
        let (raw, expand) = path_mock::read_raw("User").unwrap();
        assert!(expand, "scrub must not flatten REG_EXPAND_SZ");
        assert_eq!(raw, r"%SystemRoot%\system32;%RemovaVendorDir%\Shared");
        // An entry already present in expanded form must not duplicate raw.
        assert!(!super::restore_path_entry(r"C:\Program Files\Shared", &["User"]).unwrap());
        // Restore appends raw bytes and keeps the type.
        assert!(super::restore_path_entry(r"C:\Tools\App", &["User"]).unwrap());
        let (raw, expand) = path_mock::read_raw("User").unwrap();
        assert!(expand);
        assert_eq!(
            raw,
            r"%SystemRoot%\system32;%RemovaVendorDir%\Shared;C:\Tools\App"
        );
        // The literal entry now present must not be appended twice.
        assert!(!super::restore_path_entry(r"C:\Tools\App", &["User"]).unwrap());
        // Scrub an entry that only matches after expansion.
        assert!(super::scrub_path_entry(r"C:\Program Files\Shared").unwrap());
        let (raw, expand) = path_mock::read_raw("User").unwrap();
        assert!(expand);
        assert_eq!(raw, r"%SystemRoot%\system32;C:\Tools\App");
        // REG_SZ scopes keep plain String semantics.
        super::expand_mock::clear();
        path_mock::clear();
        path_mock::install(r"C:\A;C:\B", r"C:\Windows");
        assert!(super::scrub_path_entry(r"C:\A").unwrap());
        let (raw, expand) = path_mock::read_raw("User").unwrap();
        assert!(!expand);
        assert_eq!(raw, r"C:\B");
        path_mock::clear();
    }

    /// A variable no source can resolve stays literal; restore appends
    /// honestly (and keeps the raw form) instead of guessing a match.
    #[test]
    fn restore_appends_when_var_unresolvable() {
        use super::path_mock;
        let _lock = path_mock::lock_mock();
        path_mock::clear();
        path_mock::install_expand(r"%RemovaNowhereAtAll%\Shared", r"");
        assert!(super::restore_path_entry(r"C:\Program Files\Shared", &["User"]).unwrap());
        let (raw, expand) = path_mock::read_raw("User").unwrap();
        assert!(expand);
        assert_eq!(raw, r"%RemovaNowhereAtAll%\Shared;C:\Program Files\Shared");
        path_mock::clear();
    }
}
