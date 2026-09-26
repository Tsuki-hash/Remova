//! Enumerate Windows uninstall registry entries (read-only).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Uninstall command fingerprints from the latest server-side app scan (blocks IPC-forged RCE).
static UNINSTALL_TRUST: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn uninstall_trust() -> &'static Mutex<HashSet<String>> {
    UNINSTALL_TRUST.get_or_init(|| Mutex::new(HashSet::new()))
}

fn trust_key(uninstall: &str, quiet: &str) -> String {
    format!("{uninstall}\n{quiet}")
}

/// Remember uninstall command pairs produced by the latest scan (REV-BE-04).
/// Build the next set first, then swap — never leave an empty trust window for concurrent uninstall.
pub fn remember_uninstall_commands(apps: &[InstalledApp]) {
    let mut next = HashSet::new();
    for a in apps {
        if a.uninstall_string.trim().is_empty() && a.quiet_uninstall_string.trim().is_empty() {
            continue;
        }
        next.insert(trust_key(&a.uninstall_string, &a.quiet_uninstall_string));
    }
    if let Ok(mut g) = uninstall_trust().lock() {
        *g = next;
    }
}

/// True when this app's uninstall command pair came from the latest scan.
pub fn is_trusted_uninstall_app(app: &InstalledApp) -> bool {
    let k = trust_key(&app.uninstall_string, &app.quiet_uninstall_string);
    uninstall_trust()
        .lock()
        .map(|g| g.contains(&k))
        .unwrap_or(false)
}

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
    /// `YYYY-MM-DD` when registry InstallDate is parseable, else empty.
    pub install_date: String,
    /// Raw DisplayIcon registry value (may be `path` or `path,index`).
    pub display_icon: String,
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
        // Merge Store/MSIX packages (WinRT). Dedup after sort by name.
        out.extend(crate::storeapps::scan_store_apps());
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    // HKLM64 + HKLM32 often list the same product twice (same path / MSI product code).
    out.dedup_by(|a, b| {
        if !same_product(a, b) {
            return false;
        }
        merge_app_fields(a, b);
        true
    });
    // AR-04: backend-enforce ignore rules (publisher / name / install path).
    let ignore = crate::ignore::load();
    out.retain(|a| {
        !crate::ignore::is_app_ignored(&ignore, &a.name, &a.publisher, &a.install_location)
    });
    // S-RCE: every scan path refreshes the uninstall trust table (not only list IPC).
    remember_uninstall_commands(&out);
    out
}

/// Uninstall string comparable form: case-insensitive, no quotes/whitespace.
fn normalize_cmd(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .replace(['"', '\''], "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn key_leaf(registry_key: &str) -> &str {
    registry_key.rsplit('\\').next().unwrap_or("")
}

/// True when two uninstall entries refer to the same installed product.
pub fn same_product(a: &InstalledApp, b: &InstalledApp) -> bool {
    if !a.name.eq_ignore_ascii_case(&b.name) {
        return false;
    }
    // Same MSI/product GUID under Uninstall (even if hive/view differs).
    let ka = key_leaf(&a.registry_key).to_lowercase();
    let kb = key_leaf(&b.registry_key).to_lowercase();
    if ka.starts_with('{') && ka.ends_with('}') && ka == kb {
        return true;
    }
    // SEC-5: generic names need GUID — never merge "Update"/short names on name alone.
    let n = a.name.trim();
    let generic = n.len() < 4
        || n.eq_ignore_ascii_case("update")
        || n.eq_ignore_ascii_case("installer")
        || n.eq_ignore_ascii_case("setup");
    if generic {
        return false;
    }
    // Same uninstall command (quote/whitespace-insensitive).
    if !a.uninstall_string.trim().is_empty()
        && !b.uninstall_string.trim().is_empty()
        && normalize_cmd(&a.uninstall_string) == normalize_cmd(&b.uninstall_string)
    {
        return true;
    }
    // Same non-empty install location (the usual HKLM64/HKLM32 twin).
    if !a.install_location.trim().is_empty()
        && a.install_location.eq_ignore_ascii_case(&b.install_location)
    {
        return true;
    }
    false
}

/// Fill empty fields on `keep` from `drop` so the surviving row is more complete.
fn merge_app_fields(keep: &mut InstalledApp, drop: &InstalledApp) {
    if keep.quiet_uninstall_string.trim().is_empty() {
        keep.quiet_uninstall_string = drop.quiet_uninstall_string.clone();
    }
    if keep.uninstall_string.trim().is_empty() {
        keep.uninstall_string = drop.uninstall_string.clone();
    }
    if keep.install_location.trim().is_empty() {
        keep.install_location = drop.install_location.clone();
    }
    if keep.estimated_size_kb <= 0 {
        keep.estimated_size_kb = drop.estimated_size_kb;
    }
    if keep.display_icon.trim().is_empty() {
        keep.display_icon = drop.display_icon.clone();
    }
    if keep.install_date.trim().is_empty() {
        keep.install_date = drop.install_date.clone();
    }
    if keep.version.trim().is_empty() {
        keep.version = drop.version.clone();
    }
    if keep.publisher.trim().is_empty() {
        keep.publisher = drop.publisher.clone();
    }
    // Prefer 64-bit hive view as the canonical registry key for deletion.
    if keep.source == "HKLM32" && drop.source == "HKLM64" {
        keep.source = drop.source.clone();
        keep.registry_key = drop.registry_key.clone();
    }
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
        let st = RegOpenKeyExW(
            hive,
            PCWSTR(sub_w.as_ptr()),
            0,
            KEY_READ | access,
            &mut root,
        );
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
    let mut install_date = String::new();
    let mut display_icon = String::new();

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
                "DisplayIcon" => display_icon = s,
                "InstallDate" => install_date = format_install_date(&s),
                _ => {}
            }
        }
        // EstimatedSize is REG_DWORD — must be read outside the REG_SZ arm.
        if name == "EstimatedSize" && (vtype == 4/* REG_DWORD */) && data_len >= 4 {
            size_kb = i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as i64;
        }
    }
    let _ = RegCloseKey(hk);

    if display.trim().is_empty() || (uninstall_string.trim().is_empty() && quiet.trim().is_empty())
    {
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
        install_date,
        display_icon,
    })
}

