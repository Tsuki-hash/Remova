//! Startup / service / scheduled-task listing and enable-disable (P1-2).

use serde::{Deserialize, Serialize};

use crate::safety::{
    allow_manage_reg_write, allow_manage_service_write, is_allowed_startup_approved_key,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManageItem {
    pub name: String,
    pub detail: String,
    pub location: String,
    pub enabled: bool,
}

const RUN_KEYS: &[(&str, &str, &str)] = &[
    (
        "HKLM64",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "64",
    ),
    (
        "HKLM64",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
        "64",
    ),
    (
        "HKLM32",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "32",
    ),
    (
        "HKLM32",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
        "32",
    ),
    (
        "HKCU",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "64",
    ),
    (
        "HKCU",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
        "64",
    ),
    // Policy-driven Run keys (enterprise / some OEM images)
    (
        "HKLM64",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer\Run",
        "64",
    ),
    (
        "HKCU",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer\Run",
        "64",
    ),
];

pub fn list_startup_items() -> Vec<ManageItem> {
    let mut out = Vec::new();
    for (alias, sub, _view) in RUN_KEYS {
        let key = format!(r"{}\{}", alias, sub);
        for (vname, vdata) in crate::regscan::list_values(&key) {
            let display = vname.trim_end_matches(".remova-disabled").to_string();
            // Prefer StartupApproved flag; fall back to legacy rename suffix.
            let enabled = startup_approved_enabled(&key, &display)
                .unwrap_or(!vname.ends_with(".remova-disabled"));
            out.push(ManageItem {
                name: display,
                detail: vdata.chars().take(160).collect(),
                location: format!("{key}::{vname}"),
                enabled,
            });
        }
    }
    // Startup folders (user + common) — .lnk / .exe / .bat etc.
    for folder in startup_folder_paths() {
        let Ok(rd) = std::fs::read_dir(&folder) else {
            continue;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            if !path.is_file() {
                continue;
            }
            let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if fname.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(fname)
                .trim_end_matches(".remova-disabled")
                .to_string();
            let enabled = startup_folder_enabled(&stem)
                .unwrap_or(!fname.to_lowercase().contains(".remova-disabled"));
            out.push(ManageItem {
                name: stem,
                detail: path.display().to_string(),
                location: format!("FOLDER::{folder}::{fname}"),
                enabled,
            });
        }
    }
    // UWP / Store packaged startup tasks (Task Manager “Startup apps”).
    for it in list_packaged_startup() {
        out.push(it);
    }
    // Auto-start (Start=2) user-mode services — consumer tools list these as 开机自启.
    for svc in list_auto_services() {
        out.push(svc);
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name) && a.location == b.location);
    out
}

/// User-mode services with Start=2 (auto), including Microsoft ones (consumers expect them).
fn list_auto_services() -> Vec<ManageItem> {
    let mut out = Vec::new();
    let keys = [
        ("HKLM64", r"SYSTEM\CurrentControlSet\Services"),
        ("HKLM32", r"SYSTEM\CurrentControlSet\Services"),
    ];
    let mut seen = std::collections::HashSet::new();
    for (alias, sub) in keys {
        let root = format!(r"{alias}\{sub}");
        for svc in crate::regscan::list_subkeys(&root) {
            if svc.contains('\\') {
                continue;
            }
            if crate::safety::is_critical_service(&svc) {
                continue;
            }
            if !seen.insert(svc.to_lowercase()) {
                continue;
            }
            let path = format!(r"{root}\{svc}");
            let svc_type = crate::regscan::read_dword(&path, "Type").unwrap_or(0);
            if svc_type & 0xF0 == 0 {
                continue;
            }
            let start = crate::regscan::read_dword(&path, "Start").unwrap_or(3);
            if start != 2 {
                continue;
            }
            let display_name = crate::regscan::read_string(&path, "DisplayName")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| svc.clone());
            out.push(ManageItem {
                name: display_name,
                detail: format!("自动服务 · {svc}"),
                location: format!("SVC::{svc}"),
                enabled: true,
            });
        }
    }
    out
}

/// Enumerate StartupApproved PackagedStartup values (UWP/Store apps).
/// Value name is typically `PackageFamilyName!AppId`; binary flag same layout as Run.
fn list_packaged_startup() -> Vec<ManageItem> {
    let mut out = Vec::new();
    let sa =
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\PackagedStartup";
    for (vname, _) in crate::regscan::list_values(sa) {
        if vname.is_empty() {
            continue;
        }
        let enabled = crate::regscan::read_binary(sa, &vname)
            .map(|b| !(!b.is_empty() && b[0] == 0x03))
            .unwrap_or(true);
        // Resolve display name from SystemAppData when possible.
        let display = resolve_packaged_display_name(&vname).unwrap_or_else(|| vname.clone());
        out.push(ManageItem {
            name: display,
            detail: format!("Store · {vname}"),
            location: format!("PACKAGED::{sa}::{vname}"),
            enabled,
        });
    }
    out
}

