//! Remova core library (Tauri backend).

pub mod ai;
pub mod apps;
pub mod association;
pub mod backup;
pub mod commands;
pub mod constants;
pub mod dirsize;
pub mod diskradar;
pub mod error;
pub mod executor;
pub mod fsutil;
pub mod history;
pub mod icon;
pub mod idle;
pub mod ignore;
pub mod installers;
pub mod installmon;
pub mod manage;
pub mod orphans;
pub mod path_seal;
pub mod policy;
pub mod regops;
pub mod regscan;
pub mod restore;
pub mod safety;
pub mod scan_allow;
pub mod scan_task;
pub mod scanner;
pub mod shared;
pub mod storeapps;
pub mod sysops;
pub mod toolcache;

// End-to-end smokes (temp dirs / path mock only) — kept in the module tree so
// `cargo test` actually compiles and runs them.
#[cfg(test)]
mod path_value_smoke;
#[cfg(test)]
mod pipeline_smoke;

use apps::InstalledApp;
use executor::{CleanupReport, FullCleanupOptions, FullCleanupReport};
use scanner::{CleanupItem, ScanResult};
use tauri::{Emitter, Manager};

#[tauri::command]
async fn list_installed_apps() -> Result<Vec<InstalledApp>, String> {
    // scan_installed_apps already refreshes the uninstall trust table.
    tauri::async_runtime::spawn_blocking(apps::scan_installed_apps)
        .await
        .map_err(|e| e.to_string())
}

/// Clear cancel flag before a new estimate batch.
#[tauri::command]
fn begin_size_estimate() {
    dirsize::clear_cancel();
}

/// Estimate on-disk size of an install location (KB).
/// Runs on the blocking pool so large trees do not freeze the webview.
/// Result is zeroed if the estimate batch was cancelled / superseded.
/// `capped` marks a file-cap partial (floor, not total) — .
#[derive(serde::Serialize)]
struct SizeEstimate {
    kb: i64,
    capped: bool,
}

#[tauri::command]
async fn estimate_dir_size_kb(path: String) -> Result<SizeEstimate, String> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Ok(SizeEstimate {
            kb: 0,
            capped: false,
        });
    }
    let gen = dirsize::current_batch();
    tauri::async_runtime::spawn_blocking(move || {
        // Honors the global cancel flag (batch cancel must zero in-flight walks).
        let (kb, capped) = dirsize::walk_size_kb_capped(std::path::Path::new(&path));
        if dirsize::batch_stale(gen) {
            SizeEstimate {
                kb: 0,
                capped: false,
            }
        } else {
            SizeEstimate { kb, capped }
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Cancel in-flight directory size walks (they return 0).
#[tauri::command]
fn cancel_size_estimate() {
    dirsize::request_cancel();
}

/// -01: strict URL shape for `open_path` (scheme + host, no control/quote/space).
/// plain `http://` only for loopback (local Ollama etc.); remote must be https.
fn is_safe_http_url(url: &str) -> bool {
    let is_https = url.starts_with("https://");
    let is_http = url.starts_with("http://");
    if !is_https && !is_http {
        return false;
    }
    if url
        .chars()
        .any(|c| c.is_control() || c == '"' || c == '\'' || c == ' ' || c == '\t')
    {
        return false;
    }
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or("");
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains(['@', '\\']) {
        return false;
    }
    // Bracketed IPv6 (`[::1]:8080`) — a plain colon split would truncate to "[".
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        match rest.split_once(']') {
            Some((h, tail)) => {
                if h.parse::<std::net::Ipv6Addr>().is_err() {
                    return false;
                }
                let port = if tail.is_empty() {
                    None
                } else {
                    match tail.strip_prefix(':') {
                        Some(p) => Some(p),
                        None => return false,
                    }
                };
                (format!("[{h}]"), port)
            }
            None => return false,
        }
    } else {
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(h, p)| (h, Some(p)));
        (host.to_string(), port)
    };
    if port.is_some_and(|p| {
        p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()) || p.parse::<u16>().is_err()
    }) {
        return false;
    }
    if host.is_empty()
        || !host.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || c == '.'
                || c == '-'
                || c == '['
                || c == ']'
                || (c == ':' && host.starts_with('[') && host.ends_with(']'))
        })
    {
        return false;
    }
    if is_http {
        // loopback only (IPv4 / localhost / [::1])
        let low = host.to_ascii_lowercase();
        return low == "localhost"
            || low == "[::1]"
            || low
                .parse::<std::net::Ipv4Addr>()
                .is_ok_and(|ip| ip.is_loopback());
    }
    true
}

