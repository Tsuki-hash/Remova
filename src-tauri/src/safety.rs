//! Safety guards — port of Python safety.py for later delete paths.
//! Read-only phase: functions are unit-tested here for parity.

/// Critical Windows service names that must never be deleted.
pub fn critical_service_names() -> &'static [&'static str] {
    &[
        "eventlog",
        "dhcp",
        "dnscache",
        "lanmanserver",
        "lanmanworkstation",
        "winmgmt",
        "wuauserv",
        "bits",
        "cryptsvc",
        "trustedinstaller",
        "wscsvc",
        "mpssvc",
        "rpcss",
        "schedule",
        "spooler",
        "themes",
    ]
}

fn normalize_hklm(key_path: &str) -> String {
    let low = key_path.replace('/', "\\").to_uppercase();
    let low = low.strip_prefix("HKLM64\\").map(|s| format!("HKLM\\{s}")).unwrap_or(low);
    let low = low
        .strip_prefix("HKLM32\\")
        .map(|s| format!("HKLM\\{s}"))
        .unwrap_or(low);
    low
}

/// Final gate for registry key/value paths (same shape as Python `is_safe_to_delete_registry`).
pub fn is_safe_to_delete_registry(key_path: &str) -> Result<(), String> {
    if key_path.trim().is_empty() {
        return Err("empty registry path".into());
    }
    let low = normalize_hklm(key_path);

    let uninstall_roots = [
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\UNINSTALL",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\UNINSTALL",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\UNINSTALL",
        "HKCU\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\UNINSTALL",
    ];
    for root in uninstall_roots {
        if low == root {
            return Err("uninstall root protected".into());
        }
        if low.starts_with(&format!("{root}\\")) {
            return Ok(());
        }
    }

    let app_paths_roots = [
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\APP PATHS",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\APP PATHS",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\APP PATHS",
        "HKCU\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\APP PATHS",
    ];
    for root in app_paths_roots {
        if low == root {
            return Err("app paths root protected".into());
        }
        if low.starts_with(&format!("{root}\\")) {
            return Ok(());
        }
    }

    // Services: direct child only; critical names blocked
    let services_root = "HKLM\\SYSTEM\\CURRENTCONTROLSET\\SERVICES";
    if low == services_root {
        return Err("services root protected".into());
    }
    if let Some(rest) = low.strip_prefix(&format!("{services_root}\\")) {
        if rest.is_empty() || rest.contains('\\') {
            return Err("only top-level service keys allowed".into());
        }
        if critical_service_names()
            .iter()
            .any(|n| rest == n.to_uppercase())
        {
            return Err("critical system service protected".into());
        }
        return Ok(());
    }

    // Task Cache tree: block Microsoft\, allow vendor tasks
    let task_root =
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS NT\\CURRENTVERSION\\SCHEDULE\\TASKCACHE\\TREE";
    if low == task_root {
        return Err("task tree root protected".into());
    }
    if let Some(rest) = low.strip_prefix(&format!("{task_root}\\")) {
        if rest == "MICROSOFT" || rest.starts_with("MICROSOFT\\") {
            return Err("system scheduled tasks protected".into());
        }
        return Ok(());
    }

    // Run/RunOnce values: key|ValueName only, never the key itself
    let run_roots = [
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
    ];
    if let Some((key, _val)) = key_path.rsplit_once('|') {
        let key_low = normalize_hklm(key);
        for root in run_roots {
            if key_low == root {
                return Ok(());
            }
        }
    }
    for root in run_roots {
        if low == root {
            return Err("run root protected".into());
        }
    }

    // PROTECTED prefixes (Python parity)
    let protected = [
        "HKLM\\SYSTEM",
        "HKLM\\HARDWARE",
        "HKLM\\SAM",
        "HKLM\\SECURITY",
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS",
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS NT",
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION",
        "HKLM\\SOFTWARE\\MICROSOFT\\CRYPTOGRAPHY",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION",
    ];
    for pref in protected {
        if low == pref || low.starts_with(&format!("{pref}\\")) {
            return Err("protected registry prefix".into());
        }
    }

    Ok(())
}

/// Unified filesystem safety gate (scanner + executor).
/// Rejects protected prefixes, drive roots (`C:` / `C:\`), and shallow paths.
pub fn is_safe_fs(p: &std::path::Path) -> bool {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let trimmed = s.trim_end_matches('\\');
    // Drive root: "c:" or "c:\"
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return false;
    }
    // Shallow: must be at least `drive:\dir\file-or-dir` (4 components on Windows).
    let comps = p.components().count();
    if comps < 4 {
        return false;
    }
    let protected = [
        r"c:\windows",
        r"c:\windows.old",
        r"c:\programdata\microsoft",
        r"c:\program files\windowsapps",
        r"c:\program files\common files\microsoft shared",
        r"c:\program files (x86)\common files\microsoft shared",
        r"c:\users\default",
    ];
    !protected
        .iter()
        .any(|pref| s == *pref || s.starts_with(&format!("{pref}\\")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn fs_rejects_drive_root() {
        assert!(!is_safe_fs(Path::new(r"C:\")));
        assert!(!is_safe_fs(Path::new("C:")));
        assert!(!is_safe_fs(Path::new(r"D:\")));
    }

    #[test]
    fn fs_rejects_shallow() {
        assert!(!is_safe_fs(Path::new(r"C:\foo")));
    }

    #[test]
    fn fs_rejects_protected() {
        assert!(!is_safe_fs(Path::new(r"C:\Windows\System32")));
        assert!(!is_safe_fs(Path::new(r"c:\programdata\microsoft\x")));
    }

    #[test]
    fn fs_allows_normal_install() {
        assert!(is_safe_fs(Path::new(r"C:\Program Files\MyApp")));
        assert!(is_safe_fs(Path::new(r"D:\Games\SomeGame")));
    }

    #[test]
    fn uninstall_product_key_ok() {
        assert!(is_safe_to_delete_registry(
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{ABC}"
        )
        .is_ok());
    }

    #[test]
    fn uninstall_root_blocked() {
        assert!(is_safe_to_delete_registry(
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
        )
        .is_err());
    }

    #[test]
    fn critical_service_blocked() {
        assert!(is_safe_to_delete_registry(
            r"HKLM64\SYSTEM\CurrentControlSet\Services\Winmgmt"
        )
        .is_err());
    }

    #[test]
    fn vendor_service_ok() {
        assert!(is_safe_to_delete_registry(
            r"HKLM64\SYSTEM\CurrentControlSet\Services\DemoVendorHelper"
        )
        .is_ok());
    }

    #[test]
    fn microsoft_task_blocked() {
        assert!(is_safe_to_delete_registry(
            r"HKLM64\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\Microsoft"
        )
        .is_err());
    }

    #[test]
    fn vendor_task_ok() {
        assert!(is_safe_to_delete_registry(
            r"HKLM64\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\DemoVendor"
        )
        .is_ok());
    }

    #[test]
    fn run_root_blocked_value_ok() {
        let root = r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
        assert!(is_safe_to_delete_registry(root).is_err());
        assert!(is_safe_to_delete_registry(&format!("{root}|DemoApp")).is_ok());
    }

    #[test]
    fn hklm32_normalize_matches_64() {
        assert!(is_safe_to_delete_registry(
            r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{ABC}"
        )
        .is_ok());
    }
}
