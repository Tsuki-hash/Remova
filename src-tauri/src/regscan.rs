//! Minimal registry helpers for scanner (read-only).

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::ERROR_SUCCESS;
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

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

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
            ("HKLM64", r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths"),
            ("HKLM32", r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths"),
            ("HKCU", r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths"),
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
            if RegOpenKeyExW(hive, PCWSTR(sub_w.as_ptr()), 0, KEY_READ | access, &mut root)
                != ERROR_SUCCESS
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
            if RegOpenKeyExW(hive, PCWSTR(sub_w.as_ptr()), 0, KEY_READ | access, &mut root)
                != ERROR_SUCCESS
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
                if RegEnumValueW(
                    root,
                    n,
                    windows::core::PWSTR(vname.as_mut_ptr()),
                    &mut vname_len,
                    None,
                    Some(&mut vtype),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                ) != ERROR_SUCCESS
                {
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

pub fn read_string_default(key: &str) -> Option<String> {
    #[cfg(not(windows))]
    {
        let _ = key;
        None
    }
    #[cfg(windows)]
    {
        let Some((hive, sub, access)) = parse_alias(key) else {
            return None;
        };
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
            let st = windows::Win32::System::Registry::RegQueryValueExW(
                hk,
                PCWSTR(empty.as_ptr()),
                None,
                Some(&mut vtype),
                Some(buf.as_mut_ptr()),
                Some(&mut len),
            );
            let _ = RegCloseKey(hk);
            if st != ERROR_SUCCESS {
                return None;
            }
            Some(wstring_from_reg_data(&buf[..len as usize]))
        }
    }
}

#[cfg(windows)]
fn wstring_from_reg_data(data: &[u8]) -> String {
    if data.len() < 2 {
        return String::new();
    }
    let mut u16s = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        u16s.push(u16::from_le_bytes([data[i], data[i + 1]]));
        i += 2;
    }
    while u16s.last().copied() == Some(0) {
        u16s.pop();
    }
    String::from_utf16_lossy(&u16s)
}