/// Launch a validated http(s) URL via ShellExecuteW "open" (no shell metacharacter parsing).
#[cfg(windows)]
fn open_url_shell(url: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let url_w: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let empty: Vec<u16> = vec![0];
    unsafe {
        let rc = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(url_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR(empty.as_ptr()),
            SW_SHOWNORMAL,
        );
        if (rc.0 as isize) > 32 {
            Ok(())
        } else {
            Err("open_path:failed".into())
        }
    }
}

#[cfg(not(windows))]
fn open_url_shell(_url: &str) -> Result<(), String> {
    Err("open_path:failed".into())
}

/// Stable errors: `open_path:empty` | `open_path:not_found` | `open_path:failed`.
#[tauri::command]
fn open_path_in_explorer(path: String) -> Result<(), String> {
    let path = path.trim().trim_matches('"').trim();
    if path.is_empty() {
        return Err("open_path:empty".into());
    }
    // HTTP(S): ShellExecuteW + strict URL shape (-01 — no `cmd /C start` injection surface).
    if path.starts_with("http://") || path.starts_with("https://") {
        if !is_safe_http_url(path) {
            return Err("open_path:failed".into());
        }
        return open_url_shell(path);
    }
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err("open_path:not_found".into());
    }
    use std::process::Command;
    let is_file = p.is_file();
    let explorer = regops::explorer_exe();
    if explorer.is_empty() {
        return Err("open_path:failed".into());
    }
    let mut cmd = Command::new(&explorer);
    if is_file {
        // Single argument form so Explorer selects the file in its parent.
        cmd.arg(format!("/select,{path}"));
    } else {
        cmd.arg(path);
    }
    regops::hide_console(&mut cmd);
    cmd.spawn().map(|_| ()).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "open_path:not_found".into(),
        _ => "open_path:failed".into(),
    })
}

/// Return `data:image/png;base64,...` for the app icon, or null.
#[tauri::command]
async fn app_icon_data(display_icon: Option<String>) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let raw = display_icon?;
        if raw.trim().is_empty() {
            return None;
        }
        let png = icon::extract_icon_png(&raw)?;
        use base64_light::*;
        Some(format!("data:image/png;base64,{}", b64_encode(&png)))
    })
    .await
    .map_err(|e| e.to_string())
}

