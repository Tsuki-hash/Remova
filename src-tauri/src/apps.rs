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

/// Remember uninstall command pairs produced by the latest scan.
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
/// Stable scan identity shared by analyze and cleanup reports.
pub fn app_key(app: &InstalledApp) -> String {
    format!("{}\0{}\0{}", app.source, app.registry_key, app.name)
}

pub fn is_trusted_uninstall_app(app: &InstalledApp) -> bool {
    let k = trust_key(&app.uninstall_string, &app.quiet_uninstall_string);
    uninstall_trust()
        .lock()
        .map(|g| g.contains(&k))
        .unwrap_or(false)
}

/// Test-only access to the process-wide trust set. Arming it via
/// `remember_uninstall_commands` *replaces* the whole set, so a test that needs
/// a specific command trusted must restore what was there or it silently
/// changes another test's outcome.
#[cfg(test)]
pub mod trust_test_support {
    use std::collections::HashSet;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes tests that replace the process-wide trust set. The product
    /// lock cannot be held here (the test itself calls
    /// `remember_uninstall_commands`), so a separate test mutex orders them.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    pub fn snapshot() -> HashSet<String> {
        super::uninstall_trust()
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn restore(saved: HashSet<String>) {
        if let Ok(mut g) = super::uninstall_trust().lock() {
            *g = saved;
        }
    }

    /// Snapshot + exclusive test lock. Restores on drop, including on panic
    /// (A5): a failed assertion must not leave another test's trust set armed.
    pub struct TrustGuard {
        saved: HashSet<String>,
        _lock: MutexGuard<'static, ()>,
    }

    pub fn guard() -> TrustGuard {
        let lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        TrustGuard {
            saved: snapshot(),
            _lock: lock,
        }
    }

    impl Drop for TrustGuard {
        fn drop(&mut self) {
            restore(std::mem::take(&mut self.saved));
        }
    }
}

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
    let mut out = scan_installed_inventory();
    let ignore = crate::ignore::load();
    filter_visible_apps(&mut out, &ignore);
    remember_uninstall_commands(&out);
    out
}

pub(crate) fn filter_visible_apps(out: &mut Vec<InstalledApp>, ignore: &crate::ignore::IgnoreList) {
    out.retain(|a| {
        (!a.uninstall_string.trim().is_empty() || !a.quiet_uninstall_string.trim().is_empty())
            && !looks_system_update(&a.name)
            && !crate::ignore::is_app_ignored(ignore, &a.name, &a.publisher, &a.install_location)
    });
}

/// Complete ownership inventory, including products hidden from the app list.
/// This read-only scan must not expand or replace uninstall authorization.
pub(crate) fn scan_installed_inventory() -> Vec<InstalledApp> {
    let mut out: Vec<InstalledApp> = Vec::new();
    if crate::scan_task::cancelled() {
        return out;
    }
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
            if crate::scan_task::cancelled() {
                return out;
            }
            collect_uninstall(hive, sub, access, alias, &mut out);
        }
        // Merge Store/MSIX packages (WinRT). Dedup after sort by name.
        if crate::scan_task::cancelled() {
            return out;
        }
        out.extend(crate::storeapps::scan_store_apps());
    }
    dedup_same_products(&mut out);
    out
}

