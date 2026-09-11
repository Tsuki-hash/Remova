//! Enumerate Windows uninstall registry entries (read-only).

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledApp {
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub install_location: String,
    pub uninstall_string: String,
    pub quiet_uninstall_string: String,
    pub source: String,
    pub registry_key: String,
    pub estimated_size_kb: i64,
}

/// Scan HKLM64 / HKLM32 / HKCU Uninstall keys (canonical view paths, no WOW6432Node in path).
pub fn scan_installed_apps() -> Vec<InstalledApp> {
    let mut out: Vec<InstalledApp> = Vec::new();
    #[cfg(windows)]
    {
        let sources: [(&str, HKEY, &str, REG_SAM_FLAGS); 3] = [
            (
                "HKLM64",
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
                KEY_WOW64_64KEY,
            ),
            (
                "HKLM32",
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
                KEY_WOW64_32KEY,
            ),
            (
                "HKCU",
                HKEY_CURRENT_USER,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
                KEY_WOW64_64KEY,
            ),
        ];
        for (alias, hive, sub, access) in sources {
            collect_uninstall(hive, sub, access, alias, &mut out);
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| {
        a.name.eq_ignore_ascii_case(&b.name)
            && a.version == b.version
            && a.uninstall_string.eq_ignore_ascii_case(&b.uninstall_string)
    });
    out
}

#[cfg(windows)]
fn collect_uninstall(
    hive: HKEY,
    sub: &str,
    access: REG_SAM_FLAGS,
    alias: &str,
    out: &mut Vec<InstalledApp>,
) {
    unsafe {
        let mut root = HKEY::default();
        let sub_w = to_wide(sub);
        let st = RegOpenKeyExW(hive, PCWSTR(sub_w.as_ptr()), 0, KEY_READ | access, &mut root);
        if st != ERROR_SUCCESS {
            return;
        }

        let mut count = 0u32;
        let mut max_sub = 0u32;
        let mut max_val = 0u32;
        let _ = RegQueryInfoKeyW(
            root,
            windows::core::PWSTR::null(),
            None,
            None,
            Some(&mut count),
            Some(&mut max_sub),
            None,
            Some(&mut max_val),
            None,
            None,
            None,
            None,
        );

        for i in 0..count {
            let mut name_buf = vec![0u16; (max_sub as usize) + 2];
            let mut name_len = name_buf.len() as u32;
            let st = RegEnumKeyExW(
                root,
                i,
                windows::core::PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                windows::core::PWSTR::null(),
                None,
                None,
            );
            if st != ERROR_SUCCESS {
                continue;
            }
            name_buf.truncate(name_len as usize);
            let key_name = String::from_utf16_lossy(&name_buf);
            if let Some(app) = read_uninstall_entry(hive, sub, access, alias, &key_name, max_val) {
                out.push(app);
            }
        }
        let _ = RegCloseKey(root);
    }
}

#[cfg(windows)]
unsafe fn read_uninstall_entry(
    hive: HKEY,
    sub: &str,
    access: REG_SAM_FLAGS,
    alias: &str,
    key_name: &str,
    max_val: u32,
) -> Option<InstalledApp> {
    let path = format!(r"{}\{}", sub, key_name);
    let path_w = to_wide(&path);
    let mut hk = HKEY::default();
    let st = RegOpenKeyExW(hive, PCWSTR(path_w.as_ptr()), 0, KEY_READ | access, &mut hk);
    if st != ERROR_SUCCESS {
        return None;
    }

    let mut display = String::new();
    let mut version = String::new();
    let mut publisher = String::new();
    let mut install_location = String::new();
    let mut uninstall_string = String::new();
    let mut quiet = String::new();
    let mut size_kb = 0i64;

    let mut n = 0u32;
    loop {
        let mut vname = vec![0u16; (max_val as usize).max(64) + 2];
        let mut vname_len = vname.len() as u32;
        let mut vtype = 0u32;
        let mut data: Vec<u8> = vec![0u8; 4096];
        let mut data_len = data.len() as u32;
        let st = RegEnumValueW(
            hk,
            n,
            windows::core::PWSTR(vname.as_mut_ptr()),
            &mut vname_len,
            None,
            Some(&mut vtype),
            Some(data.as_mut_ptr()),
            Some(&mut data_len),
        );
        if st != ERROR_SUCCESS {
            break;
        }
        n += 1;
        let name = String::from_utf16_lossy(&vname[..vname_len as usize]);
        if vtype == REG_SZ.0 || vtype == REG_EXPAND_SZ.0 {
            let s = wstring_from_reg_data(&data[..data_len as usize]);
            match name.as_str() {
                "DisplayName" => display = s,
                "DisplayVersion" => version = s,
                "Publisher" => publisher = s,
                "InstallLocation" => install_location = s.trim_end_matches(['\\', '/']).to_string(),
                "UninstallString" => uninstall_string = s,
                "QuietUninstallString" => quiet = s,
                "EstimatedSize" => {
                    if data_len >= 4 {
                        size_kb = i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as i64;
                    }
                }
                _ => {}
            }
        }
    }
    let _ = RegCloseKey(hk);

    if display.trim().is_empty() || (uninstall_string.trim().is_empty() && quiet.trim().is_empty()) {
        return None;
    }
    if looks_system_update(&display) {
        return None;
    }

    let registry_key = format!(
        r"{}\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{}",
        alias, key_name
    );
    Some(InstalledApp {
        name: display,
        version,
        publisher,
        install_location,
        uninstall_string,
        quiet_uninstall_string: quiet,
        source: alias.to_string(),
        registry_key,
        estimated_size_kb: size_kb,
    })
}

fn looks_system_update(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return true;
    }
    let lower = n.to_lowercase();
    if lower.starts_with("kb") && n.len() > 2 && n[2..].chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    lower.starts_with("update for") || lower.starts_with("security update")
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn wstring_from_reg_data(data: &[u8]) -> String {
    if data.len() < 2 {
        return String::new();
    }
    let mut u16s: Vec<u16> = Vec::with_capacity(data.len() / 2);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_update_filter() {
        assert!(looks_system_update("KB5021234"));
        assert!(looks_system_update("Update for Windows 10"));
        assert!(!looks_system_update("7-Zip 24.08"));
    }

    #[cfg(windows)]
    #[test]
    fn scan_returns_list() {
        let apps = scan_installed_apps();
        for a in &apps {
            assert!(!a.name.trim().is_empty());
            assert!(!a.registry_key.is_empty());
        }
    }
}