/// `PackageFamilyName!AppId` → friendly display from AppModel registry.
fn resolve_packaged_display_name(value_name: &str) -> Option<String> {
    let (pfn, app_id) = value_name.split_once('!')?;
    let root = format!(
        r"HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData\{pfn}"
    );
    // AppId may be a GUID path under SystemAppData\{pfn}\{AppId}
    let key = format!(r"{root}\{app_id}");
    if let Some(d) = crate::regscan::read_string(&key, "DisplayName").filter(|s| !s.is_empty()) {
        return Some(d);
    }
    // Some images store DisplayName on the package root.
    if let Some(d) = crate::regscan::read_string(&root, "DisplayName").filter(|s| !s.is_empty()) {
        return Some(d);
    }
    None
}

/// Serialize manage write-side mutations (services / startup / tasks) — S-08.
static MANAGE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_manage() -> std::sync::MutexGuard<'static, ()> {
    MANAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn startup_folder_paths() -> Vec<String> {
    let mut dirs = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(format!(
            r"{appdata}\Microsoft\Windows\Start Menu\Programs\Startup"
        ));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        dirs.push(format!(
            r"{pd}\Microsoft\Windows\Start Menu\Programs\Startup"
        ));
    }
    dirs
}

/// Task Manager stores folder-shortcut enable flags under StartupApproved\StartupFolder.
fn startup_folder_enabled(file_name: &str) -> Option<bool> {
    crate::regscan::read_binary(
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
        file_name,
    )
    .map(|b| !(!b.is_empty() && b[0] == 0x03))
}

/// Read Explorer StartupApproved\Run binary for a value name. None if missing.
fn startup_approved_enabled(run_key: &str, value_name: &str) -> Option<bool> {
    let low = run_key.to_uppercase().replace('/', "\\");
    let sa_key = if low.contains("\\RUNONCE") {
        return None;
    } else if low.contains("HKLM32") {
        r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    } else if low.starts_with("HKLM") {
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    } else {
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    };
    crate::regscan::read_binary(sa_key, value_name).map(|b| {
        // 0x03 in first byte = disabled; anything else (incl. missing/0x02) = enabled
        !(!b.is_empty() && b[0] == 0x03)
    })
}

