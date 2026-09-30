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
        // Manage-path expansions (): core OS / security / session services.
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

/// Unified write-policy for manage IPC ( light): critical service + startup-approved keys.
pub fn allow_manage_service_write(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() || n.contains('\\') || n.contains('/') {
        return Err(crate::error::manage_err("bad_name", "bad service name").to_ipc());
    }
    if is_critical_service(n) {
        return Err(crate::error::manage_err("protected", n).to_ipc());
    }
    // / Microsoft-family services (MSMQ, MsSqlServer, Microsoft*, MS *).
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

/// intrinsic write-target allowlist for registry value primitives.
/// Covers every legitimate writer in the codebase — Uninstall roots (product
/// metadata), Run/RunOnce + StartupApproved (startup management), per-service
/// keys (start type) and Remova's own context-menu key — so a value write can
/// never land outside these shapes even if a future caller forgets its gate.
pub fn allow_reg_value_write(key_path: &str) -> Result<(), String> {
    let low = normalize_hklm(key_path);
    const UNINSTALL_ROOTS: &[&str] = &[
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\UNINSTALL",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\UNINSTALL",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\UNINSTALL",
        "HKCU\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\UNINSTALL",
    ];
    const SERVICES_PREFIX: &str = "HKLM\\SYSTEM\\CURRENTCONTROLSET\\SERVICES\\";
    // require a path separator after the base key so sibling keys
    // like `RemovaDeepUninstallX` are not treated as subkeys.
    const REMOVA_MENU_BASE: &str = "HKCU\\SOFTWARE\\CLASSES\\*\\SHELL\\REMOVADEEPUNINSTALL";
    if UNINSTALL_ROOTS
        .iter()
        .any(|r| low == *r || low.starts_with(&format!("{r}\\")))
        || is_allowed_run_key(key_path)
        || is_allowed_startup_approved_key(key_path)
        || low == REMOVA_MENU_BASE
        || low.starts_with(&format!("{REMOVA_MENU_BASE}\\"))
    {
        return Ok(());
    }
    // Services: intrinsic gate must refuse critical / Microsoft-family names
    // even if a future caller skips `allow_manage_service_write`.
    if low.starts_with(SERVICES_PREFIX) && low.len() > SERVICES_PREFIX.len() {
        let leaf = registry_key_part(key_path)
            .replace('/', "\\")
            .rsplit('\\')
            .next()
            .unwrap_or("")
            .to_string();
        allow_manage_service_write(&leaf)?;
        return Ok(());
    }
    Err(crate::error::safety_err(format!("registry write outside allowlist: {key_path}")).to_ipc())
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

/// Registry paths may carry a trailing `|ValueName`. Gates that judge the key
/// tree (protected prefixes, service names) must compare the key part only —
/// otherwise `HKLM\SYSTEM|X` slips past `HKLM\SYSTEM` / `HKLM\SYSTEM\`.
fn registry_key_part(key_path: &str) -> &str {
    match key_path.rsplit_once('|') {
        Some((key, _)) => key,
        None => key_path,
    }
}

/// Final gate for registry key/value paths (same shape as Python `is_safe_to_delete_registry`).
pub fn is_safe_to_delete_registry(key_path: &str) -> Result<(), String> {
    if key_path.trim().is_empty() {
        return Err(crate::error::safety_err("empty registry path").to_ipc());
    }
    // Value-shaped `key|Value`: empty name never authorizes deleting the key.
    // Tree rules below always judge the key part only (see `registry_key_part`).
    let value_name = match key_path.rsplit_once('|') {
        Some((_, val)) => {
            if val.trim().is_empty() {
                return Err(
                    crate::error::safety_err("registry value name must not be empty").to_ipc(),
                );
            }
            Some(val)
        }
        None => None,
    };
    let low = normalize_hklm(registry_key_part(key_path));

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
        // `rest` is already the key part (value name stripped above).
        if rest.contains('\\') {
            return Err(crate::error::safety_err("only top-level service keys allowed").to_ipc());
        }
        if rest.is_empty() {
            // `Services\|Start` — no service named; never authorize.
            return Err(crate::error::safety_err("service key name must not be empty").to_ipc());
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
    // Empty ValueName (`…\Run|`) must NOT authorize deleting the whole Run key
    // (handled above). A value on a Run root is legal; the bare root is not.
    let run_roots = [
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKLM\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKLM\\SOFTWARE\\WOW6432NODE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN",
        "HKCU\\SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE",
    ];
    for root in run_roots {
        if low == root {
            if value_name.is_some() {
                return Ok(());
            }
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
/// Non-UTF-8 paths fail closed (no lossy conversion into the comparison).
pub fn is_safe_fs(p: &std::path::Path) -> bool {
    let Some(s) = path_utf8_lower(p) else {
        return false;
    };
    // extended-length / 8.3 shapes must not slip past prefix matching.
    if is_abnormal_path_shape(&s) {
        return false;
    }
    let trimmed = s.trim_end_matches('\\');
    // Drive root: "c:" or "c:\"
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return false;
    }
    // reject path traversal segments before any prefix comparison.
    if trimmed.split('\\').any(|seg| seg == ".." || seg == ".") {
        return false;
    }
    // absolute drive path only — no relative, no UNC/device shares.
    if !p.has_root() || trimmed.starts_with("\\\\") {
        return false;
    }
    //exact Common Files roots are never deletable (vendor subpaths via policy association).
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

/// Lowercased backslash form of a path, or `None` when not valid UTF-8 (fail-closed).
fn path_utf8_lower(p: &std::path::Path) -> Option<String> {
    // Win32 strips trailing dots/spaces per segment — normalize before compare.
    // Keep `.` / `..` intact so traversal rejection still works.
    let s = p.to_str()?.replace('/', "\\").to_lowercase();
    Some(
        s.split('\\')
            .map(|seg| {
                if seg == "." || seg == ".." {
                    seg
                } else {
                    seg.trim_end_matches(['.', ' '])
                }
            })
            .collect::<Vec<_>>()
            .join("\\"),
    )
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

/// Protected FS prefixes: hardcoded C-drive defaults + live environment roots ().
pub fn protected_fs_prefixes() -> Vec<String> {
    let mut out = vec![
        r"c:\windows".to_string(),
        r"c:\windows.old".to_string(),
        r"c:\programdata\microsoft".to_string(),
        r"c:\program files\windowsapps".to_string(),
        //protect Microsoft Shared subtree; Common Files vendor subpaths need association (policy).
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

/// 8.3 short-name segment (`NAME~DIGITS` / `NAME~DIGITS.EXT`), 1–8 alnum + 1–8 digits ().
fn is_83_short_segment(seg: &str) -> bool {
    let low = seg.to_ascii_lowercase();
    let base = low.split('.').next().unwrap_or(low.as_str());
    let Some((name, num)) = base.split_once('~') else {
        return false;
    };
    if name.is_empty() || name.len() > 8 || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    !num.is_empty() && num.len() <= 8 && num.chars().all(|c| c.is_ascii_digit())
}

/// True when the path uses an abnormal Windows shape that can bypass prefix/segment matching:
/// extended-length / device prefixes (`\\?\`, `\\.\`) or any non-profile 8.3 short segment
/// (`WINDOWS~1`, `PROGRA~3`, `COMMON~1`, …). Profile short names (`Users\RUNNER~1\…`) stay legal.
pub fn is_abnormal_path_shape(p: &str) -> bool {
    let s = p.replace('/', "\\");
    if s.contains("\\\\?\\") || s.contains("\\\\.\\") {
        return true;
    }
    let segs: Vec<&str> = s.split('\\').collect();
    for (i, seg) in segs.iter().enumerate() {
        if !is_83_short_segment(seg) {
            continue;
        }
        // Users\<short>\… is a legitimate profile home (CI `RUNNER~1`).
        if i > 0 && segs[i - 1].eq_ignore_ascii_case("users") {
            continue;
        }
        return true;
    }
    false
}

/// Delete-grade gate on top of [`is_safe_fs`]: also rejects the user-data and sync-conflict red
/// lines. Scanner proposal filtering keeps using [`is_safe_fs`] so those items are still surfaced
/// (flagged `user_data`) for the "why we kept this" explanation instead of vanishing. Any code that
/// is about to *remove* something must call this instead of [`is_safe_fs`].
pub fn is_safe_fs_for_delete(p: &std::path::Path) -> bool {
    let Some(s) = p.to_str() else {
        return false;
    };
    is_safe_fs(p) && !is_user_data_path(s) && !looks_like_sync_conflict(s)
}

/// Restore target gate: write-back must not hit library roots, sync-conflict trees, or
/// protected system prefixes. Unlike delete, 8.3 profile names (`Users\RUNNER~1\…`) are
/// legitimate restore destinations ( + CI temp homes).
pub fn is_safe_restore_target(p: &std::path::Path) -> bool {
    let Some(s) = p.to_str().map(|s| s.to_string()) else {
        // Non-UTF-8 destinations never restore (no lossy compare).
        return false;
    };
    if s.trim().is_empty() {
        return false;
    }
    if looks_like_sync_conflict(&s) || is_user_data_path(&s) {
        return false;
    }
    // Same fail-closed shape gate as delete (8.3 short names / extended prefixes).
    // Profile short homes (`Users\RUNNER~1\…`) remain legal via `is_abnormal_path_shape`.
    if is_abnormal_path_shape(&s) {
        return false;
    }
    let low = s.replace('/', "\\").to_lowercase();
    // Reject traversal segments on the raw path first — trailing-dot strip
    // below would turn `..` into an empty segment and hide it.
    if low.split('\\').any(|seg| seg == ".." || seg == ".") {
        return false;
    }
    // Win32 strips per-segment trailing dots/spaces when resolving paths —
    // compare on the normalized form so `C:\Windows.` cannot slip past the
    // protected roots (same normalization the delete side uses).
    let normalized: String = low
        .split('\\')
        .map(|seg| seg.trim_end_matches(['.', ' ']))
        .collect::<Vec<_>>()
        .join("\\");
    let trimmed = normalized.trim_end_matches('\\');
    if trimmed.split('\\').any(|seg| seg == ".." || seg == ".") {
        return false;
    }
    // absolute drive paths only — no relative targets, no UNC,
    // not even drive-relative backslash-led forms (`\srv`, same rule as
    // the delete gate enforces via `is_safe_fs`).
    if !p.has_root() || trimmed.starts_with('\\') {
        return false;
    }
    // Reuse delete-adjacent system roots without the 8.3 ban.
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return false;
    }
    // match system roots on the normalized path (drive + suffix).
    // A bare `\windows` prefix never matches `c:\windows\...` — strip the drive first.
    let after_drive = trimmed
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let protected_suffixes = [
        r"\windows",
        r"\windows.old",
        r"\program files",
        r"\program files (x86)",
        r"\programdata\microsoft",
    ];
    for pref in protected_suffixes {
        if after_drive == pref || after_drive.starts_with(&format!("{pref}\\")) {
            return false;
        }
    }
    // Env-specific protected roots (SystemRoot on D:, ProgramData, …).
    for pref in protected_fs_prefixes() {
        if trimmed == pref || trimmed.starts_with(&format!("{pref}\\")) {
            return false;
        }
    }
    // S-R4: never write into Startup (persistence). Library *subpaths* stay restorable —
    // path_map is the server-side record of where the file came from (roots already blocked).
    if trimmed.contains("\\start menu\\programs\\startup")
        || trimmed.contains("\\microsoft\\windows\\start menu\\programs\\startup")
    {
        return false;
    }
    true
}

/// Lowercased, per-segment Win32-normalized form of a path string: trailing
/// dots/spaces are stripped from every segment (the OS does this when
/// resolving) so `C:\Users\a\Documents.` cannot slip past a segment-name
/// red line. `.` / `..` segments are kept for traversal checks.
fn normalize_path_segments(p: &str) -> String {
    let s = p.replace('/', "\\").to_lowercase();
    let trimmed = s.trim_end_matches('\\');
    trimmed
        .split('\\')
        .map(|seg| {
            if seg == "." || seg == ".." {
                seg
            } else {
                seg.trim_end_matches(['.', ' '])
            }
        })
        .collect::<Vec<_>>()
        .join("\\")
}

/// Red line: the user's library **roots** (Documents/Downloads/…) and sync-conflict trees.
/// Never delete these. Subfolders under a library (e.g. `Documents\<App>`, updater caches)
/// are **not** red-lined — they may be cleaned when associated. See [`is_user_library_path`].
pub fn is_user_data_path(p: &str) -> bool {
    // Library-root red line is based on segment names. Abnormal `\\?\` / 8.3 shapes are
    // rejected by `is_safe_fs` for deletes; they must not mark every `Users\RUNNER~1\…`
    // as user_data or restores into CI/profile temp homes become impossible.
    // Segments are Win32-normalized (trailing dots/spaces) before matching.
    let trimmed = normalize_path_segments(p);
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
    let low = normalize_path_segments(p);
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
/// segment-aware — a bare `conflict` substring (`MyConflictApp`) must not match.
pub fn looks_like_sync_conflict(p: &str) -> bool {
    let low = normalize_path_segments(p);
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

    /// registry value primitives may only write inside the allowlist.
    #[test]
    fn reg_value_write_allowlist() {
        // Allowed shapes.
        assert!(allow_reg_value_write(
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\App"
        )
        .is_ok());
        assert!(
            allow_reg_value_write(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run").is_ok()
        );
        assert!(allow_reg_value_write(
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder"
        )
        .is_ok());
        assert!(
            allow_reg_value_write(r"HKLM64\SYSTEM\CurrentControlSet\Services\VendorSvc").is_ok()
        );
        assert!(allow_reg_value_write(
            r"HKCU\Software\Classes\*\shell\RemovaDeepUninstall\command"
        )
        .is_ok());
        assert!(
            allow_reg_value_write(r"HKCU\Software\Classes\*\shell\RemovaDeepUninstall").is_ok()
        );
        // sibling keys must not ride the Remova menu prefix.
        assert!(
            allow_reg_value_write(r"HKCU\Software\Classes\*\shell\RemovaDeepUninstallX").is_err()
        );
        assert!(allow_reg_value_write(
            r"HKCU\Software\Classes\*\shell\RemovaDeepUninstallX\command"
        )
        .is_err());
        // Everything else is refused — even plausible-but-unlisted keys.
        assert!(allow_reg_value_write(r"HKCU\Software\Vendor\Config").is_err());
        assert!(allow_reg_value_write(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\evil.exe"
        )
        .is_err());
        assert!(allow_reg_value_write(r"HKCU\Environment").is_err());
        assert!(allow_reg_value_write(r"HKLM64\SYSTEM\CurrentControlSet\Services").is_err());
        assert!(allow_reg_value_write(r"HKCU\Software\Classes\*\shell\OtherTool").is_err());
        assert!(allow_reg_value_write("").is_err());
    }

    /// Protected roots accept value deletes only when the key part is judged.
    /// `HKLM\SYSTEM|X` must fail the prefix gate the same way `HKLM\SYSTEM` does.
    #[test]
    fn reg_delete_gate_blocks_protected_root_value_shapes() {
        for path in [
            r"HKLM\SYSTEM|X",
            r"HKLM\SYSTEM|MachineGuid",
            r"HKLM\SECURITY|X",
            r"HKLM\SAM|X",
            r"HKLM\HARDWARE|X",
            r"HKLM\SOFTWARE\Microsoft\Cryptography|MachineGuid",
            r"HKLM\SOFTWARE\Microsoft\Windows|X",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion|X",
            r"HKCU\SOFTWARE\Microsoft\Windows|X",
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion|X",
        ] {
            assert!(
                is_safe_to_delete_registry(path).is_err(),
                "protected root value must be refused: {path}"
            );
        }
        // Uninstall product values remain legal (existing contract).
        assert!(is_safe_to_delete_registry(
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{ABC}|QuietUninstallString"
        )
        .is_ok());
    }

    /// Intrinsic write allowlist must refuse critical services even when a
    /// future caller skips `allow_manage_service_write`.
    #[test]
    fn reg_value_write_blocks_critical_services() {
        for key in [
            r"HKLM64\SYSTEM\CurrentControlSet\Services\WinDefend",
            r"HKLM\SYSTEM\CurrentControlSet\Services\Winmgmt",
            r"HKLM\SYSTEM\CurrentControlSet\Services\DcomLaunch",
            r"HKLM\SYSTEM\CurrentControlSet\Services\TrustedInstaller",
            r"HKLM\SYSTEM\CurrentControlSet\Services\MicrosoftEdgeUpdate",
        ] {
            assert!(
                allow_reg_value_write(key).is_err(),
                "critical/Microsoft service write must be refused: {key}"
            );
        }
        // Non-critical vendor service still allowed.
        assert!(
            allow_reg_value_write(r"HKLM64\SYSTEM\CurrentControlSet\Services\VendorSvc").is_ok()
        );
    }

    /// Value-shaped `Services|value` paths must not bypass the critical list.
    #[test]
    fn reg_delete_gate_blocks_service_value_shape() {
        let err =
            is_safe_to_delete_registry(r"HKLM\SYSTEM\CurrentControlSet\Services\WinDefend|Start")
                .unwrap_err();
        assert!(err.contains("critical system service protected"), "{err}");
        assert!(is_safe_to_delete_registry(
            r"HKLM\SYSTEM\CurrentControlSet\Services\VendorSvc|Start"
        )
        .is_ok());
        // Empty service name in the value shape must not pass either.
        let empty_name =
            is_safe_to_delete_registry(r"HKLM\SYSTEM\CurrentControlSet\Services\|Start")
                .unwrap_err();
        assert!(
            empty_name.contains("service key name must not be empty"),
            "{empty_name}"
        );
    }

    /// Restore-target gate must judge the Win32-normalized path (trailing
    /// dots/spaces per segment are stripped by the OS when resolving).
    #[test]
    fn restore_target_blocks_trailing_dot_shapes() {
        assert!(!is_safe_restore_target(Path::new(r"C:\Windows.\evil.dll")));
        assert!(!is_safe_restore_target(Path::new(
            r"C:\Program Files.\evil.exe"
        )));
        assert!(!is_safe_restore_target(Path::new(r"C:\Windows. \evil.dll")));
        // Ordinary dotted names keep restoring fine.
        assert!(is_safe_restore_target(Path::new(
            r"C:\Users\testuser\AppData\Local\App\v1.2\file.dll"
        )));
    }

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
        //Microsoft Shared subtree still protected; bare vendor CF dir is not FS-protected.
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
        // Microsoft-prefixed services are write-protected even if not critical.
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
        // empty value name must not authorize deleting the whole key.
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

    /// Win32 strips per-segment trailing dots/spaces when resolving —
    /// the red-line comparisons must judge the normalized segments, not raw ones.
    #[test]
    fn user_data_red_line_survives_trailing_dot_segments() {
        assert!(super::is_user_data_path(r"C:\Users\a\Documents."));
        assert!(super::is_user_data_path(r"C:\Users\a\Documents. "));
        assert!(super::is_user_data_path(r"C:\Users.\a\Documents."));
        assert!(super::is_user_library_path(
            r"C:\Users\a\Documents.\App\saves.db"
        ));
        // Red line still must not fire outside a Users-style prefix.
        assert!(!super::is_user_data_path(
            r"C:\Program Files\App\Documents."
        ));
        assert!(super::looks_like_sync_conflict(
            r"C:\Users\a\Documents\conflict."
        ));
        assert!(super::looks_like_sync_conflict(
            r"C:\Users\a\Documents\MyApp - conflict."
        ));
        assert!(super::looks_like_sync_conflict(
            r"C:\Users\a\Documents\report (conflicted copy 2024). docx."
        ));
    }

    #[test]
    fn sync_conflict_flagged() {
        assert!(super::looks_like_sync_conflict(
            r"C:\Users\a\Documents\坚果云同步冲突"
        ));
        assert!(!super::looks_like_sync_conflict(r"C:\ProgramData\App"));
        // bare `conflict` substring must not match ordinary product names.
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
        // extended-length / 8.3 shapes cannot enter safe / non-user-data results.
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
        // Non-profile NAME~digits segments always abnormal (windows~1 / progra~3 / common~1).
        assert!(super::is_abnormal_path_shape(
            r"C:\WINDOWS~1\System32\evil.dll"
        ));
        assert!(super::is_abnormal_path_shape(r"C:\PROGRA~3\Vendor\App"));
        assert!(super::is_abnormal_path_shape(r"C:\PROGRA~1\Common Files\x"));
        assert!(super::is_abnormal_path_shape(
            r"C:\COMMON~1\Microsoft Shared\x"
        ));
        assert!(super::is_abnormal_path_shape(
            r"C:\Users\Aaron\DOCUME~1\App"
        ));
        // Profile short home under Users stays legal.
        assert!(!super::is_abnormal_path_shape(
            r"C:\Users\RUNNER~1\Documents\App"
        ));
        assert!(!super::is_abnormal_path_shape(
            r"C:\Users\RUNNER~1\AppData\Local\Acme"
        ));
        assert!(!super::is_safe_fs(Path::new(
            r"C:\WINDOWS~1\System32\evil.dll"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\WINDOWS~1\System32\evil.dll"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\PROGRA~1\App\bin.exe"
        )));
        assert!(super::is_safe_restore_target(Path::new(
            r"C:\Users\RUNNER~1\Documents\App\file.txt"
        )));
        assert!(!super::is_safe_fs(Path::new(
            r"\\?\C:\Windows\System32\evil"
        )));
        assert!(!super::is_safe_fs(Path::new(r"C:\PROGRA~1\App")));
        // 8.3 library short-names are blocked by is_safe_fs, not the user_data segment list.
        assert!(!super::is_user_data_path(r"C:\Users\Aaron\DOCUME~1"));
        assert!(!super::is_safe_fs(Path::new(r"C:\Users\Aaron\DOCUME~1")));
        assert!(super::is_safe_fs(Path::new(r"C:\Users\Aaron\Documents")));
        assert!(super::is_user_data_path(r"\\?\C:\Users\Aaron\Documents"));
    }

    ///  adversarial: tampered path_map targeting system prefixes must be refused.
    #[test]
    fn adversarial_restore_target_rejects_system_prefixes() {
        use std::path::Path;
        // Drive-prefixed system roots (the old `\\windows` vs `c:\\windows\\...` miss).
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Windows\System32\evil.dll"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Windows\evil.dll"
        )));
        assert!(!super::is_safe_restore_target(Path::new(r"c:\windows")));
        assert!(!super::is_safe_restore_target(Path::new(
            r"D:\Windows\evil.dll"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Program Files\App\bin.exe"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Program Files (x86)\App\bin.exe"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\ProgramData\Microsoft\evil.dll"
        )));
        // Case / slash style must not bypass.
        assert!(!super::is_safe_restore_target(Path::new(
            r"c:/windows/system32/evil.dll"
        )));
        // Ordinary app paths remain legitimate restore destinations.
        assert!(super::is_safe_restore_target(Path::new(
            r"C:\Vendor\Tool\file.txt"
        )));
        assert!(super::is_safe_restore_target(Path::new(
            r"D:\Games\Save\slot.dat"
        )));
    }

    /// R1: library *subpaths* are restorable (path_map originals); roots / Startup / sync-conflict stay blocked.
    #[test]
    fn restore_allows_library_subpaths_not_roots() {
        // Backup of `Documents\<App>` must be able to write back.
        assert!(super::is_safe_restore_target(Path::new(
            r"C:\Users\a\Documents\App\Config\file.txt"
        )));
        assert!(super::is_safe_restore_target(Path::new(
            r"C:\Users\a\Documents\MyGame\saves\slot.dat"
        )));
        assert!(super::is_safe_restore_target(Path::new(
            r"C:\Users\a\Downloads\App\pkg.dat"
        )));
        assert!(super::is_safe_restore_target(Path::new(
            r"C:\Users\a\AppData\Local\Acme\data.bin"
        )));
        // Library roots remain red-lined.
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Users\a\Documents"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Users\a\Downloads"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Users\a\Documents\"
        )));
        // Startup persistence stays blocked.
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\evil.lnk"
        )));
        // Sync-conflict trees stay blocked.
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Users\a\Documents\conflict\save.dat"
        )));
    }

    /// N-risk: non-UTF-8 paths fail closed in delete/restore gates (no lossy compare).
    #[test]
    fn non_utf8_paths_fail_closed() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        // Invalid UTF-16 unit → not valid UTF-8 on the Rust side.
        let wide: Vec<u16> = vec![
            'C' as u16,
            ':' as u16,
            '\\' as u16,
            0xD800,
            '\\' as u16,
            'x' as u16,
        ];
        let os = OsString::from_wide(&wide);
        let p = std::path::Path::new(&os);
        assert!(!super::is_safe_fs(p));
        assert!(!super::is_safe_fs_for_delete(p));
        assert!(!super::is_safe_restore_target(p));
    }

    /// Library-subpath restore requires an out-of-session seal (see path_seal).
    #[test]
    fn restore_target_shape_allows_library_subpath() {
        // Shape gate alone permits it; restore.rs enforces the seal separately.
        assert!(super::is_safe_restore_target(Path::new(
            r"C:\Users\a\Documents\App\file.txt"
        )));
        assert!(super::is_user_library_path(
            r"C:\Users\a\Documents\App\file.txt"
        ));
    }
    /// restore targets must be absolute drive paths — relative and
    /// UNC shapes were previously accepted by the string gate.
    #[test]
    fn restore_target_refuses_relative_and_unc() {
        assert!(!super::is_safe_restore_target(Path::new(
            r"Windows\evil.dll"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"\\srv\share\evil.dll"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"\srv\share\evil.dll"
        )));
        assert!(super::is_safe_restore_target(Path::new(
            r"C:\Users\a\AppData\Local\App\f.txt"
        )));
    }

    /// `..` / `.` segments must be refused on the raw request, not only after
    /// Win32 trailing-dot stripping (which would empty `..`).
    #[test]
    fn restore_target_refuses_dot_segments() {
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\a\..\..\Windows\x"
        )));
        assert!(!super::is_safe_restore_target(Path::new(
            r"C:\Users\a\AppData\Local\App\..\..\..\..\Windows\x"
        )));
        assert!(!super::is_safe_restore_target(Path::new(r"C:\a\.\x")));
    }
}