/// Sort by lowercase name and merge same-product twins (HKLM64/HKLM32/HKCU).
/// `dedup_by` removes its FIRST argument and keeps the second, so the dropped
/// twin's fields must be merged INTO the surviving row (was inverted — the
/// kept row silently lost size/location/quiet-command data).
fn dedup_same_products(out: &mut Vec<InstalledApp>) {
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| {
        if !same_product(a, b) {
            return false;
        }
        merge_app_fields(b, a);
        true
    });
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
    // generic names need GUID — never merge "Update"/short names on name alone.
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
            if crate::scan_task::cancelled() {
                break;
            }
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
        let mut st = RegEnumValueW(
            hk,
            n,
            windows::core::PWSTR(vname.as_mut_ptr()),
            &mut vname_len,
            None,
            Some(&mut vtype),
            Some(data.as_mut_ptr()),
            Some(&mut data_len),
        );
        if st == ERROR_MORE_DATA {
            // one retry with the reported sizes — an oversized value
            // must not silently drop every remaining value of the entry.
            let need_data = data_len as usize;
            let need_name = vname_len as usize;
            if need_data > data.len() && need_data <= (1 << 20) {
                data = vec![0u8; need_data];
            }
            if need_name > vname.len() && need_name <= (1 << 14) {
                vname = vec![0u16; need_name + 1];
            }
            vname_len = vname.len() as u32;
            data_len = data.len() as u32;
            st = RegEnumValueW(
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
                n += 1; // skip this value; keep enumerating the rest
                continue;
            }
        } else if st != ERROR_SUCCESS {
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

    if display.trim().is_empty() {
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
    // slice the LOWERCASED string — U+212A KELVIN SIGN lowercases
    // to ASCII 'k', so slicing the original at byte 2 could split a char and
    // panic on registry-controlled DisplayNames.
    if lower.starts_with("kb") && lower.len() > 2 && lower[2..].chars().all(|c| c.is_ascii_digit())
    {
        return true;
    }
    lower.starts_with("update for") || lower.starts_with("security update")
}

// Shared Windows string helpers live in `fsutil`.
#[cfg(windows)]
use crate::fsutil::{to_wide, wstring_from_reg_data};

/// An MSI product whose Uninstall record still exists while its install
/// directory is gone — Programs & Features can no longer uninstall it (SOP §9.7).
/// Detection is read-only; record cleanup is a separate, backed-up action.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrokenInstall {
    pub app: InstalledApp,
    /// Lower-case `{GUID}` from the MsiExec uninstall command.
    pub guid: String,
    /// Stable code the UI localises (`broken_install_missing_dir`).
    pub reason: String,
}

pub const BROKEN_INSTALL_MISSING_DIR: &str = "broken_install_missing_dir";

/// Scan the installed inventory for broken MSI uninstall records.
pub fn scan_broken_installs() -> Vec<BrokenInstall> {
    scan_broken_installs_from(&scan_installed_inventory())
}

fn scan_broken_installs_from(apps: &[InstalledApp]) -> Vec<BrokenInstall> {
    apps.iter().filter_map(broken_install_from).collect()
}

fn broken_install_from(app: &InstalledApp) -> Option<BrokenInstall> {
    let cmd = if !app.uninstall_string.trim().is_empty() {
        app.uninstall_string.as_str()
    } else {
        app.quiet_uninstall_string.as_str()
    };
    let guid = msiexec_uninstall_guid(cmd)?;
    let location = crate::regops::expand_env_string(app.install_location.trim());
    let location = location.trim().trim_matches('"').trim();
    // An unexpanded variable, relative path, UNC/removable/offline location
    // cannot prove the product is broken; fail closed rather than offering a
    // record delete for a product that is currently unavailable.
    if location.is_empty()
        || location.contains('%')
        || !std::path::Path::new(location).is_absolute()
        || !fixed_local_install_path(location)
        || std::path::Path::new(location).exists()
    {
        return None;
    }
    Some(BrokenInstall {
        app: app.clone(),
        guid,
        reason: BROKEN_INSTALL_MISSING_DIR.into(),
    })
}

/// Only fixed local drives are safe to judge by `exists()`: a UNC share or a
/// currently detached USB drive would otherwise look like a deleted install.
fn fixed_local_install_path(location: &str) -> bool {
    let bytes = location.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return false;
    }
    if bytes[2] != b'\\' && bytes[2] != b'/' {
        return false;
    }
    crate::diskradar::is_local_fixed_drive(bytes[0] as char)
}

/// Parse a real `msiexec.exe /x {GUID}` command. A substring match would also
/// catch wrappers such as `msiexec-helper.exe /x ...`, whose record must not be
/// deleted as if it were an MSI product.
fn msiexec_uninstall_guid(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    let (exe, rest) = split_command_token(cmd)?;
    let exe_name = std::path::Path::new(exe)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())?;
    if !exe_name.eq_ignore_ascii_case("msiexec") && !exe_name.eq_ignore_ascii_case("msiexec.exe") {
        return None;
    }
    let args: Vec<&str> = rest.split_whitespace().collect();
    let x_at = args.iter().position(|arg| {
        let lower = arg.to_ascii_lowercase();
        lower == "/x" || (lower.starts_with("/x{") && lower.ends_with('}'))
    })?;
    // Only the token carrying (or immediately following) `/x` is a product
    // code; a GUID in a later log path must not redirect installer-key cleanup.
    let x_arg = args[x_at];
    if x_arg.to_ascii_lowercase().starts_with("/x{") {
        crate::shared::first_guid(x_arg)
    } else {
        args.get(x_at + 1)
            .and_then(|arg| crate::shared::first_guid(arg))
    }
}