pub fn list_services() -> Vec<ManageItem> {
    let mut out = Vec::new();
    let keys = [
        ("HKLM64", r"SYSTEM\CurrentControlSet\Services"),
        ("HKLM32", r"SYSTEM\CurrentControlSet\Services"),
    ];
    let mut seen = std::collections::HashSet::new();
    for (alias, sub) in keys {
        let root = format!(r"{alias}\{sub}");
        for svc in crate::regscan::list_subkeys(&root) {
            if svc.contains('\\') {
                continue;
            }
            if crate::safety::is_critical_service(&svc) {
                continue;
            }
            let path = format!(r"{root}\{svc}");
            if !seen.insert(svc.to_lowercase()) {
                continue;
            }
            // Skip kernel / filesystem drivers (Type bit0/bit1). Keep Win32 process services (0x10/0x20).
            let svc_type = crate::regscan::read_dword(&path, "Type").unwrap_or(0);
            if svc_type & 0xF0 == 0 {
                continue;
            }
            let start = crate::regscan::read_dword(&path, "Start").unwrap_or(3);
            // 2=auto, 3=manual, 4=disabled
            let enabled = start != 4;
            let display_name = crate::regscan::read_string(&path, "DisplayName")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| svc.clone());
            let desc = crate::regscan::read_string(&path, "Description")
                .filter(|s| !s.is_empty())
                .unwrap_or_default();
            let start_label = match start {
                2 => "Auto",
                3 => "Manual",
                4 => "Disabled",
                _ => "Other",
            };
            let detail = if desc.is_empty() {
                format!("{display_name} · {start_label}")
            } else {
                format!("{start_label} · {desc}")
            };
            out.push(ManageItem {
                name: svc,
                detail,
                location: path,
                enabled,
            });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Parse `schtasks /query /fo CSV /v` rows.
/// Verbose CSV column order is stable across locales:
/// 0=HostName, 1=TaskName, 2=NextRunTime, 3=Status, 8=TaskToRun, 10=Comment.
pub fn list_scheduled_tasks() -> Vec<ManageItem> {
    #[cfg(not(windows))]
    {
        Vec::new()
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let mut cmd = Command::new(format!(r"{windir}\System32\schtasks.exe"));
        cmd.args(["/query", "/fo", "CSV", "/v"]);
        crate::regops::hide_console(&mut cmd);
        let Ok(out) = cmd.output() else {
            return Vec::new();
        };
        // schtasks on zh-CN often emits OEM code page, not UTF-8.
        let text = decode_console_bytes(&out.stdout);
        let mut items = Vec::new();
        let mut lines = text.lines();
        // Skip header; positional columns are locale-stable in /fo CSV /v:
        // 0=HostName, 1=TaskName, 2=NextRunTime, 3=Status, 8=TaskToRun, 10=Comment
        let _ = lines.next();
        const IDX_TASK: usize = 1;
        const IDX_STATUS: usize = 3;
        const IDX_RUN: usize = 8;
        const IDX_COMMENT: usize = 10;
        for line in lines {
            let cols = split_csv_line(line);
            let Some(name) = cols.get(IDX_TASK).cloned() else {
                continue;
            };
            if name.is_empty() || name.eq_ignore_ascii_case("taskname") {
                continue;
            }
            let lower = name.to_lowercase();
            if lower.starts_with("\\microsoft\\windows\\") {
                continue;
            }
            let status = cols.get(IDX_STATUS).cloned().unwrap_or_default();
            let run = cols.get(IDX_RUN).cloned().unwrap_or_default();
            let comment = cols.get(IDX_COMMENT).cloned().unwrap_or_default();
            let disabled_markers = [
                "disabled",
                "已禁用",
                "禁用",
                "deaktiviert",
                "désactivé",
                "desactivado",
                "비활성화",
            ];
            let enabled = !disabled_markers
                .iter()
                .any(|m| status.eq_ignore_ascii_case(m));
            let detail = if !comment.is_empty() && comment != "N/A" {
                comment.chars().take(120).collect()
            } else if !run.is_empty() && run != "N/A" {
                run.chars().take(120).collect()
            } else {
                status.clone()
            };
            let location = name.clone();
            items.push(ManageItem {
                name,
                detail,
                location,
                enabled,
            });
        }
        items.sort_by_key(|a| a.name.to_lowercase());
        items.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
        items.truncate(crate::constants::MANAGE_TASK_LIST_CAP);
        items
    }
}

/// Decode console output that may be UTF-8 or OEM code page (schtasks on zh-CN).
fn decode_console_bytes(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::ERROR_INVALID_PARAMETER;
        use windows::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};
        let len = bytes.len() as i32;
        if len <= 0 {
            return String::new();
        }
        let n = unsafe {
            MultiByteToWideChar(
                CP_OEMCP,
                windows::Win32::Globalization::MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0),
                bytes,
                None,
            )
        };
        if n <= 0 {
            let _ = ERROR_INVALID_PARAMETER;
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; n as usize];
        let written = unsafe {
            MultiByteToWideChar(
                CP_OEMCP,
                windows::Win32::Globalization::MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0),
                bytes,
                Some(&mut wide),
            )
        };
        if written <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        String::from_utf16_lossy(&wide[..written as usize])
    }
    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_q && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_q = !in_q;
                }
            }
            ',' if !in_q => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Enable/disable a Run startup value via Explorer `StartupApproved\Run` binary flag.
