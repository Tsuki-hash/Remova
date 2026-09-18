//! Registry delete / value delete (real cleanup).

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::ERROR_SUCCESS;
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

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Delete registry key tree. Caller must have run safety checks.
/// Opens the parent with the correct WOW64 view, then deletes the leaf via RegDeleteTreeW.
/// Parent needs DELETE + enumerate/query/set rights (MSDN RegDeleteTreeW).
pub fn delete_key(key_path: &str) -> Result<(), String> {
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
pub fn delete_value(key_path: &str, value_name: &str) -> Result<(), String> {
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

/// System32 absolute path for a tool (SEC-4: avoid PATH hijack).
pub fn sys_tool(name: &str) -> String {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{windir}\System32\{name}")
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

/// Best-effort: stop/delete Windows service via sc.exe.
pub fn sc_delete_service(svc_name: &str) -> bool {
    if svc_name.is_empty() || svc_name.contains('\\') || svc_name.contains('"') {
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

/// Best-effort: schtasks /delete for a task leaf name.
pub fn schtasks_delete(task_name: &str) -> bool {
    if task_name.is_empty() || task_name.contains('"') {
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
pub fn restore_path_entry(entry: &str, scopes: &[&str]) -> Result<bool, String> {
    let mut changed = false;
    let targets: Vec<&str> = if scopes.is_empty() {
        vec!["User", "Machine"]
    } else {
        scopes.to_vec()
    };
    for scope in targets {
        let current = read_path_scope(scope)?;
        if let Some(next) = merge_path_entry(&current, entry) {
            write_path_scope(scope, &next)?;
            changed = true;
        }
    }
    if changed {
        broadcast_env_change();
    }
    Ok(changed)
}

/// Remove one PATH segment from User and Machine environments (exact match only).
/// Returns Ok(true) if at least one scope changed.
pub fn scrub_path_entry(entry: &str) -> Result<bool, String> {
    let needle = normalize_path_entry(entry);
    if needle.is_empty() {
        return Err("empty path entry".into());
    }
    let mut changed = false;
    for scope in ["User", "Machine"] {
        let current = read_path_scope(scope)?;
        let parts: Vec<&str> = current
            .split(';')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let mut kept: Vec<&str> = Vec::new();
        let mut hit = false;
        for p in parts {
            if normalize_path_entry(p) == needle {
                hit = true;
                continue;
            }
            kept.push(p);
        }
        if !hit {
            continue;
        }
        write_path_scope(scope, &kept.join(";"))?;
        changed = true;
    }
    if changed {
        broadcast_env_change();
    }
    Ok(changed)
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
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{windir}\System32\WindowsPowerShell\v1.0\powershell.exe")
}

fn read_path_scope(scope: &str) -> Result<String, String> {
    use std::process::Command;
    let ps = format!("[Environment]::GetEnvironmentVariable('Path','{scope}')");
    let mut cmd = Command::new(powershell_exe());
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps]);
    hide_console(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return std::env::var("PATH").map_err(|e| e.to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Public wrapper so the scanner can read true User/Machine PATH.
pub fn read_path_scope_public(scope: &str) -> Result<String, String> {
    read_path_scope(scope)
}

pub(crate) fn write_path_scope(scope: &str, value: &str) -> Result<(), String> {
    use std::process::Command;
    let mut child_cmd = Command::new(powershell_exe());
    child_cmd.env("REMOVA_PATH_VALUE", value).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!(
            "[Environment]::SetEnvironmentVariable('Path', $env:REMOVA_PATH_VALUE, '{scope}')"
        ),
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
pub fn create_reg_sz(key_path: &str, value_name: &str, data: &str) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = (key_path, value_name, data);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        // Prefer reg.exe for reliable key creation under HKCU\Software\Classes\*\shell
        let mut args = vec!["add".to_string(), key_path.to_string(), "/f".to_string()];
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
    #[cfg(not(windows))]
    {
        let _ = (key_path, value_name, data);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        // reg.exe REG_BINARY takes hex without 0x, e.g. 02000000...
        let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
        let mut args = vec![
            "add".to_string(),
            key_path.to_string(),
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
                key_path.into(),
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
///
/// Errors use stable codes for the UI:
/// `manage:access_denied:<name>` | `manage:open_failed:<name>` | `manage:write_failed:<name>`
pub fn write_service_start(svc_name: &str, start: u32) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = (svc_name, start);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
        use windows::Win32::System::Registry::{RegOpenKeyExW, RegSetValueExW, REG_DWORD};
        let key_path = format!(r"HKLM64\SYSTEM\CurrentControlSet\Services\{svc_name}");
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
    fn normalize_path_entry_strips_quotes_and_slash() {
        assert_eq!(
            super::normalize_path_entry(r#""C:\Python3\\"#),
            "c:\\python3"
        );
        assert_eq!(super::normalize_path_entry("C:/Python3/"), "c:\\python3");
    }

    #[test]
    fn scrub_uses_exact_segment_match() {
        // SEC-1: C:\Python3 must not equal C:\Python312
        let a = super::normalize_path_entry(r"C:\Python3");
        let b = super::normalize_path_entry(r"C:\Python312");
        let c = super::normalize_path_entry(r"C:\Python3\Scripts");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, super::normalize_path_entry(r"C:\Python3\"));
    }
}
