//! Minimal registry helpers for scanner (read-only).

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_EXPAND_SZ, REG_SAM_FLAGS, REG_SZ,
};

fn parse_alias(key: &str) -> Option<(HKEY, String, REG_SAM_FLAGS)> {
    let (alias, rest) = key.split_once('\\')?;
    let rest = rest.to_string();
    match alias.to_uppercase().as_str() {
        "HKLM64" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_64KEY)),
        "HKLM32" | "HKLMWOW" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_32KEY)),
        "HKLM" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_64KEY)),
        "HKCU" => Some((HKEY_CURRENT_USER, rest, KEY_WOW64_64KEY)),
        _ => None,
    }
}

// Shared Windows string helpers live in `fsutil`.
#[cfg(windows)]
use crate::fsutil::{to_wide, wstring_from_reg_data};

/// App Paths subkeys named like `exe_name` under HKLM64 / HKLM32 / HKCU.
pub fn find_app_paths(exe_name: &str) -> Vec<String> {
    #[cfg(not(windows))]
    {
        let _ = exe_name;
        return vec![];
    }
    #[cfg(windows)]
    {
        let targets = [
            (
                "HKLM64",
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
            ),
            (
                "HKLM32",
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
            ),
            (
                "HKCU",
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
            ),
        ];
        let needle = exe_name.to_lowercase();
        let mut hits = vec![];
        for (alias, sub) in targets {
            let full = format!("{alias}\\{sub}");
            for sk in list_subkeys(&full) {
                let l = sk.to_lowercase();
                if l == needle || l == format!("{needle}.exe") {
                    hits.push(format!("{full}\\{sk}"));
                }
            }
        }
        hits
    }
}

pub fn list_subkeys(key: &str) -> Vec<String> {
    #[cfg(not(windows))]
    {
        let _ = key;
        vec![]
    }
    #[cfg(windows)]
    {
        let Some((hive, sub, access)) = parse_alias(key) else {
            return vec![];
        };
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return vec![];
            }
            let mut count = 0u32;
            let mut max_sub = 0u32;
            let _ = RegQueryInfoKeyW(
                root,
                windows::core::PWSTR::null(),
                None,
                None,
                Some(&mut count),
                Some(&mut max_sub),
                None,
                None,
                None,
                None,
                None,
                None,
            );
            let mut out = vec![];
            for i in 0..count {
                let mut buf = vec![0u16; max_sub as usize + 2];
                let mut len = buf.len() as u32;
                if RegEnumKeyExW(
                    root,
                    i,
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut len,
                    None,
                    windows::core::PWSTR::null(),
                    None,
                    None,
                ) == ERROR_SUCCESS
                {
                    buf.truncate(len as usize);
                    out.push(String::from_utf16_lossy(&buf));
                }
            }
            let _ = RegCloseKey(root);
            out
        }
    }
}

pub fn list_values(key: &str) -> Vec<(String, String)> {
    #[cfg(not(windows))]
    {
        let _ = key;
        vec![]
    }
    #[cfg(windows)]
    {
        let Some((hive, sub, access)) = parse_alias(key) else {
            return vec![];
        };
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return vec![];
            }
            let mut out = vec![];
            let mut n = 0u32;
            loop {
                let mut vname = vec![0u16; 256];
                let mut vname_len = vname.len() as u32;
                let mut vtype = 0u32;
                let mut data = vec![0u8; 4096];
                let mut data_len = data.len() as u32;
                let mut st = RegEnumValueW(
                    root,
                    n,
                    windows::core::PWSTR(vname.as_mut_ptr()),
                    &mut vname_len,
                    None,
                    Some(&mut vtype),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                );
                if st == ERROR_MORE_DATA {
                    // REV-BE-09: one retry with the buffer sizes the API reported
                    // instead of silently dropping oversized names/values.
                    let need_name = vname_len as usize;
                    let need_data = data_len as usize;
                    if need_name > (1 << 14) || need_data > (1 << 20) {
                        n += 1; // pathological — skip this value, keep enumerating
                        continue;
                    }
                    if need_name > vname.len() {
                        vname = vec![0u16; need_name + 1];
                    }
                    if need_data > data.len() {
                        data = vec![0u8; need_data];
                    }
                    vname_len = vname.len() as u32;
                    data_len = data.len() as u32;
                    st = RegEnumValueW(
                        root,
                        n,
                        windows::core::PWSTR(vname.as_mut_ptr()),
                        &mut vname_len,
                        None,
                        Some(&mut vtype),
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    );
                }
                if st == ERROR_MORE_DATA {
                    // Second failure (value grew again / buffer race) — skip
                    // this value and keep enumerating instead of dropping the rest.
                    n += 1;
                    continue;
                }
                if st != ERROR_SUCCESS {
                    break;
                }
                n += 1;
                if vtype == REG_SZ.0 || vtype == REG_EXPAND_SZ.0 {
                    let name = String::from_utf16_lossy(&vname[..vname_len as usize]);
                    let val = wstring_from_reg_data(&data[..data_len as usize]);
                    out.push((name, val));
                }
            }
            let _ = RegCloseKey(root);
            out
        }
    }
}

