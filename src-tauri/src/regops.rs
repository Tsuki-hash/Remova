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
pub fn delete_key(key_path: &str) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = key_path;
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        let (hive, sub, access) = parse(key_path).ok_or_else(|| "bad key".to_string())?;
        unsafe {
            let w = to_wide(&sub);
            let st = RegDeleteTreeW(hive, PCWSTR(w.as_ptr()));
            // Fallback if hive open path differs: open parent
            if st != ERROR_SUCCESS {
                let mut hk = HKEY::default();
                let _ = RegOpenKeyExW(
                    hive,
                    PCWSTR(w.as_ptr()),
                    0,
                    KEY_READ | access,
                    &mut hk,
                );
                let _ = RegCloseKey(hk);
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
            RegOpenKeyExW(
                hive,
                PCWSTR(w.as_ptr()),
                0,
                KEY_SET_VALUE | access,
                &mut hk,
            )
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

/// Best-effort: stop/delete Windows service via sc.exe.
pub fn sc_delete_service(svc_name: &str) -> bool {
    if svc_name.is_empty() || svc_name.contains('\\') || svc_name.contains('"') {
        return false;
    }
    use std::process::Command;
    let _ = Command::new("sc").args(["stop", svc_name]).output();
    let out = Command::new("sc").args(["delete", svc_name]).output();
    matches!(out, Ok(o) if o.status.success())
}

/// Best-effort: schtasks /delete for a task leaf name.
pub fn schtasks_delete(task_name: &str) -> bool {
    if task_name.is_empty() || task_name.contains('"') {
        return false;
    }
    use std::process::Command;
    let out = Command::new("schtasks")
        .args(["/delete", "/tn", task_name, "/f"])
        .output();
    matches!(out, Ok(o) if o.status.success())
}

pub fn leaf_name(path: &str) -> String {
    path.rsplit('\\').next().unwrap_or("").to_string()
}
