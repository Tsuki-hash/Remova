//! Safety guards — port of Python safety.py for later delete paths.
//! Read-only phase: functions are unit-tested here for parity.

/// Critical Windows service names that must never be deleted or disabled via manage IPC.
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
        // Manage-path expansions (AR-02): core OS / security / session services.
        "appinfo",
        "profsvc",
        "dcomlaunch",
        "power",
        "windefend",
        "securityhealthservice",
        "eventsystem",
        "lsmsm",
        "samss",
        "netlogon",
        "msiserver",
        "rpceptmapper",
        "gpsvc",
        "iphlpsvc",
        "nsi",
        "sens",
        "shellhwdetection",
        "trkwks",
        "wcmsvc",
        "wlansvc",
        "wudfsvc",
    ]
}

/// True when `name` is a critical system service (case-insensitive).
pub fn is_critical_service(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return true;
    }
    critical_service_names()
        .iter()
        .any(|c| c.eq_ignore_ascii_case(n))
}

/// Manage IPC may only write REG_BINARY under these StartupApproved roots.
pub fn is_allowed_startup_approved_key(key_path: &str) -> bool {
    let low = key_path.trim().replace('/', "\\").to_uppercase();
    const ALLOWED: &[&str] = &[
        r"HKCU\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\EXPLORER\STARTUPAPPROVED\PACKAGEDSTARTUP",
        r"HKCU\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\EXPLORER\STARTUPAPPROVED\STARTUPFOLDER",
        r"HKCU\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\EXPLORER\STARTUPAPPROVED\RUN",
        r"HKLM\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\EXPLORER\STARTUPAPPROVED\RUN",
        r"HKLM64\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\EXPLORER\STARTUPAPPROVED\RUN",
        r"HKLM32\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\EXPLORER\STARTUPAPPROVED\RUN",
        r"HKLM\SOFTWARE\WOW6432NODE\MICROSOFT\WINDOWS\CURRENTVERSION\EXPLORER\STARTUPAPPROVED\RUN",
    ];
    ALLOWED.iter().any(|a| low == *a)
}

/// Run / RunOnce keys that manage may enable/disable (StartupApproved companions).
pub fn is_allowed_run_key(key_path: &str) -> bool {
    let low = key_path.trim().replace('/', "\\").to_uppercase();
    // Normalized HKLM64/HKLM32 → HKLM for comparison.
    let low = low
        .strip_prefix("HKLM64\\")
        .map(|s| format!("HKLM\\{s}"))
        .unwrap_or(low);
    let low = low
        .strip_prefix("HKLM32\\")
        .map(|s| format!(r"HKLM\WOW6432NODE\{s}"))
        .unwrap_or(low);
    const ALLOWED: &[&str] = &[
        r"HKLM\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\RUN",
        r"HKLM\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\RUNONCE",
        r"HKLM\SOFTWARE\WOW6432NODE\MICROSOFT\WINDOWS\CURRENTVERSION\RUN",
        r"HKLM\SOFTWARE\WOW6432NODE\MICROSOFT\WINDOWS\CURRENTVERSION\RUNONCE",
        r"HKCU\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\RUN",
        r"HKCU\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\RUNONCE",
        r"HKLM\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\POLICIES\EXPLORER\RUN",
        r"HKCU\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\POLICIES\EXPLORER\RUN",
    ];
    ALLOWED.iter().any(|a| low == *a)
}

/// Unified write-policy for manage IPC (AR-06 light): critical service + startup-approved keys.
pub fn allow_manage_service_write(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() || n.contains('\\') || n.contains('/') {
        return Err(crate::error::manage_err("bad_name", "bad service name").to_ipc());
    }
    if is_critical_service(n) {
        return Err(crate::error::manage_err("protected", n).to_ipc());
    }
    // S-R4-09 / S-R6-08: Microsoft-family services (MSMQ, MsSqlServer, Microsoft*, MS *).
    let low = n.to_lowercase();
    if low.starts_with("microsoft")
        || low.starts_with("ms ")
        || low == "msmq"
        || low.starts_with("mssql")
        || low.starts_with("msdtc")
        || low.starts_with("mspq")
        || low.starts_with("mstee")
    {
        return Err(crate::error::manage_err("protected", n).to_ipc());
    }
    Ok(())
}