/// Falls back to renaming a legacy `.remova-disabled` value if present.
///
/// Windows reads every value under Run at logon; only StartupApproved actually blocks it.
/// Binary layout (12 bytes): byte0 = 0x02 enabled / 0x03 disabled.
pub fn set_startup_enabled(location: &str, enabled: bool) -> Result<(), String> {
    if let Some(svc) = location.strip_prefix("SVC::") {
        allow_manage_service_write(svc)?;
        // S-N5: align with set_service_start_disabled — never auto-start (2); manual=3 / disabled=4.
        let start: u32 = if enabled { 3 } else { 4 };
        let _guard = lock_manage();
        return crate::regops::write_service_start(svc, start);
    }
    if let Some(rest) = location.strip_prefix("PACKAGED::") {
        let Some((sa_key, vname)) = rest.rsplit_once("::") else {
            return Err("bad packaged startup location".into());
        };
        if vname.trim().is_empty() || vname.contains('\\') || vname.contains('/') {
            return Err("bad packaged startup value".into());
        }
        allow_manage_reg_write(sa_key, true)?;
        let mut buf = [0u8; 12];
        buf[0] = if enabled { 0x02 } else { 0x03 };
        let _guard = lock_manage();
        return crate::regops::write_reg_binary(sa_key, vname, &buf);
    }
    if let Some(rest) = location.strip_prefix("FOLDER::") {
        let Some((dir, fname)) = rest.rsplit_once("::") else {
            return Err("bad startup folder location".into());
        };
        // S-09: only allow write when dir is one of the known Startup folders.
        let dir_norm = dir.replace('/', "\\");
        let dir_ok = startup_folder_paths()
            .iter()
            .any(|a| dir_norm.eq_ignore_ascii_case(&a.replace('/', "\\")));
        if !dir_ok {
            return Err(crate::error::manage_err("protected_registry", dir).to_ipc());
        }
        let stem = std::path::Path::new(fname)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(fname)
            .trim_end_matches(".remova-disabled")
            .to_string();
        if stem.trim().is_empty() || stem.contains('\\') || stem.contains('/') {
            return Err(crate::error::manage_err("bad_name", fname).to_ipc());
        }
        let _guard = lock_manage();
        return write_startup_folder_approved(&stem, enabled);
    }
    let Some((key, vname)) = location.rsplit_once("::") else {
        return Err("bad startup location".into());
    };
    allow_manage_reg_write(key, false)?;
    if vname.trim().is_empty() {
        return Err("bad startup value name".into());
    }
    let base = vname.trim_end_matches(".remova-disabled");
    let cur_disabled = vname.ends_with(".remova-disabled");
    let _guard = lock_manage();
    if cur_disabled {
        crate::regops::rename_reg_value(key, vname, base)?;
    }
    write_startup_approved(key, base, enabled)
}

fn write_startup_folder_approved(file_stem: &str, enabled: bool) -> Result<(), String> {
    let mut buf = [0u8; 12];
    buf[0] = if enabled { 0x02 } else { 0x03 };
    crate::regops::write_reg_binary(
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
        file_stem,
        &buf,
    )
}

fn write_startup_approved(run_key: &str, value_name: &str, enabled: bool) -> Result<(), String> {
    // Map Run key → matching StartupApproved hive/view.
    // HKCU\...\Run → HKCU\...\Explorer\StartupApproved\Run
    // HKLM64/32\...\Run → same hive Explorer\StartupApproved\Run (Run32 for 32-bit view)
    let low = run_key.to_uppercase().replace('/', "\\");
    let sa_key = if low.contains("\\RUNONCE") {
        // RunOnce has no StartupApproved companion; leave value intact (one-shot).
        return Ok(());
    } else if low.contains("HKLM32") {
        r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    } else if low.starts_with("HKLM") {
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    } else {
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    };
    if !is_allowed_startup_approved_key(sa_key) {
        return Err(crate::error::manage_err("protected_registry", sa_key).to_ipc());
    }
    allow_manage_reg_write(sa_key, true)?;

    let mut buf = [0u8; 12];
    buf[0] = if enabled { 0x02 } else { 0x03 };
    crate::regops::write_reg_binary(sa_key, value_name, &buf)
}

/// Set service Start=4 (disabled) or 3 (manual) — not auto to avoid surprise.
pub fn set_service_start_disabled(name: &str, disable: bool) -> Result<(), String> {
    allow_manage_service_write(name)?;
    let start: u32 = if disable { 4 } else { 3 };
    let _guard = lock_manage();
    crate::regops::write_service_start(name, start)
}

