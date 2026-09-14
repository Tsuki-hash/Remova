//! Remova core library (Tauri backend).

pub mod apps;
pub mod backup;
pub mod dirsize;
pub mod executor;
pub mod history;
pub mod icon;
pub mod manage;
pub mod regops;
pub mod regscan;
pub mod restore;
pub mod safety;
pub mod scanner;
pub mod storeapps;
pub mod sysops;

use apps::InstalledApp;
use executor::{CleanupReport, FullCleanupOptions, FullCleanupReport};
use history::HistoryEntry;
use scanner::{CleanupItem, ScanResult};

#[tauri::command]
fn list_installed_apps() -> Result<Vec<InstalledApp>, String> {
    Ok(apps::scan_installed_apps())
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

/// Return `data:image/png;base64,...` for the app icon, or null.
#[tauri::command]
fn app_icon_data(display_icon: String) -> Option<String> {
    let png = icon::extract_icon_png(&display_icon)?;
    use base64_light::*;
    Some(format!(
        "data:image/png;base64,{}",
        b64_encode(&png)
    ))
}

mod base64_light {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
fn analyze_associations(app: InstalledApp) -> Result<ScanResult, String> {
    Ok(scanner::analyze_associations(
        &app.name,
        &app.install_location,
        &app.publisher,
        &app.registry_key,
    ))
}

#[tauri::command]
fn run_cleanup_dry_run(app_name: String, items: Vec<CleanupItem>) -> Result<CleanupReport, String> {
    Ok(executor::run_cleanup_dry(&app_name, &items))
}

#[tauri::command]
fn run_full_cleanup(
    app: InstalledApp,
    items: Vec<CleanupItem>,
    options: FullCleanupOptions,
) -> Result<FullCleanupReport, String> {
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
    Ok(report)
}

#[tauri::command]
fn restore_latest_backup() -> Result<Vec<String>, String> {
    let sessions = restore::list_sessions();
    let Some(s) = sessions.first() else {
        return Err("no backup sessions".into());
    };
    restore::restore_session(s)
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
}

#[tauri::command]
fn disk_usage() -> Result<DiskInfo, String> {
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        use windows::core::PCWSTR;
        let root: Vec<u16> = "C:\\\0".encode_utf16().collect();
        let mut free = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        unsafe {
            let ok = GetDiskFreeSpaceExW(
                PCWSTR(root.as_ptr()),
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
        })
    }
    #[cfg(not(windows))]
    {
        Ok(DiskInfo {
            free_gb: 0.0,
            total_gb: 0.0,
        })
    }
}

#[tauri::command]
fn elevate_restart() -> Result<(), String> {
    sysops::elevate_relaunch(&[])
}

#[tauri::command]
fn list_restore_sessions() -> Result<Vec<String>, String> {
    Ok(restore::list_session_names())
}

#[tauri::command]
fn restore_session_by_name(name: String) -> Result<Vec<String>, String> {
    restore::restore_by_name(&name)
}

#[tauri::command]
fn list_startup_items() -> Result<Vec<manage::ManageItem>, String> {
    Ok(manage::list_startup_items())
}

#[tauri::command]
fn list_services() -> Result<Vec<manage::ManageItem>, String> {
    Ok(manage::list_services())
}

#[tauri::command]
fn list_scheduled_tasks() -> Result<Vec<manage::ManageItem>, String> {
    Ok(manage::list_scheduled_tasks())
}

#[tauri::command]
fn set_startup_enabled(location: String, enabled: bool) -> Result<(), String> {
    manage::set_startup_enabled(&location, enabled)
}

#[tauri::command]
fn set_service_start_disabled(name: String, disable: bool) -> Result<(), String> {
    manage::set_service_start_disabled(&name, disable)
}

#[tauri::command]
fn set_task_enabled(name: String, enabled: bool) -> Result<(), String> {
    manage::set_task_enabled(&name, enabled)
}

const CONTEXT_MENU_KEY: &str = r"HKCU\Software\Classes\*\shell\RemovaDeepUninstall";

#[tauri::command]
fn register_context_menu() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().to_string();
    // Create key tree via PowerShell-free reg write
    crate::regops::create_reg_sz(CONTEXT_MENU_KEY, "MUIVerb", "用 Remova 深度卸载")?;
    crate::regops::create_reg_sz(CONTEXT_MENU_KEY, "Icon", &format!("\"{exe}\""))?;
    let cmd_key = format!(r"{CONTEXT_MENU_KEY}\command");
    crate::regops::create_reg_sz(
        &cmd_key,
        "",
        &format!("\"{exe}\" --analyze \"%1\""),
    )?;
    Ok(())
}

#[tauri::command]
fn unregister_context_menu() -> Result<(), String> {
    crate::regops::delete_key(CONTEXT_MENU_KEY)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_installed_apps,
            app_icon_data,
            estimate_dir_size_kb,
            cancel_size_estimate,
            begin_size_estimate,
            analyze_associations,
            run_cleanup_dry_run,
            run_full_cleanup,
            restore_latest_backup,
            list_cleanup_history,
            export_history_csv,
            is_elevated,
            disk_usage,
            elevate_restart,
            list_restore_sessions,
            restore_session_by_name,
            list_startup_items,
            list_services,
            list_scheduled_tasks,
            set_startup_enabled,
            set_service_start_disabled,
            set_task_enabled,
            register_context_menu,
            unregister_context_menu
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