pub fn allow_manage_reg_write(
    key_path: &str,
    require_startup_approved: bool,
) -> Result<(), String> {
    if require_startup_approved {
        if !is_allowed_startup_approved_key(key_path) {
            return Err(crate::error::manage_err("protected_registry", key_path).to_ipc());
        }
        return Ok(());
    }
    if !is_allowed_run_key(key_path) && !is_allowed_startup_approved_key(key_path) {
        return Err(crate::error::manage_err("protected_registry", key_path).to_ipc());
    }
    Ok(())
}

fn normalize_hklm(key_path: &str) -> String {
    let low = key_path.replace('/', "\\").to_uppercase();
    let low = low
        .strip_prefix("HKLM64\\")
        .map(|s| format!("HKLM\\{s}"))
        .unwrap_or(low);
    let low = low
        .strip_prefix("HKLM32\\")
        .map(|s| format!("HKLM\\{s}"))
        .unwrap_or(low);
    low
}

/// Final gate for registry key/value paths (same shape as Python `is_safe_to_delete_registry`).
pub fn is_safe_to_delete_registry(key_path: &str) -> Result<(), String> {
    if key_path.trim().is_empty() {
        return Err(crate::error::safety_err("empty registry path").to_ipc());
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
            return Err(crate::error::safety_err("uninstall root protected").to_ipc());
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
            return Err(crate::error::safety_err("app paths root protected").to_ipc());
        }
        if low.starts_with(&format!("{root}\\")) {
            return Ok(());
        }
    }

    // Services: direct child only; critical names blocked
    let services_root = "HKLM\\SYSTEM\\CURRENTCONTROLSET\\SERVICES";
    if low == services_root {
        return Err(crate::error::safety_err("services root protected").to_ipc());
    }
    if let Some(rest) = low.strip_prefix(&format!("{services_root}\\")) {
        if rest.is_empty() || rest.contains('\\') {
            return Err(crate::error::safety_err("only top-level service keys allowed").to_ipc());
        }
        if critical_service_names()
            .iter()
            .any(|n| rest == n.to_uppercase())
        {
            return Err(crate::error::safety_err("critical system service protected").to_ipc());
        }
        return Ok(());
    }

    // Task Cache tree: block Microsoft\, allow vendor tasks
    let task_root =
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS NT\\CURRENTVERSION\\SCHEDULE\\TASKCACHE\\TREE";
    if low == task_root {
        return Err(crate::error::safety_err("task tree root protected").to_ipc());
    }
    if let Some(rest) = low.strip_prefix(&format!("{task_root}\\")) {
        if rest == "MICROSOFT" || rest.starts_with("MICROSOFT\\") {
            return Err(crate::error::safety_err("system scheduled tasks protected").to_ipc());
        }
        return Ok(());
    }

    // Run/RunOnce values: key|ValueName only, never the key itself.
    // Empty ValueName (`…\Run|`) must NOT authorize deleting the whole Run key.
    let run_roots = [
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
    ];
    if let Some((key, val)) = key_path.rsplit_once('|') {
        if val.trim().is_empty() {
            return Err(crate::error::safety_err("registry value name must not be empty").to_ipc());
        }
        let key_low = normalize_hklm(key);
        for root in run_roots {
            if key_low == root {
                return Ok(());
            }
        }
        // Non-Run `key|Value` falls through to the same key-tree rules below
        // (Uninstall|Name, App Paths|Name, …).
    }
    for root in run_roots {
        if low == root {
            return Err(crate::error::safety_err("run root protected").to_ipc());
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
            return Err(crate::error::safety_err("protected registry prefix").to_ipc());
        }
    }

    Ok(())
}

/// Unified filesystem safety gate (scanner + executor).
/// Rejects protected prefixes, drive roots (`C:` / `C:\`), and shallow paths.
/// Prefixes come from environment (SystemRoot / ProgramData / ProgramFiles…) with `c:\` fallbacks.
pub fn is_safe_fs(p: &std::path::Path) -> bool {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    // S-R6-05: extended-length / 8.3 shapes must not slip past prefix matching.
    if is_abnormal_path_shape(&s) {
        return false;
    }
    let trimmed = s.trim_end_matches('\\');
    // Drive root: "c:" or "c:\"
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return false;
    }
    // S-4: reject path traversal segments before any prefix comparison.
    if trimmed.split('\\').any(|seg| seg == ".." || seg == ".") {
        return false;
    }
    // S-7B: exact Common Files roots are never deletable (vendor subpaths via policy association).
    if crate::shared::is_common_files_root(trimmed) {
        return false;
    }
    // Shallow: must be at least `drive:\dir\file-or-dir` (4 components on Windows).
    let comps = p.components().count();
    if comps < 4 {
        return false;
    }
    let protected = protected_fs_prefixes();
    !protected
        .iter()
        .any(|pref| s == *pref || s.starts_with(&format!("{pref}\\")))
}