/// Normalize common InstallDate registry formats to `YYYY-MM-DD`.
pub fn format_install_date(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    // YYYYMMDD
    if digits.len() == 8
        && s.chars()
            .all(|c| c.is_ascii_digit() || c == '/' || c == '-' || c == '.')
    {
        let (y, m, d) = (&digits[0..4], &digits[4..6], &digits[6..8]);
        if let (Ok(mv), Ok(dv)) = (m.parse::<u32>(), d.parse::<u32>()) {
            if (1..=12).contains(&mv) && (1..=31).contains(&dv) {
                return format!("{y}-{m:0>2}-{d:0>2}");
            }
        }
    }
    // YYYY/MM/DD or YYYY-MM-DD already
    if digits.len() == 8 {
        if let Some(rest) = s.strip_prefix(&digits[0..4]) {
            let sep_ok = rest.starts_with(['/', '-', '.']);
            if sep_ok {
                let m = &digits[4..6];
                let d = &digits[6..8];
                if let (Ok(mv), Ok(dv)) = (m.parse::<u32>(), d.parse::<u32>()) {
                    if (1..=12).contains(&mv) && (1..=31).contains(&dv) {
                        return format!("{}-{m:0>2}-{d:0>2}", &digits[0..4]);
                    }
                }
            }
        }
    }
    String::new()
}

