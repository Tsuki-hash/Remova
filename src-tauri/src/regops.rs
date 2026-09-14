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

/// Best-effort: stop/delete Windows service via sc.exe.
pub fn sc_delete_service(svc_name: &str) -> bool {
    if svc_name.is_empty() || svc_name.contains('\\') || svc_name.contains('"') {
        return false;
    }
    use std::process::Command;
    let sc = sys_tool("sc.exe");
    let _ = Command::new(&sc).args(["stop", svc_name]).output();
    let out = Command::new(&sc).args(["delete", svc_name]).output();
    matches!(out, Ok(o) if o.status.success())
}

/// Best-effort: schtasks /delete for a task leaf name.
pub fn schtasks_delete(task_name: &str) -> bool {
    if task_name.is_empty() || task_name.contains('"') {
        return false;
    }
    use std::process::Command;
    let out = Command::new(sys_tool("schtasks.exe"))
        .args(["/delete", "/tn", task_name, "/f"])
        .output();
    matches!(out, Ok(o) if o.status.success())
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
        let out = Command::new(sys_tool("reg.exe")).args(&args).output();
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
        let out = Command::new(sys_tool("reg.exe")).args(&args).output();
        match out {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Write service Start DWORD (2=auto, 3=manual, 4=disabled).
pub fn write_service_start(svc_name: &str, start: u32) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = (svc_name, start);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{RegOpenKeyExW, RegSetValueExW, REG_DWORD};
        let key_path = format!(r"HKLM64\SYSTEM\CurrentControlSet\Services\{svc_name}");
        let (hive, sub, access) = parse(&key_path).ok_or_else(|| "bad key".to_string())?;
        unsafe {
            let w = to_wide(&sub);
            let mut hk = HKEY::default();
            RegOpenKeyExW(hive, PCWSTR(w.as_ptr()), 0, KEY_SET_VALUE | access, &mut hk)
                .ok()
                .map_err(|_| format!("open service key failed {svc_name}"))?;
            let name_w = to_wide("Start");
            let bytes = start.to_le_bytes();
            let st = RegSetValueExW(hk, PCWSTR(name_w.as_ptr()), 0, REG_DWORD, Some(&bytes));
            let _ = RegCloseKey(hk);
            if st != ERROR_SUCCESS {
                return Err(format!("write Start failed for {svc_name}"));
            }
        }
        Ok(())
    }
}