fn env_dir_lower(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| {
        std::path::PathBuf::from(v)
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase()
            .trim_end_matches('\\')
            .to_string()
    })
}

/// Protected FS prefixes: hardcoded C-drive defaults + live environment roots (AR-03).
pub fn protected_fs_prefixes() -> Vec<String> {
    let mut out = vec![
        r"c:\windows".to_string(),
        r"c:\windows.old".to_string(),
        r"c:\programdata\microsoft".to_string(),
        r"c:\program files\windowsapps".to_string(),
        // S-7B: protect Microsoft Shared subtree; Common Files vendor subpaths need association (policy).
        r"c:\program files\common files\microsoft shared".to_string(),
        r"c:\program files (x86)\common files\microsoft shared".to_string(),
        r"c:\users\default".to_string(),
        r"c:\users\public\documents".to_string(),
        r"c:\users\public\desktop".to_string(),
        r"c:\users\public\downloads".to_string(),
    ];
    if let Some(root) = env_dir_lower("SystemRoot") {
        out.push(root.clone());
        out.push(format!("{root}.old"));
    }
    if let Some(pd) = env_dir_lower("ProgramData") {
        out.push(format!(r"{pd}\microsoft"));
    }
    if let Some(pf) = env_dir_lower("ProgramFiles") {
        out.push(format!(r"{pf}\windowsapps"));
        out.push(format!(r"{pf}\common files\microsoft shared"));
    }
    if let Some(pf86) = env_dir_lower("ProgramFiles(x86)") {
        out.push(format!(r"{pf86}\common files\microsoft shared"));
    }
    if let Some(sd) = std::env::var_os("SystemDrive") {
        let sd = sd.to_string_lossy().replace('/', "\\").to_lowercase();
        let sd = sd.trim_end_matches('\\').to_string();
        if sd.len() >= 2 {
            out.push(format!(r"{sd}\users\default"));
            out.push(format!(r"{sd}\users\public\documents"));
            out.push(format!(r"{sd}\users\public\desktop"));
            out.push(format!(r"{sd}\users\public\downloads"));
        }
    }
    out
}

/// True when the path uses an abnormal Windows shape that can bypass prefix/segment matching:
/// extended-length / device prefixes (`\\?\`, `\\.\`) or 8.3 short-name segments (`NAME~1`).
/// S-R6-05: these must never be treated as ordinary safe paths or non-library paths.
pub fn is_abnormal_path_shape(p: &str) -> bool {
    let s = p.replace('/', "\\");
    // Extended-length / device / UNC-device prefixes anywhere in the path.
    if s.contains("\\\\?\\") || s.contains("\\\\.\\") {
        return true;
    }
    // 8.3 short-name segment shape: `NAME~DIGITS` optionally followed by an extension.
    for seg in s.split('\\') {
        let low = seg.to_ascii_lowercase();
        let Some(tilde) = low.find('~') else {
            continue;
        };
        let rest = &low[tilde + 1..];
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            continue;
        }
        let after = &rest[digits..];
        if after.is_empty() || after.starts_with('.') {
            return true;
        }
    }
    false
}

/// Delete-grade gate on top of [`is_safe_fs`]: also rejects the user-data and sync-conflict red
/// lines. Scanner proposal filtering keeps using [`is_safe_fs`] so those items are still surfaced
/// (flagged `user_data`) for the "why we kept this" explanation instead of vanishing. Any code that
/// is about to *remove* something must call this instead of [`is_safe_fs`].
pub fn is_safe_fs_for_delete(p: &std::path::Path) -> bool {
    let s = p.to_string_lossy();
    is_safe_fs(p) && !is_user_data_path(&s) && !looks_like_sync_conflict(&s)
}