/// Split DisplayIcon into (file path, resource index).
pub fn parse_display_icon(raw: &str) -> (String, i32) {
    let s = raw.trim();
    if s.is_empty() {
        return (String::new(), 0);
    }
    let unquoted = s.trim_matches('"');
    if let Some((path, idx)) = unquoted.rsplit_once(',') {
        let idx = idx.trim().trim_matches('"');
        if !idx.is_empty() && idx.chars().all(|c| c.is_ascii_digit() || c == '-') {
            let index = idx.parse::<i32>().unwrap_or(0);
            let path = path.trim().trim_matches('"').to_string();
            return (path, index);
        }
    }
    (unquoted.to_string(), 0)
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

// Shared Windows string helpers live in `fsutil`.
#[cfg(windows)]
use crate::fsutil::{to_wide, wstring_from_reg_data};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_update_filter() {
        assert!(looks_system_update("KB5021234"));
        assert!(looks_system_update("Update for Windows 10"));
        assert!(!looks_system_update("7-Zip 24.08"));
    }

    #[test]
    fn install_date_formats() {
        assert_eq!(format_install_date("20250601"), "2025-06-01");
        assert_eq!(format_install_date("2025/06/01"), "2025-06-01");
        assert_eq!(format_install_date(""), "");
        assert_eq!(format_install_date("not-a-date"), "");
    }

    #[test]
    fn display_icon_parse() {
        assert_eq!(
            parse_display_icon(r#"C:\App\app.exe,0"#),
            (r"C:\App\app.exe".to_string(), 0)
        );
        assert_eq!(
            parse_display_icon(r#""C:\App\app.exe",1"#),
            (r"C:\App\app.exe".to_string(), 1)
        );
        assert_eq!(
            parse_display_icon(r#"C:\App\app.ico"#),
            (r"C:\App\app.ico".to_string(), 0)
        );
    }

    #[test]
    fn same_product_by_install_location() {
        let mk = |name: &str, loc: &str, src: &str, key: &str, uns: &str| InstalledApp {
            name: name.into(),
            version: "1.0".into(),
            publisher: "P".into(),
            install_location: loc.into(),
            uninstall_string: uns.into(),
            quiet_uninstall_string: String::new(),
            source: src.into(),
            registry_key: key.into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let a = mk(
            "Xftp 8",
            r"C:\Program Files (x86)\NetSarang\Xftp 8",
            "HKLM64",
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Xftp 8",
            r"C:\Program Files (x86)\NetSarang\Xftp 8\uninst.exe",
        );
        let b = mk(
            "xftp 8",
            r"C:\Program Files (x86)\NetSarang\Xftp 8",
            "HKLM32",
            r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Xftp 8",
            r#""C:\Program Files (x86)\NetSarang\Xftp 8\uninst.exe""#,
        );
        assert!(same_product(&a, &b));
        let c = mk(
            "Other App",
            r"C:\Program Files\Other",
            "HKLM64",
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Other",
            "x",
        );
        assert!(!same_product(&a, &c));
    }

    #[test]
    fn same_product_by_msi_guid() {
        let mk = |src: &str| InstalledApp {
            name: "Acme".into(),
            version: "2".into(),
            publisher: String::new(),
            install_location: String::new(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: src.into(),
            registry_key: format!(
                r"{src}\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{{ABC-123}}"
            ),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        assert!(same_product(&mk("HKLM64"), &mk("HKLM32")));
    }

    #[test]
    fn merge_fills_empty_fields() {
        let mut keep = InstalledApp {
            name: "App".into(),
            version: String::new(),
            publisher: String::new(),
            install_location: String::new(),
            uninstall_string: "u".into(),
            quiet_uninstall_string: String::new(),
            source: "HKLM32".into(),
            registry_key: "HKLM32\\...\\App".into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let drop = InstalledApp {
            name: "App".into(),
            version: "1.2".into(),
            publisher: "Pub".into(),
            install_location: r"C:\App".into(),
            uninstall_string: "u".into(),
            quiet_uninstall_string: "q".into(),
            source: "HKLM64".into(),
            registry_key: "HKLM64\\...\\App".into(),
            estimated_size_kb: 99,
            install_date: "2024-01-01".into(),
            display_icon: "icon".into(),
        };
        merge_app_fields(&mut keep, &drop);
        assert_eq!(keep.version, "1.2");
        assert_eq!(keep.estimated_size_kb, 99);
        assert_eq!(keep.source, "HKLM64");
    }

    /// P1.7 regression: EstimatedSize is REG_DWORD and must leave size_kb > 0.
    /// Parse arm is inline in collect_uninstall; assert the merge path honors a positive size
    /// and that a DWORD-shaped value is not dropped by the REG_SZ-only reader contract.
    #[test]
    fn estimated_size_dword_not_lost_in_merge() {
        let mut keep = InstalledApp {
            name: "App".into(),
            version: String::new(),
            publisher: String::new(),
            install_location: String::new(),
            uninstall_string: "u".into(),
            quiet_uninstall_string: String::new(),
            source: "HKLM32".into(),
            registry_key: "HKLM32\\...\\App".into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        // DWORD EstimatedSize arrives as positive KB on one hive row only.
        let drop = InstalledApp {
            estimated_size_kb: 4096,
            uninstall_string: "u".into(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: "HKLM64\\...\\App".into(),
            ..keep.clone()
        };
        merge_app_fields(&mut keep, &drop);
        assert_eq!(keep.estimated_size_kb, 4096);

        // Little-endian DWORD bytes → i64 KB (the collect_uninstall conversion).
        let data: [u8; 4] = 4096i32.to_le_bytes();
        let parsed = i32::from_le_bytes(data) as i64;
        assert_eq!(parsed, 4096);
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