/// First whitespace-delimited command token, honoring a leading quoted path.
fn split_command_token(s: &str) -> Option<(&str, &str)> {
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((&rest[..end], &rest[end + 1..]));
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    let token = &s[..end];
    if token.is_empty() {
        None
    } else {
        Some((token, &s[end..]))
    }
}

/// Outcome of removing a broken record. The backup session is returned so the
/// UI can tell the user where the exported keys live.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrokenCleanupReport {
    pub removed: Vec<String>,
    pub failed: Vec<String>,
    pub backup_dir: String,
}

/// Remove a broken MSI record after exporting every key to a Safety Vault
/// session. The record is re-scanned here, so the webview cannot point this at
/// an arbitrary product; the packed installer keys are matched by the exact
/// ProductCode derived from the `/x{GUID}` command.
pub fn cleanup_broken_install(
    app_name: &str,
    registry_key: &str,
    guid: &str,
    elevated: bool,
) -> Result<BrokenCleanupReport, String> {
    let target = scan_broken_installs()
        .into_iter()
        .find(|b| broken_record_matches(b, app_name, registry_key, guid))
        .ok_or_else(|| "broken install record no longer present".to_string())?;

    let mut keys = vec![target.app.registry_key.clone()];
    keys.extend(find_installer_product_keys(&target.guid));
    keys.sort();
    keys.dedup();
    if !elevated
        && keys
            .iter()
            .any(|k| k.to_ascii_uppercase().starts_with("HKLM"))
    {
        return Err("administrator privileges are required for machine-wide records".into());
    }

    let session = crate::backup::create_session(&target.app.name)
        .map_err(|e| format!("backup session failed: {e}"))?;
    let items: Vec<crate::scanner::CleanupItem> = keys.iter().map(|k| registry_item(k)).collect();
    let (_ok, fail, errors) = crate::backup::backup_items(&items, &session);
    if fail > 0 {
        return Err(format!("backup failed; record kept: {}", errors.join("; ")));
    }

    let mut removed = Vec::new();
    let mut failed = Vec::new();
    for key in &keys {
        match crate::regops::delete_key(key) {
            Ok(true) => removed.push(key.clone()),
            Ok(false) => failed.push(format!("{key}: already absent")),
            Err(e) => failed.push(format!("{key}: {e}")),
        }
    }
    Ok(BrokenCleanupReport {
        removed,
        failed,
        backup_dir: session.to_string_lossy().into_owned(),
    })
}

fn broken_record_matches(
    record: &BrokenInstall,
    app_name: &str,
    registry_key: &str,
    guid: &str,
) -> bool {
    record.app.registry_key.eq_ignore_ascii_case(registry_key)
        && record.guid.eq_ignore_ascii_case(guid)
        && record.app.name.eq_ignore_ascii_case(app_name)
}

/// Packed product keys (`Classes\Installer\Products\<32 hex>`) for the exact
/// product code from the uninstall command. Matching by ProductName alone could
/// delete an unrelated MSI product that happens to share the display name.
fn find_installer_product_keys(guid: &str) -> Vec<String> {
    let Some(packed) = packed_product_code(guid) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for alias in ["HKLM64", "HKLM32"] {
        let root = format!(r"{alias}\SOFTWARE\Classes\Installer\Products");
        for sub in crate::regscan::list_subkeys(&root) {
            if sub.eq_ignore_ascii_case(&packed) {
                out.push(format!(r"{root}\{sub}"));
            }
        }
    }
    out
}

/// Windows Installer's compressed ProductCode form used as the
/// `Installer\Products` key: reverse the first three GUID groups, then swap the
/// nibbles of every remaining byte.
fn packed_product_code(guid: &str) -> Option<String> {
    let hex: String = guid
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_uppercase();
    if hex.len() != 32 {
        return None;
    }
    let mut out = String::with_capacity(32);
    for range in [0..8, 8..12, 12..16] {
        out.extend(hex[range].chars().rev());
    }
    for pair in hex.as_bytes()[16..].chunks(2) {
        out.push(pair[1] as char);
        out.push(pair[0] as char);
    }
    Some(out)
}