mod base64_light {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn b64_encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(ALPHABET[(n >> 6) as usize & 63] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(ALPHABET[n as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::b64_encode;

        #[test]
        fn encode_known() {
            assert_eq!(b64_encode(b"Man"), "TWFu");
            assert_eq!(b64_encode(b"Ma"), "TWE=");
            assert_eq!(b64_encode(b"M"), "TQ==");
        }
    }
}

#[cfg(test)]
mod open_path_url_tests {
    // -01: strict URL whitelist for open_path (no cmd /C start).
    #[test]
    fn http_url_whitelist() {
        assert!(super::is_safe_http_url(
            "https://github.com/Tsuki-hash/Remova/releases"
        ));
        assert!(super::is_safe_http_url("http://127.0.0.1:11434/v1"));
        assert!(super::is_safe_http_url("http://localhost:11434/v1"));
        assert!(super::is_safe_http_url("http://127.9.8.7:11434/v1"));
        assert!(super::is_safe_http_url("http://[::1]:11434/v1"));
        for url in [
            "http://127.0.0.1.evil.com",
            "http://127.999.0.1",
            "http://127.1",
            "http://128.0.0.1",
            "http://127.0.0.1@evil.com",
            "http://127.0.0.1:80@evil.com",
            "http://[::1]:80@evil.com",
            "http://[::1]evil.com",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:",
            "http://127.0.0.1:abc",
        ] {
            assert!(!super::is_safe_http_url(url), "must reject {url}");
        }
        // remote cleartext http is refused
        assert!(!super::is_safe_http_url("http://example.com/x"));
        assert!(!super::is_safe_http_url("https://"));
        assert!(!super::is_safe_http_url("ftp://example.com/x"));
        assert!(!super::is_safe_http_url("https://evil.com/\"&calc.exe"));
        assert!(!super::is_safe_http_url("https://evil.com/ path"));
        assert!(!super::is_safe_http_url("javascript:alert(1)"));
    }
}

#[tauri::command]
async fn analyze_associations(app: InstalledApp) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        scanner::analyze_associations(
            &app.name,
            &app.install_location,
            &app.publisher,
            &app.registry_key,
            &app.source,
        )
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_cleanup_dry_run(
    app: InstalledApp,
    items: Vec<CleanupItem>,
    cleanup_source: Option<String>,
) -> Result<CleanupReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = match cleanup_source.as_deref() {
            Some("orphan") => crate::policy::CleanupSource::Orphan,
            Some("monitor") => crate::policy::CleanupSource::Monitor,
            Some("copilot") => crate::policy::CleanupSource::Copilot,
            Some("installer") => crate::policy::CleanupSource::Installer,
            Some("toolcache") => crate::policy::CleanupSource::ToolCache,
            _ => crate::policy::CleanupSource::Uninstall,
        };
        executor::run_cleanup_dry_for_app_source(&app, &items, source)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_official_uninstall(
    app: InstalledApp,
) -> Result<executor::OfficialUninstallResult, String> {
    tauri::async_runtime::spawn_blocking(move || executor::run_official_uninstall(&app))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_full_cleanup(
    app: InstalledApp,
    items: Vec<CleanupItem>,
    options: FullCleanupOptions,
) -> Result<FullCleanupReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let report = executor::run_full_cleanup(&app, &items, &options);
        if !history::append(
            &report.app_name,
            report.deleted,
            report.failed,
            report.skipped,
            report.delayed,
            report.aborted,
            report.dry_run,
            &report.backup_dir,
        ) {
            eprintln!("cleanup completed, but history append failed");
        }
        report
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn is_elevated() -> Result<bool, String> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            let mut token = windows::Win32::Foundation::HANDLE::default();
            if OpenProcessToken(
                GetCurrentProcess(),
                windows::Win32::Security::TOKEN_QUERY,
                &mut token,
            )
            .is_err()
            {
                return Err("elevation:query_failed".into());
            }
            let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut ret = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elev as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret,
            );
            let _ = CloseHandle(token);
            ok.map_err(|_| "elevation:query_failed".to_string())?;
            Ok(elev.TokenIsElevated != 0)
        }
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

#[derive(serde::Serialize)]
struct DiskInfo {
    free_gb: f64,
    total_gb: f64,
    drive: String,
}

#[tauri::command]
fn disk_usage() -> Result<DiskInfo, String> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        // System drive, not hardcoded C:
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let drive = drive.trim_end_matches('\\').to_uppercase();
        let root = format!("{drive}\\\0");
        let wide: Vec<u16> = root.encode_utf16().collect();
        let mut free = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        unsafe {
            let ok = GetDiskFreeSpaceExW(
                PCWSTR(wide.as_ptr()),
                Some(&mut free),
                Some(&mut total),
                Some(&mut total_free),
            );
            if ok.is_err() {
                return Err("GetDiskFreeSpaceExW failed".into());
            }
        }
        Ok(DiskInfo {
            free_gb: total_free as f64 / 1024f64.powi(3),
            total_gb: total as f64 / 1024f64.powi(3),
            drive,
        })
    }
    #[cfg(not(windows))]
    {
        Ok(DiskInfo {
            free_gb: 0.0,
            total_gb: 0.0,
            drive: String::new(),
        })
    }
}