pub fn set_task_enabled(task_name: &str, enabled: bool) -> Result<(), String> {
    if task_name.trim().is_empty() {
        return Err("empty task".into());
    }
    let _guard = lock_manage();
    // S-02: mirror list-side filter — never disable Microsoft\Windows system tasks.
    let low = task_name.replace('/', "\\").to_lowercase();
    if low.starts_with("\\microsoft\\windows\\") || low.starts_with("microsoft\\windows\\") {
        return Err(crate::error::manage_err("protected_task", task_name).to_ipc());
    }
    #[cfg(not(windows))]
    {
        let _ = (task_name, enabled);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let action = if enabled { "/enable" } else { "/disable" };
        let mut cmd = Command::new(format!(r"{windir}\System32\schtasks.exe"));
        // No manual quotes: std::process::Command already quotes args containing spaces.
        // Manual quotes make schtasks look for a name with literal quote chars (NEW-A).
        cmd.args(["/change", "/tn", task_name, action]);
        crate::regops::hide_console(&mut cmd);
        let st = cmd.output().map_err(|e| e.to_string())?;
        if st.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&st.stderr).trim().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn csv_split_quotes() {
        let cols = super::split_csv_line(r#""C:\a","say ""hi""","x""#);
        assert_eq!(cols[0], r"C:\a");
        assert_eq!(cols[1], r#"say "hi""#);
        assert_eq!(cols[2], "x");
    }

    #[cfg(windows)]
    #[test]
    fn list_services_skips_critical() {
        let svcs = super::list_services();
        for s in &svcs {
            assert!(!crate::safety::is_critical_service(&s.name));
        }
    }

    #[test]
    fn packaged_location_whitelist() {
        let good = format!(
            "PACKAGED::{}::PackageFamily!App",
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\PackagedStartup"
        );
        // Good key is allowed to reach write_reg_binary — may fail later on missing value; policy check is first.
        // Bad key must fail at policy gate before any registry write.
        let bad = r"PACKAGED::HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run::Evil";
        assert!(super::set_startup_enabled(bad, false).is_err());
        let bad2 =
            r"PACKAGED::HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\evil.exe::x";
        assert!(super::set_startup_enabled(bad2, false).is_err());
        let _ = good;
    }

    #[test]
    fn run_location_rejects_non_run_keys() {
        let loc = r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders::Startup";
        assert!(super::set_startup_enabled(loc, false).is_err());
        let loc2 = r"HKLM\SOFTWARE\EvilCorp\Config::payload";
        assert!(super::set_startup_enabled(loc2, true).is_err());
    }

    #[test]
    fn system_scheduled_task_write_rejected() {
        assert!(
            super::set_task_enabled(r"\Microsoft\Windows\Defrag\ScheduledDefrag", false).is_err()
        );
        assert!(super::set_task_enabled("Microsoft\\Windows\\Update", true).is_err());
    }

    #[test]
    fn folder_startup_location_rejects_unknown_dir() {
        // S-09: FOLDER:: must not accept arbitrary directories.
        let loc = r"FOLDER::C:\Temp\NotStartup::evil.exe";
        let err = super::set_startup_enabled(loc, false).unwrap_err();
        assert!(
            err.contains("protected_registry") || err.contains("bad startup"),
            "got {err}"
        );
        let bad = r"FOLDER::missing-separators";
        assert!(super::set_startup_enabled(bad, false).is_err());
    }

    #[test]
    fn critical_services_rejected_by_manage() {
        for name in [
            "WinDefend",
            "windefend",
            "Appinfo",
            "DcomLaunch",
            "Power",
            "ProfSvc",
        ] {
            assert!(
                super::set_service_start_disabled(name, true).is_err(),
                "expected {name} to be protected"
            );
            assert!(
                super::set_startup_enabled(&format!("SVC::{name}"), false).is_err(),
                "expected SVC::{name} to be protected"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn list_startup_runs() {
        let items = super::list_startup_items();
        for i in &items {
            assert!(i.location.contains("::"));
        }
        // Machine typically has more than one Run value or startup-folder entry.
        // Empty list is still valid on a locked-down VM, so only assert shape.
        assert!(items.len() < 10_000);
    }

    #[test]
    fn startup_folder_paths_nonempty_on_windows() {
        #[cfg(windows)]
        {
            assert!(!super::startup_folder_paths().is_empty());
        }
    }

    #[test]
    fn schtasks_verbose_csv_uses_taskname_column_not_hostname() {
        // HostName is col 0; TaskName is col 1 — regression for laptop-name-as-task bug.
        let header = r#""HostName","TaskName","Next Run Time","Status","Logon Mode","Last Run Time","Last Result","Author","Task To Run","Start In","Comment","Scheduled Task State""#;
        let row = r#""MYLAPTOP","\Vendor\Cleanup","N/A","Ready","Interactive only","N/A","0","Vendor","C:\tools\cleanup.exe","C:\","Cleanup temp","Enabled""#;
        let cols = super::split_csv_line(row);
        assert_eq!(cols.first().map(String::as_str), Some("MYLAPTOP"));
        assert_eq!(cols.get(1).map(String::as_str), Some(r"\Vendor\Cleanup"));
        assert_eq!(cols.get(3).map(String::as_str), Some("Ready"));
        assert_eq!(
            cols.get(8).map(String::as_str),
            Some(r"C:\tools\cleanup.exe")
        );
        let _ = header;
    }
}