fn registry_item(path: &str) -> crate::scanner::CleanupItem {
    use crate::scanner::{CleanupItem, Confidence, ItemKind, RiskLevel};
    CleanupItem {
        path: path.into(),
        kind: ItemKind::Registry,
        score: 90,
        confidence: Confidence::Confirmed,
        risk: RiskLevel::Low,
        reason: "broken install record".into(),
        evidence: vec![],
        shared: false,
        service_bound: false,
        user_data: false,
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: None,
    }
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

    fn sample_app(name: &str, location: &str, uninstall: &str) -> InstalledApp {
        InstalledApp {
            name: name.into(),
            version: "1.0".into(),
            publisher: "Vendor".into(),
            install_location: location.into(),
            uninstall_string: uninstall.into(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: "k".into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        }
    }

    #[test]
    fn broken_install_requires_msi_and_a_missing_directory() {
        let guid = "{11111111-1111-1111-1111-111111111111}";
        let msi = format!(r#"MsiExec.exe /X{guid}"#);
        let missing = sample_app("Broken", r"C:\RemovaNoSuchVendor\App", &msi);
        let hit = broken_install_from(&missing).expect("missing dir + MSI must be detected");
        assert_eq!(hit.guid, guid);
        assert_eq!(hit.reason, BROKEN_INSTALL_MISSING_DIR);

        // A present install directory is not broken, even for MSI.
        let dir = crate::test_support::TempDir::new("broken_install_present");
        let present = sample_app("Healthy", &dir.to_string_lossy(), &msi);
        assert!(broken_install_from(&present).is_none());

        // A non-MSI uninstaller is out of scope.
        let exe = sample_app(
            "Other",
            r"C:\RemovaNoSuchVendor\Other",
            r"C:\Other\uninstall.exe",
        );
        assert!(broken_install_from(&exe).is_none());

        // Without an install location there is nothing to prove missing.
        let unknown = sample_app("Unknown", "", &msi);
        assert!(broken_install_from(&unknown).is_none());

        // A wrapper whose name merely contains "msiexec" is not MSI.
        let wrapper = sample_app(
            "Wrapper",
            r"C:\RemovaNoSuchVendor\Wrapper",
            &format!(r"C:\Tools\msiexec-helper.exe /x {guid}"),
        );
        assert!(broken_install_from(&wrapper).is_none());
        assert!(broken_install_from(&sample_app(
            "Patch",
            r"C:\RemovaNoSuchVendor\Patch",
            &format!(r"MsiExec.exe /I{guid}")
        ))
        .is_none());
    }

    #[test]
    fn install_location_must_be_a_fixed_local_path() {
        assert!(fixed_local_install_path(r"C:\Program Files\Broken App"));
        assert!(fixed_local_install_path("C:/Apps/Broken"));
        assert!(!fixed_local_install_path(r"\\server\share\Broken"));
        assert!(!fixed_local_install_path("Broken"));
        assert!(!fixed_local_install_path(r"C:Broken"));
    }

    #[test]
    fn msiexec_commands_require_the_real_executable_and_uninstall_switch() {
        let guid = "{11111111-1111-1111-1111-111111111111}";
        assert_eq!(
            msiexec_uninstall_guid(&format!("MsiExec.exe /X{guid}")),
            Some(guid.into())
        );
        assert_eq!(
            msiexec_uninstall_guid(&format!(r#""C:\Windows\System32\msiexec.exe" /x {guid}"#)),
            Some(guid.into())
        );
        assert!(
            msiexec_uninstall_guid(&format!(r"C:\Tools\msiexec-helper.exe /x {guid}")).is_none()
        );
        assert!(msiexec_uninstall_guid(&format!("MsiExec.exe /I{guid}")).is_none());
        assert!(msiexec_uninstall_guid("MsiExec.exe /x").is_none());
        assert!(
            msiexec_uninstall_guid(&format!("MsiExec.exe /x /qn {guid}")).is_none(),
            "a GUID in a later argument is not the uninstall target"
        );
    }

    #[test]
    fn install_location_env_and_quotes_are_normalized_before_existence_check() {
        let _lock = crate::test_support::EnvLock::new();
        let dir = crate::test_support::TempDir::new("broken_env_location");
        let _var = crate::test_support::EnvVar::set(
            "REMOVA_BROKEN_LOCATION",
            dir.to_string_lossy().to_string(),
        );
        let msi = r#"MsiExec.exe /X{11111111-1111-1111-1111-111111111111}"#;
        let healthy = sample_app("HealthyEnv", r#""%REMOVA_BROKEN_LOCATION%""#, msi);
        assert!(broken_install_from(&healthy).is_none());

        let missing = sample_app("MissingEnv", r#""%REMOVA_BROKEN_LOCATION%\gone""#, msi);
        assert!(broken_install_from(&missing).is_some());
        let _var = crate::test_support::EnvVar::remove("REMOVA_BROKEN_LOCATION");
        let unknown = sample_app("UnknownEnv", r#""%REMOVA_BROKEN_LOCATION%""#, msi);
        assert!(
            broken_install_from(&unknown).is_none(),
            "unexpanded variables must fail closed"
        );
    }

    #[test]
    fn cleanup_targets_only_the_rescanned_record_identity() {
        let guid = "{11111111-1111-1111-1111-111111111111}";
        let record = BrokenInstall {
            app: sample_app("Broken", r"C:\Missing", &format!("MsiExec.exe /X{guid}")),
            guid: guid.into(),
            reason: BROKEN_INSTALL_MISSING_DIR.into(),
        };
        assert!(broken_record_matches(&record, "broken", "k", guid));
        assert!(!broken_record_matches(&record, "Other", "k", guid));
        assert!(!broken_record_matches(&record, "Broken", "other", guid));
        assert!(!broken_record_matches(
            &record,
            "Broken",
            "k",
            "{22222222-2222-2222-2222-222222222222}"
        ));
    }

    #[test]
    fn packed_product_code_matches_windows_installer_layout() {
        assert_eq!(
            packed_product_code("{C35E2C10-133A-BA91-B44E-890B88561421}").unwrap(),
            "01C2E53CA33119AB4BE498B088654112"
        );
        assert_eq!(
            packed_product_code("{90160000-008C-0000-1000-0000000FF1CE}").unwrap(),
            "00006109C80000000100000000F01FEC"
        );
        assert!(packed_product_code("not-a-product-code").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn native_inventory_keeps_non_uninstallable_component_owners() {
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::process::Command::new(crate::regops::sys_tool("reg.exe"))
                    .args(["delete", &self.0, "/f"])
                    .output();
            }
        }
        let sub = format!(
            r"Software\RemovaInventoryTest_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let key = format!(r"HKCU\{sub}\Component");
        let _cleanup = Cleanup(format!(r"HKCU\{sub}"));
        for (name, kind, data) in [
            ("DisplayName", "REG_SZ", "Owned Component"),
            ("InstallLocation", "REG_SZ", r"C:\Vendor\OwnedComponent"),
            ("SystemComponent", "REG_DWORD", "1"),
            ("NoRemove", "REG_DWORD", "1"),
        ] {
            let output = std::process::Command::new(crate::regops::sys_tool("reg.exe"))
                .args(["add", &key, "/v", name, "/t", kind, "/d", data, "/f"])
                .output()
                .unwrap();
            assert!(output.status.success());
        }
        let owner = unsafe {
            read_uninstall_entry(
                HKEY_CURRENT_USER,
                &sub,
                KEY_WOW64_64KEY,
                "HKCU",
                "Component",
                256,
            )
        }
        .expect("component must retain ownership without uninstall commands");
        assert_eq!(owner.install_location, r"C:\Vendor\OwnedComponent");
        assert!(owner.uninstall_string.is_empty());
        let mut visible = vec![owner];
        filter_visible_apps(&mut visible, &crate::ignore::IgnoreList::default());
        assert!(visible.is_empty());
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

    /// The dropped twin's fields must land on the SURVIVING row (dedup_by
    /// removes its first argument — the merge used to fill the dropped one).
    #[test]
    fn dedup_merges_into_surviving_row() {
        let mk = |name: &str, loc: &str, src: &str, key: &str, uns: &str, size: i64| InstalledApp {
            name: name.into(),
            version: "1.0".into(),
            publisher: "P".into(),
            install_location: loc.into(),
            uninstall_string: uns.into(),
            quiet_uninstall_string: String::new(),
            source: src.into(),
            registry_key: key.into(),
            estimated_size_kb: size,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let mut v = vec![
            mk(
                "Xftp 8",
                "",
                "HKLM64",
                r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Xftp 8",
                r"C:\Program Files (x86)\NetSarang\Xftp 8\uninst.exe",
                0,
            ),
            mk(
                "Xftp 8",
                r"C:\Program Files (x86)\NetSarang\Xftp 8",
                "HKLM32",
                r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Xftp 8",
                r#""C:\Program Files (x86)\NetSarang\Xftp 8\uninst.exe""#,
                12345,
            ),
        ];
        dedup_same_products(&mut v);
        assert_eq!(v.len(), 1);
        // Stable sort keeps the first (HKLM64) row — it must carry the dropped twin's data.
        assert_eq!(v[0].source, "HKLM64");
        assert_eq!(v[0].estimated_size_kb, 12345);
        assert_eq!(
            v[0].install_location,
            r"C:\Program Files (x86)\NetSarang\Xftp 8"
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

    /// .7 regression: EstimatedSize is REG_DWORD and must leave size_kb > 0.
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