/// Red line: the user's library **roots** (Documents/Downloads/…) and sync-conflict trees.
/// Never delete these. Subfolders under a library (e.g. `Documents\<App>`, updater caches)
/// are **not** red-lined — they may be cleaned when associated. See [`is_user_library_path`].
pub fn is_user_data_path(p: &str) -> bool {
    // S-R6-05: `\\?\` / 8.3 shapes must not bypass the library-root red line.
    if is_abnormal_path_shape(p) {
        return true;
    }
    let low = p.replace('/', "\\").to_lowercase();
    let trimmed = low.trim_end_matches('\\');
    // Exact profile / public library roots (last path segment match).
    const SEGMENTS: &[&str] = &[
        "documents",
        "my documents",
        "desktop",
        "downloads",
        "pictures",
        "videos",
        "music",
        "onedrive",
    ];
    let last = trimmed.rsplit('\\').next().unwrap_or("");
    if !SEGMENTS.contains(&last) {
        return false;
    }
    // Require a Users-style prefix so non-profile ...\Documents trees stay
    // governed by association/other gates rather than a global name ban.
    trimmed.contains(r"\users\") || trimmed.contains(r"\user\")
}

/// Path sits **under** a user library folder but is not the root itself. Cleanable when
/// associated; never default-selected (may hold saves / personal files).
pub fn is_user_library_path(p: &str) -> bool {
    if is_user_data_path(p) {
        return false;
    }
    let low = p.replace('/', "\\").to_lowercase();
    let markers = [
        r"\my documents\",
        r"\documents\",
        r"\desktop\",
        r"\downloads\",
        r"\pictures\",
        r"\videos\",
        r"\music\",
        r"\onedrive\",
    ];
    (low.contains(r"\users\") || low.contains(r"\user\")) && markers.iter().any(|m| low.contains(m))
}

/// Sync-conflict style folders often hold real user files.
/// S-R6-11: segment-aware — a bare `conflict` substring (`MyConflictApp`) must not match.
pub fn looks_like_sync_conflict(p: &str) -> bool {
    let low = p.replace('/', "\\").to_lowercase();
    if low.contains("同步冲突") {
        return true;
    }
    for seg in low.split('\\') {
        let s = seg.trim();
        if s.is_empty() {
            continue;
        }
        // Exact conflict folder names.
        if s == "conflict" || s == "conflicts" || s == "sync_conflict" || s == "sync conflict" {
            return true;
        }
        // Known sync-client conflict naming.
        if s.contains("conflicted copy")
            || s.starts_with("sync_conflict")
            || s.starts_with("sync conflict")
        {
            return true;
        }
        // `name - conflict` / `name (conflict…)` / `name_conflict` / `name.conflict`
        if s.ends_with(" - conflict")
            || s.ends_with("_conflict")
            || s.ends_with(".conflict")
            || s.contains(" (conflict")
        {
            return true;
        }
    }
    false
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
    fn fs_env_prefixes_include_system_root() {
        let prefixes = protected_fs_prefixes();
        assert!(prefixes.iter().any(|p| p.contains("windows")));
        // SystemRoot on this machine (usually C:\Windows) must be present when env is set.
        if let Ok(sr) = std::env::var("SystemRoot") {
            let low = sr
                .replace('/', "\\")
                .to_lowercase()
                .trim_end_matches('\\')
                .to_string();
            assert!(
                prefixes.iter().any(|p| p == &low
                    || p.starts_with(&format!("{low}\\"))
                    || p == &format!("{low}.old")
                    || low.starts_with(&format!("{p}\\"))
                    || p.starts_with(&low)),
                "SystemRoot {low} not reflected in {prefixes:?}"
            );
            // Deep path under SystemRoot is rejected.
            let deep_s = format!(r"{sr}\System32\drivers\etc");
            let deep = Path::new(&deep_s);
            if deep.components().count() >= 4 {
                assert!(
                    !is_safe_fs(deep),
                    "SystemRoot subtree must be protected: {deep:?}"
                );
            }
        }
    }

    #[test]
    fn fs_rejects_path_traversal() {
        assert!(!is_safe_fs(Path::new(
            r"C:\Program Files\DemoApp\..\..\..\Windows\System32\x"
        )));
        assert!(!is_safe_fs(Path::new(r"C:\Users\a\Documents\..\Windows")));
    }

    #[test]
    fn fs_allows_normal_install() {
        assert!(is_safe_fs(Path::new(r"C:\Program Files\MyApp")));
        assert!(is_safe_fs(Path::new(r"D:\Games\SomeGame")));
    }

    #[test]
    fn user_data_exact_profile_roots_flagged() {
        // Red line = library roots only (last segment + Users prefix).
        for p in [
            r"C:\Users\a\Documents",
            r"C:\Users\a\Documents\",
            r"c:\users\a\desktop",
            r"C:\Users\a\Downloads",
            r"C:\Users\Public\Documents",
            r"C:\Users\Public\Downloads",
            r"D:\Users\b\Pictures",
        ] {
            assert!(is_user_data_path(p), "expected user_data for {p}");
        }
        // Library subpaths are cleanable when associated (not the red line).
        assert!(!is_user_data_path(
            r"C:\Users\a\Documents\App\Config\file.txt"
        ));
        assert!(is_user_library_path(
            r"C:\Users\a\Documents\App\Config\file.txt"
        ));
        assert!(!is_user_data_path(r"C:\Users\a\Downloads\x.msi"));
        // AppData / updater caches are ordinary leftovers.
        assert!(!is_user_data_path(
            r"C:\Users\a\AppData\Local\Acme\updater\pkg.exe"
        ));
        assert!(!is_user_library_path(
            r"C:\Users\a\AppData\Local\Acme\updater\pkg.exe"
        ));
        assert!(!is_user_data_path(r"C:\Program Files\MyApp"));
    }

    #[test]
    fn fs_rejects_common_files_and_public_roots() {
        assert!(!is_safe_fs(Path::new(r"C:\Program Files\Common Files")));
        assert!(!is_safe_fs(Path::new(
            r"C:\Program Files (x86)\Common Files"
        )));
        assert!(!is_safe_fs(Path::new(r"C:\Users\Public\Documents")));
        // S-7B: Microsoft Shared subtree still protected; bare vendor CF dir is not FS-protected.
        assert!(!is_safe_fs(Path::new(
            r"C:\Program Files\Common Files\Microsoft Shared\X"
        )));
        assert!(is_safe_fs(Path::new(
            r"C:\Program Files\Common Files\Acme\Component"
        )));
    }

    #[test]
    fn delete_gate_adds_user_data_red_lines_to_shape_gate() {
        // Library roots and sync-conflict trees: shape gate still surfaces them for
        // the kept-list explanation, but the delete-grade gate must reject them.
        for p in [
            r"C:\Users\a\Documents",
            r"C:\Users\a\Documents\坚果云同步冲突\x",
        ] {
            assert!(
                is_safe_fs(Path::new(p)),
                "shape gate should still propose {p} for the kept-list explanation"
            );
            assert!(
                !is_safe_fs_for_delete(Path::new(p)),
                "delete gate must reject {p}"
            );
        }
        // Library subpaths and updater caches are cleanable when associated.
        assert!(is_safe_fs_for_delete(Path::new(
            r"C:\Users\a\Documents\App\Config"
        )));
        assert!(is_safe_fs_for_delete(Path::new(
            r"C:\Users\a\Downloads\setup.msi"
        )));
        assert!(is_safe_fs_for_delete(Path::new(
            r"C:\Users\a\AppData\Local\Acme\updater\pkg.exe"
        )));
        assert!(is_safe_fs_for_delete(Path::new(r"C:\Program Files\MyApp")));
        assert!(is_safe_fs_for_delete(Path::new(
            r"C:\Users\a\AppData\Roaming\MyApp"
        )));
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
        assert!(
            is_safe_to_delete_registry(r"HKLM64\SYSTEM\CurrentControlSet\Services\Winmgmt")
                .is_err()
        );
    }

    #[test]
    fn critical_service_names_include_security_stack() {
        assert!(is_critical_service("WinDefend"));
        assert!(is_critical_service("windefend"));
        assert!(is_critical_service("DcomLaunch"));
        assert!(is_critical_service("Appinfo"));
        assert!(is_critical_service("Power"));
        assert!(!is_critical_service("DemoVendorHelper"));
        assert!(
            is_safe_to_delete_registry(r"HKLM64\SYSTEM\CurrentControlSet\Services\WinDefend")
                .is_err()
        );
    }

    #[test]
    fn startup_approved_key_whitelist() {
        assert!(is_allowed_startup_approved_key(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\PackagedStartup"
        ));
        assert!(is_allowed_startup_approved_key(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
        ));
        assert!(!is_allowed_startup_approved_key(
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
        ));
        assert!(!is_allowed_startup_approved_key(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\evil.exe"
        ));
    }

    #[test]
    fn manage_policy_helpers() {
        assert!(allow_manage_service_write("WinDefend").is_err());
        assert!(allow_manage_service_write("DemoVendorHelper").is_ok());
        // S-R4-09: Microsoft-prefixed services are write-protected even if not critical.
        assert!(allow_manage_service_write("MicrosoftEdgeUpdate").is_err());
        assert!(allow_manage_service_write("microsoft some svc").is_err());
        assert!(allow_manage_reg_write(r"HKLM\SOFTWARE\Evil\Config", false).is_err());
        assert!(allow_manage_reg_write(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\PackagedStartup",
            true
        )
        .is_ok());
        assert!(allow_manage_reg_write(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            false
        )
        .is_ok());
    }

    #[test]
    fn run_key_whitelist() {
        assert!(is_allowed_run_key(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
        ));
        assert!(is_allowed_run_key(
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
        ));
        assert!(!is_allowed_run_key(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders"
        ));
        assert!(!is_allowed_run_key(r"HKLM\SOFTWARE\EvilCorp\Config"));
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
        // S-R6-02: empty value name must not authorize deleting the whole key.
        assert!(is_safe_to_delete_registry(&format!("{root}|")).is_err());
        assert!(is_safe_to_delete_registry(&format!("{root}| ")).is_err());
        let run_root = r"HKCU\SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\RUN";
        assert!(is_safe_to_delete_registry(&format!("{run_root}|Demo")).is_ok());
        assert!(is_safe_to_delete_registry(&format!("{run_root}|")).is_err());
    }

    #[test]
    fn hklm32_normalize_matches_64() {
        assert!(is_safe_to_delete_registry(
            r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{ABC}"
        )
        .is_ok());
    }

    #[test]
    fn user_data_paths_flagged() {
        assert!(super::is_user_data_path(r"C:\Users\a\Documents"));
        assert!(super::is_user_data_path(r"C:\Users\a\Downloads"));
        assert!(!super::is_user_data_path(
            r"C:\Users\a\Documents\App\file.txt"
        ));
        assert!(super::is_user_library_path(
            r"C:\Users\a\Documents\App\file.txt"
        ));
        assert!(!super::is_user_data_path(r"C:\Users\a\Downloads\x.msi"));
        assert!(super::is_user_library_path(r"C:\Users\a\Downloads\x.msi"));
        assert!(!super::is_user_data_path(r"C:\Program Files\App\bin.exe"));
    }

    #[test]
    fn sync_conflict_flagged() {
        assert!(super::looks_like_sync_conflict(
            r"C:\Users\a\Documents\坚果云同步冲突"
        ));
        assert!(!super::looks_like_sync_conflict(r"C:\ProgramData\App"));
        // S-R6-11: bare `conflict` substring must not match ordinary product names.
        assert!(!super::looks_like_sync_conflict(
            r"C:\Program Files\MyConflictApp"
        ));
        assert!(!super::looks_like_sync_conflict(
            r"C:\Program Files\ConflictResolution\bin"
        ));
        assert!(super::looks_like_sync_conflict(
            r"C:\Users\a\Documents\conflict\save.dat"
        ));
        assert!(super::looks_like_sync_conflict(
            r"C:\Users\a\Documents\report (conflicted copy 2024).docx"
        ));
    }

    #[test]
    fn abnormal_path_shape_rejected() {
        // S-R6-05: extended-length / 8.3 shapes cannot enter safe / non-user-data results.
        assert!(super::is_abnormal_path_shape(r"\\?\C:\Program Files\App"));
        assert!(super::is_abnormal_path_shape(r"\\.\C:\Program Files\App"));
        assert!(super::is_abnormal_path_shape(r"C:\PROGRA~1\App"));
        assert!(super::is_abnormal_path_shape(r"C:\Users\Aaron\DOCUME~1"));
        assert!(super::is_abnormal_path_shape(
            r"C:\Users\Aaron\DOCUME~1\file.txt"
        ));
        assert!(!super::is_abnormal_path_shape(r"C:\Program Files\App"));
        assert!(!super::is_abnormal_path_shape(r"C:\Users\Aaron\Documents"));
        // `~` not followed by digits is not an 8.3 shape.
        assert!(!super::is_abnormal_path_shape(r"C:\foo~bar\baz"));
        assert!(!super::is_safe_fs(Path::new(
            r"\\?\C:\Windows\System32\evil"
        )));
        assert!(!super::is_safe_fs(Path::new(r"C:\PROGRA~1\App")));
        assert!(super::is_user_data_path(r"C:\Users\Aaron\DOCUME~1"));
        assert!(super::is_user_data_path(r"\\?\C:\Users\Aaron\Documents"));
    }
}