#[tauri::command]
async fn elevate_restart(app: tauri::AppHandle, approved: Option<bool>) -> Result<(), String> {
    // Ask the old window to finish its busy confirmation before spawning
    // the elevated copy: user deliberation must not consume the handoff wait.
    if approved != Some(true) {
        return app
            .emit("remova:request-elevate", ())
            .map_err(|e| e.to_string());
    }
    tauri::async_runtime::spawn_blocking(|| {
        // Busy confirmation is complete. The elevated copy waits for the
        // old window to finish exiting before acquiring the instance lock.
        let handoff = format!("--elevated-relaunch={}", std::process::id());
        sysops::elevate_relaunch(&[handoff])
    })
    .await
    .map_err(|e| e.to_string())??;
    // The requesting window destroys itself only after this succeeds.
    Ok(())
}

#[tauri::command]
fn take_pending_analyze() -> Result<Option<String>, String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("no LOCALAPPDATA")?;
    let p = std::path::PathBuf::from(local)
        .join("Remova")
        .join("pending_analyze.txt");
    if !p.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&p);
    let s = s.trim().to_string();
    Ok(if s.is_empty() { None } else { Some(s) })
}

#[tauri::command]
async fn scan_orphan_leftovers() -> Result<Vec<scanner::CleanupItem>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let installed = apps::scan_installed_inventory();
        orphans::scan_orphans(&installed)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Idle software radar (read-only ranking).
#[tauri::command]
async fn rank_idle_apps() -> Result<Vec<idle::IdleApp>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let installed = apps::scan_installed_apps();
        idle::rank_idle_apps(&installed)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Installer packages + updater caches (scoped delete allow-list).
#[tauri::command]
async fn scan_installer_caches() -> Result<Vec<scanner::CleanupItem>, String> {
    tauri::async_runtime::spawn_blocking(installers::scan_installer_caches)
        .await
        .map_err(|e| e.to_string())
}

/// Dev / game / browser tool caches (scoped delete allow-list).
#[tauri::command]
async fn scan_tool_caches() -> Result<Vec<scanner::CleanupItem>, String> {
    tauri::async_runtime::spawn_blocking(toolcache::scan_tool_caches)
        .await
        .map_err(|e| e.to_string())
}

/// Disk radar: local fixed drives with free/total space (read-only).
#[tauri::command]
async fn list_local_drives() -> Result<Vec<diskradar::DriveInfo>, String> {
    tauri::async_runtime::spawn_blocking(diskradar::list_local_drives)
        .await
        .map_err(|e| e.to_string())
}

/// Disk radar: top directories for one drive (system drive → well-known roots).
#[tauri::command]
async fn list_top_dir_sizes(drive: Option<String>) -> Result<Vec<diskradar::DirSizeRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let letter = drive
            .as_deref()
            .and_then(|s| s.trim().chars().next())
            .filter(|c| c.is_ascii_alphabetic());
        diskradar::top_dir_sizes_for_drive(letter)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Disk radar drill-down: immediate children of a directory (read-only).
#[tauri::command]
async fn list_dir_children(path: String) -> Result<Vec<diskradar::DirSizeRow>, String> {
    tauri::async_runtime::spawn_blocking(move || diskradar::list_dir_children(&path))
        .await
        .map_err(|e| e.to_string())?
}

