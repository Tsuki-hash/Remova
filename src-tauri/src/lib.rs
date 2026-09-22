//! Remova core library (Tauri backend).

pub mod ai;
pub mod apps;
pub mod association;
pub mod backup;
pub mod commands;
pub mod constants;
pub mod dirsize;
pub mod error;
pub mod executor;
pub mod fsutil;
pub mod history;
pub mod icon;
pub mod ignore;
pub mod installmon;
pub mod manage;
pub mod orphans;
#[cfg(test)]
pub mod path_value_smoke;
#[cfg(test)]
pub mod pipeline_smoke;
pub mod policy;
pub mod regops;
pub mod regscan;
pub mod restore;
pub mod safety;
pub mod scanner;
pub mod shared;
pub mod storeapps;
pub mod sysops;

use apps::InstalledApp;
use executor::{CleanupReport, FullCleanupOptions, FullCleanupReport};
use scanner::{CleanupItem, ScanResult};
use tauri::Manager;

#[tauri::command]
async fn list_installed_apps() -> Result<Vec<InstalledApp>, String> {
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
#[tauri::command]
async fn estimate_dir_size_kb(path: String) -> Result<i64, String> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Ok(0);
    }
    let gen = dirsize::current_batch();
    tauri::async_runtime::spawn_blocking(move || {
        let kb = dirsize::walk_size_kb(std::path::Path::new(&path));
        if dirsize::batch_stale(gen) {
            0
        } else {
            kb
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

/// Stable errors: `open_path:empty` | `open_path:not_found` | `open_path:failed`.
#[tauri::command]
fn open_path_in_explorer(path: String) -> Result<(), String> {
    let path = path.trim().trim_matches('"').trim();
    if path.is_empty() {
        return Err("open_path:empty".into());
    }
    // HTTP(S) updates: launch default browser via `start`.
    if path.starts_with("http://") || path.starts_with("https://") {
        use std::process::Command;
        let mut cmd = Command::new(regops::sys_tool("cmd.exe"));
        cmd.args(["/C", "start", "", path]);
        regops::hide_console(&mut cmd);
        return cmd
            .spawn()
            .map(|_| ())
            .map_err(|_| "open_path:failed".to_string());
    }
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err("open_path:not_found".into());
    }
    use std::process::Command;
    let is_file = p.is_file();
    let mut last_err: Option<std::io::Error> = None;
    for explorer in [
        regops::sys_tool("explorer.exe"),
        "explorer.exe".to_string(),
        "explorer".to_string(),
    ] {
        let mut cmd = Command::new(&explorer);
        if is_file {
            // Single argument form so Explorer selects the file in its parent.
            cmd.arg(format!("/select,{path}"));
        } else {
            cmd.arg(path);
        }
        regops::hide_console(&mut cmd);
        match cmd.spawn() {
            Ok(_) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(match last_err {
        Some(e) if e.kind() == std::io::ErrorKind::NotFound => "open_path:not_found".into(),
        Some(e) => format!("open_path:failed:{}", e),
        None => "open_path:failed".into(),
    })
}

/// Return `data:image/png;base64,...` for the app icon, or null.
#[tauri::command]
async fn app_icon_data(display_icon: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let png = icon::extract_icon_png(&display_icon)?;
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

#[tauri::command]
async fn analyze_associations(app: InstalledApp) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        scanner::analyze_associations(
            &app.name,
            &app.install_location,
            &app.publisher,
            &app.registry_key,
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
        history::append(
            &report.app_name,
            report.deleted,
            report.failed,
            report.skipped,
            report.delayed,
            report.aborted,
            report.dry_run,
            &report.backup_dir,
        );
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
                return Ok(false);
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
            Ok(ok.is_ok() && elev.TokenIsElevated != 0)
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
        // System drive, not hardcoded C: (FUNC-6)
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
async fn elevate_restart() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| sysops::elevate_relaunch(&[]))
        .await
        .map_err(|e| e.to_string())?
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
        let installed = apps::scan_installed_apps();
        orphans::scan_orphans(&installed)
    })
    .await
    .map_err(|e| e.to_string())
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
async fn end_install_monitor() -> Result<installmon::MonitorDiff, String> {
    tauri::async_runtime::spawn_blocking(installmon::end)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn monitor_diff_to_items(
    diff: installmon::MonitorDiff,
) -> Result<Vec<scanner::CleanupItem>, String> {
    Ok(installmon::diff_to_cleanup_items(&diff))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // One-shot Safety Vault retention (not on list/read paths).
    std::thread::spawn(|| {
        let _ = restore::prune_old_sessions(crate::constants::BACKUP_RETENTION_DAYS);
    });
    let builder = tauri::Builder::default()
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
            let icon = match app.default_window_icon().cloned() {
                Some(i) => i,
                None => {
                    eprintln!("[remova] default window icon missing — tray disabled");
                    return Ok(());
                }
            };
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
                    "quit" => app.exit(0),
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
            analyze_associations,
            run_cleanup_dry_run,
            run_full_cleanup,
            run_official_uninstall,
            commands::history_cmd::list_cleanup_history,
            commands::history_cmd::export_history_csv,
            is_elevated,
            disk_usage,
            elevate_restart,
            commands::backup_cmd::list_restore_sessions,
            commands::backup_cmd::list_backup_sessions,
            commands::backup_cmd::delete_backup_session,
            commands::backup_cmd::restore_session_by_name,
            commands::manage_cmd::list_startup_items,
            commands::manage_cmd::list_services,
            commands::manage_cmd::list_scheduled_tasks,
            commands::manage_cmd::set_startup_enabled,
            commands::manage_cmd::set_service_start_disabled,
            commands::manage_cmd::set_service_running,
            commands::manage_cmd::set_task_enabled,
            commands::context_menu::register_context_menu,
            commands::context_menu::unregister_context_menu,
            commands::ignore_cmd::load_ignore,
            commands::ignore_cmd::ignore_publisher,
            commands::ignore_cmd::ignore_app_name,
            commands::ignore_cmd::suggest_ignore_rules,
            commands::ignore_cmd::apply_ignore_suggestions,
            scan_orphan_leftovers,
            verify_cleanup_leftovers,
            begin_install_monitor,
            end_install_monitor,
            monitor_diff_to_items,
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
