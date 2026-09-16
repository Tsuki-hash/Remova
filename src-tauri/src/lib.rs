//! Remova core library (Tauri backend).

pub mod ai;
pub mod apps;
pub mod backup;
pub mod dirsize;
pub mod executor;
pub mod history;
pub mod icon;
pub mod ignore;
pub mod installmon;
pub mod manage;
pub mod orphans;
#[cfg(test)]
pub mod pipeline_smoke;
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
use history::HistoryEntry;
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
#[tauri::command]
async fn estimate_dir_size_kb(path: String) -> Result<i64, String> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Ok(0);
    }
    tauri::async_runtime::spawn_blocking(move || dirsize::walk_size_kb(std::path::Path::new(&path)))
        .await
        .map_err(|e| e.to_string())
}

/// Cancel in-flight directory size walks (they return 0).
#[tauri::command]
fn cancel_size_estimate() {
    dirsize::request_cancel();
}

/// Open Explorer at a path (backup dir, install location).
#[tauri::command]
fn open_path_in_explorer(path: String) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("empty path".into());
    }
    use std::process::Command;
    let explorer = regops::sys_tool("explorer.exe");
    Command::new(explorer)
        .arg(path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_ai_config() -> Result<ai::AiConfigView, String> {
    Ok(ai::AiConfigView::from(&ai::load_config()))
}

/// Save AI settings. Empty `api_key` keeps the stored key unchanged.
#[tauri::command]
fn save_ai_config(
    enabled: bool,
    provider: String,
    base_url: String,
    model: String,
    allow_cloud_paths: bool,
    api_key: Option<String>,
) -> Result<ai::AiConfigView, String> {
    let mut c = ai::load_config();
    c.enabled = enabled;
    c.provider = provider;
    c.base_url = base_url;
    c.model = model;
    c.allow_cloud_paths = allow_cloud_paths;
    if let Some(k) = api_key {
        let k = k.trim().to_string();
        if !k.is_empty() {
            c.api_key = k;
        }
    }
    ai::save_config(&c)?;
    Ok(ai::AiConfigView::from(&c))
}

#[tauri::command]
async fn ai_risk_brief(request: ai::RiskBriefInput) -> Result<Option<String>, String> {
    let cfg = ai::load_config();
    if !cfg.enabled {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || ai::risk_brief(&cfg, &request).ok())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_explain_items(
    app_name: String,
    publisher: String,
    items: Vec<ai::ExplainInput>,
) -> Result<Vec<ai::ExplainOutput>, String> {
    let cfg = ai::load_config();
    if !cfg.enabled {
        return Ok(vec![]);
    }
    tauri::async_runtime::spawn_blocking(move || {
        ai::explain_items(&cfg, &app_name, &publisher, &items).unwrap_or_default()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_summarize_report(request: ai::ReportBriefInput) -> Result<Option<String>, String> {
    let cfg = ai::load_config();
    if !cfg.enabled {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || ai::summarize_report(&cfg, &request).ok())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_parse_intent(text: String, app_names: Vec<String>) -> Result<ai::NlIntent, String> {
    let cfg = ai::load_config();
    if !cfg.enabled {
        return Err("ai disabled".into());
    }
    tauri::async_runtime::spawn_blocking(move || ai::parse_nl_intent(&cfg, &text, &app_names))
        .await
        .map_err(|e| e.to_string())?
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
    app_name: String,
    items: Vec<CleanupItem>,
) -> Result<CleanupReport, String> {
    tauri::async_runtime::spawn_blocking(move || executor::run_cleanup_dry(&app_name, &items))
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
fn list_cleanup_history() -> Result<Vec<HistoryEntry>, String> {
    Ok(history::load(200))
}

#[tauri::command]
fn export_history_csv() -> Result<String, String> {
    let entries = history::load(500);
    let mut out = String::from("app_name,deleted,failed,skipped,aborted,backup_dir,created_at\n");
    for e in entries {
        out.push_str(&format!(
            "\"{}\",{},{},{},{},\"{}\",{}\n",
            e.app_name.replace('"', "'"),
            e.deleted,
            e.failed,
            e.skipped,
            e.aborted,
            e.backup_dir,
            e.created_at
        ));
    }
    Ok(out)
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
fn list_restore_sessions() -> Result<Vec<String>, String> {
    Ok(restore::list_session_names())
}

#[tauri::command]
fn list_backup_sessions() -> Result<Vec<restore::SessionInfo>, String> {
    Ok(restore::list_session_info())
}

#[tauri::command]
async fn delete_backup_session(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || restore::delete_session_by_name(&name))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn restore_session_by_name(name: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || restore::restore_by_name(&name))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_startup_items() -> Result<Vec<manage::ManageItem>, String> {
    tauri::async_runtime::spawn_blocking(manage::list_startup_items)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_services() -> Result<Vec<manage::ManageItem>, String> {
    tauri::async_runtime::spawn_blocking(manage::list_services)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_scheduled_tasks() -> Result<Vec<manage::ManageItem>, String> {
    tauri::async_runtime::spawn_blocking(manage::list_scheduled_tasks)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_startup_enabled(location: String, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || manage::set_startup_enabled(&location, enabled))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_service_start_disabled(name: String, disable: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || manage::set_service_start_disabled(&name, disable))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_task_enabled(name: String, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || manage::set_task_enabled(&name, enabled))
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

const CONTEXT_MENU_KEY: &str = r"HKCU\Software\Classes\*\shell\RemovaDeepUninstall";

#[tauri::command]
async fn register_context_menu() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.to_string_lossy().to_string();
        crate::regops::create_reg_sz(CONTEXT_MENU_KEY, "MUIVerb", "Remova Deep Uninstall")?;
        crate::regops::create_reg_sz(CONTEXT_MENU_KEY, "Icon", &format!("\"{exe}\""))?;
        let cmd_key = format!(r"{CONTEXT_MENU_KEY}\command");
        crate::regops::create_reg_sz(&cmd_key, "", &format!("\"{exe}\" --analyze \"%1\""))?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn unregister_context_menu() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| crate::regops::delete_key(CONTEXT_MENU_KEY))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn load_ignore() -> Result<ignore::IgnoreList, String> {
    Ok(ignore::load())
}

#[tauri::command]
fn ignore_publisher(name: String) -> Result<ignore::IgnoreList, String> {
    ignore::add_publisher(&name)
}

#[tauri::command]
fn ignore_app_name(name: String) -> Result<ignore::IgnoreList, String> {
    ignore::add_name(&name)
}

/// Rule-based ignore suggestions from leftover paths (shared runtimes).
#[tauri::command]
fn suggest_ignore_rules(
    publisher: String,
    paths: Vec<String>,
) -> Result<Vec<ignore::IgnoreSuggestion>, String> {
    Ok(ignore::suggest_from_leftovers(&publisher, &paths))
}

#[tauri::command]
fn apply_ignore_suggestions(
    suggestions: Vec<ignore::IgnoreSuggestion>,
) -> Result<ignore::IgnoreList, String> {
    ignore::apply_suggestions(&suggestions)
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

#[derive(serde::Serialize)]
struct VerifyRow {
    path: String,
    kind: String,
    still_there: bool,
}

/// SOP §7: re-check selected paths after cleanup (checklist evidence).
#[tauri::command]
async fn verify_cleanup_leftovers(
    items: Vec<scanner::CleanupItem>,
) -> Result<Vec<VerifyRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = Vec::new();
        for it in items {
            let still = match it.kind {
                scanner::ItemKind::Registry => {
                    let path = it.path.split('|').next().unwrap_or(&it.path);
                    crate::regscan::list_subkeys(path).len()
                        + crate::regscan::list_values(path).len()
                        > 0
                }
                scanner::ItemKind::Path => crate::regops::scrub_path_entry_ok(&it.path),
                _ => std::path::Path::new(&it.path).exists(),
            };
            out.push(VerifyRow {
                path: it.path,
                kind: format!("{:?}", it.kind).to_lowercase(),
                still_there: still,
            });
        }
        out
    })
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
        let _ = restore::prune_old_sessions(7);
    });
    tauri::Builder::default()
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
            let icon = app
                .default_window_icon()
                .cloned()
                .expect("default window icon required for tray");
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
            analyze_associations,
            run_cleanup_dry_run,
            run_full_cleanup,
            run_official_uninstall,
            list_cleanup_history,
            export_history_csv,
            is_elevated,
            disk_usage,
            elevate_restart,
            list_restore_sessions,
            list_backup_sessions,
            delete_backup_session,
            restore_session_by_name,
            list_startup_items,
            list_services,
            list_scheduled_tasks,
            set_startup_enabled,
            set_service_start_disabled,
            set_task_enabled,
            register_context_menu,
            unregister_context_menu,
            load_ignore,
            ignore_publisher,
            ignore_app_name,
            suggest_ignore_rules,
            apply_ignore_suggestions,
            scan_orphan_leftovers,
            verify_cleanup_leftovers,
            begin_install_monitor,
            end_install_monitor,
            monitor_diff_to_items,
            take_pending_analyze,
            get_ai_config,
            save_ai_config,
            ai_risk_brief,
            ai_explain_items,
            ai_summarize_report,
            ai_parse_intent
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