/// Read REG_SZ/EXPAND_SZ value by name via RegQueryValueExW (PERF-5).
pub fn read_string(key: &str, value_name: &str) -> Option<String> {
    #[cfg(not(windows))]
    {
        let _ = (key, value_name);
        None
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{
            RegQueryValueExW, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
        };
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let name_w = to_wide(value_name);
            let mut vtype = REG_VALUE_TYPE(0);
            let mut data = vec![0u8; 4096];
            let mut data_len = data.len() as u32;
            let mut st = RegQueryValueExW(
                root,
                PCWSTR(name_w.as_ptr()),
                None,
                Some(&mut vtype),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            if st == ERROR_MORE_DATA {
                // REV-BE-09: re-query with the reported size — long UninstallString
                // / display values must not be silently dropped.
                let need = data_len as usize;
                if need > data.len() && need <= (1 << 20) {
                    data = vec![0u8; need];
                    data_len = data.len() as u32;
                    st = RegQueryValueExW(
                        root,
                        PCWSTR(name_w.as_ptr()),
                        None,
                        Some(&mut vtype),
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    );
                }
            }
            let _ = RegCloseKey(root);
            if st != ERROR_SUCCESS || (vtype != REG_SZ && vtype != REG_EXPAND_SZ) {
                return None;
            }
            Some(wstring_from_reg_data(&data[..data_len as usize]))
        }
    }
}

/// Read REG_DWORD value by name via RegQueryValueExW (REV-BE-03, O(1) — same path as `read_string`).
pub fn read_dword(key: &str, value_name: &str) -> Option<u32> {
    #[cfg(not(windows))]
    {
        let _ = (key, value_name);
        None
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{RegQueryValueExW, REG_DWORD, REG_VALUE_TYPE};
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let name_w = to_wide(value_name);
            let mut vtype = REG_VALUE_TYPE(0);
            let mut data = [0u8; 4];
            let mut data_len = data.len() as u32;
            let st = RegQueryValueExW(
                root,
                PCWSTR(name_w.as_ptr()),
                None,
                Some(&mut vtype),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            let _ = RegCloseKey(root);
            if st != ERROR_SUCCESS || vtype != REG_DWORD || data_len < 4 {
                return None;
            }
            Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
    }
}

/// Read REG_BINARY value by name (StartupApproved etc.).
pub fn read_binary(key: &str, value_name: &str) -> Option<Vec<u8>> {
    #[cfg(not(windows))]
    {
        let _ = (key, value_name);
        None
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{RegQueryValueExW, REG_BINARY, REG_VALUE_TYPE};
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let name_w = to_wide(value_name);
            let mut vtype = REG_VALUE_TYPE(0);
            let mut data = vec![0u8; 64];
            let mut data_len = data.len() as u32;
            let st = RegQueryValueExW(
                root,
                PCWSTR(name_w.as_ptr()),
                None,
                Some(&mut vtype),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            let _ = RegCloseKey(root);
            if st != ERROR_SUCCESS || vtype != REG_BINARY {
                return None;
            }
            data.truncate(data_len as usize);
            Some(data)
        }
    }
}

pub fn read_string_default(key: &str) -> Option<String> {
    #[cfg(not(windows))]
    {
        let _ = key;
        None
    }
    #[cfg(windows)]
    {
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut hk = HKEY::default();
            if RegOpenKeyExW(hive, PCWSTR(sub_w.as_ptr()), 0, KEY_READ | access, &mut hk)
                != ERROR_SUCCESS
            {
                return None;
            }
            let mut buf = vec![0u8; 4096];
            let mut len = buf.len() as u32;
            let mut vtype = windows::Win32::System::Registry::REG_VALUE_TYPE(0);
            let empty = to_wide("");
            let mut st = windows::Win32::System::Registry::RegQueryValueExW(
                hk,
                PCWSTR(empty.as_ptr()),
                None,
                Some(&mut vtype),
                Some(buf.as_mut_ptr()),
                Some(&mut len),
            );
            if st == ERROR_MORE_DATA {
                // REV-BE-09: re-query with the reported size instead of dropping.
                let need = len as usize;
                if need > buf.len() && need <= (1 << 20) {
                    buf = vec![0u8; need];
                    len = buf.len() as u32;
                    st = windows::Win32::System::Registry::RegQueryValueExW(
                        hk,
                        PCWSTR(empty.as_ptr()),
                        None,
                        Some(&mut vtype),
                        Some(buf.as_mut_ptr()),
                        Some(&mut len),
                    );
                }
            }
            let _ = RegCloseKey(hk);
            if st != ERROR_SUCCESS {
                return None;
            }
            Some(wstring_from_reg_data(&buf[..len as usize]))
        }
    }
}