/// SOP §7: re-check selected paths after cleanup (checklist evidence).
#[tauri::command]
async fn verify_cleanup_leftovers(
    items: Vec<scanner::CleanupItem>,
) -> Result<Vec<sysops::VerifyRow>, String> {
    tauri::async_runtime::spawn_blocking(move || sysops::verify_cleanup_leftovers(&items))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn begin_install_monitor() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(installmon::begin)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn end_install_monitor() -> Result<installmon::MonitorEndResult, String> {
    tauri::async_runtime::spawn_blocking(installmon::end)
        .await
        .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // One-shot Safety Vault retention (not on list/read paths).
    std::thread::spawn(|| {
        let _ = restore::prune_old_sessions(crate::constants::BACKUP_RETENTION_DAYS);
        #[cfg(windows)]
        path_seal::cleanup_abandoned_stages();
    });
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Focus the existing window when a second launch happens
        // (e.g. Explorer context menu while Remova is already open).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .setup(|app| {
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
            use tauri::Manager;

            let show_i = MenuItem::with_id(app, "show", "打开 Remova", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出 Remova", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;
            // Prefer dedicated 32×32 transparent PNG for the tray (crisp at 16–32 px).
            let icon = match tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png")) {
                Ok(i) => i,
                Err(_) => match app.default_window_icon().cloned() {
                    Some(i) => i,
                    None => {
                        eprintln!("[remova] tray icon missing — tray disabled");
                        return Ok(());
                    }
                },
            };
            // Taskbar / alt-tab: larger raster so high-DPI does not upscale a 32px glyph.
            if let Ok(big) =
                tauri::image::Image::from_bytes(include_bytes!("../icons/128x128@2x.png"))
            {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_icon(big);
                }
            }
            let _tray = TrayIconBuilder::with_id("main")
                .icon(icon)
                .tooltip("Remova")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                            let _ = app.emit("remova:request-quit", ());
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_installed_apps,
            app_icon_data,
            estimate_dir_size_kb,
            cancel_size_estimate,
            begin_size_estimate,
            open_path_in_explorer,
            commands::update::check_github_latest,
            commands::update::online_update_supported,
            analyze_associations,
            run_cleanup_dry_run,
            run_full_cleanup,
            run_official_uninstall,
            commands::history_cmd::list_cleanup_history,
            commands::history_cmd::export_history_csv,
            commands::history_cmd::delete_cleanup_history,
            commands::history_cmd::clear_cleanup_history,
            is_elevated,
            disk_usage,
            elevate_restart,
            commands::backup_cmd::list_restore_sessions,
            commands::backup_cmd::list_backup_sessions,
            commands::backup_cmd::delete_backup_session,
            commands::backup_cmd::restore_session_by_name,
            commands::backup_cmd::preview_restore_session,
            commands::manage_cmd::list_startup_items,
            commands::manage_cmd::list_services,
            commands::manage_cmd::list_scheduled_tasks,
            commands::manage_cmd::set_startup_enabled,
            commands::manage_cmd::set_service_start_disabled,
            commands::manage_cmd::set_service_running,
            commands::manage_cmd::set_task_enabled,
            commands::ignore_cmd::load_ignore,
            commands::ignore_cmd::ignore_publisher,
            commands::ignore_cmd::ignore_app_name,
            commands::ignore_cmd::unignore_publisher,
            commands::ignore_cmd::unignore_app_name,
            commands::ignore_cmd::suggest_ignore_rules,
            commands::ignore_cmd::apply_ignore_suggestions,
            scan_orphan_leftovers,
            rank_idle_apps,
            scan_installer_caches,
            scan_tool_caches,
            list_local_drives,
            list_top_dir_sizes,
            list_dir_children,
            verify_cleanup_leftovers,
            begin_install_monitor,
            end_install_monitor,
            take_pending_analyze,
            commands::ai_cmd::get_ai_config,
            commands::ai_cmd::save_ai_config,
            commands::ai_cmd::ai_risk_brief,
            commands::ai_cmd::ai_explain_items,
            commands::ai_cmd::ai_summarize_report,
            commands::ai_cmd::ai_parse_intent
        ]);
    match builder.run(tauri::generate_context!()) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("[remova] failed to run: {e}");
            std::process::exit(1);
        }
    }
}
