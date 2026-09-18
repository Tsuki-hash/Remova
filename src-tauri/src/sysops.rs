//! Windows system helpers: reboot-delete, restore point, elevate relaunch.

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

/// Schedule path deletion on next reboot (locked files).
pub fn schedule_delete_on_reboot(path: &str) -> bool {
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
    #[cfg(windows)]
    {
        let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let null: Vec<u16> = vec![0];
        unsafe {
            MoveFileExW(
                PCWSTR(w.as_ptr()),
                PCWSTR(null.as_ptr()),
                MOVEFILE_DELAY_UNTIL_REBOOT,
            )
            .is_ok()
        }
    }
}

/// Best-effort system restore point. Returns (ok, message).
pub fn create_restore_point(description: &str) -> (bool, String) {
    #[cfg(not(windows))]
    {
        let _ = description;
        (false, "not windows".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Restore::{
            SRSetRestorePointW, APPLICATION_INSTALL, BEGIN_SYSTEM_CHANGE, RESTOREPOINTINFOW,
            RESTOREPOINTINFO_TYPE, STATEMGRSTATUS,
        };
        let desc: Vec<u16> = description
            .chars()
            .take(63)
            .collect::<String>()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let mut sz = [0u16; 256];
            let n = desc.len().min(255);
            sz[..n].copy_from_slice(&desc[..n]);
            let rp = RESTOREPOINTINFOW {
                dwEventType: BEGIN_SYSTEM_CHANGE,
                dwRestorePtType: APPLICATION_INSTALL,
                llSequenceNumber: 0,
                szDescription: sz,
            };
            let mut sm = STATEMGRSTATUS {
                nStatus: windows::Win32::Foundation::WIN32_ERROR(0),
                llSequenceNumber: 0,
            };
            let _ = RESTOREPOINTINFO_TYPE(0);
            let ok = SRSetRestorePointW(&rp, &mut sm);
            if ok.as_bool() {
                let seq = sm.llSequenceNumber;
                (true, format!("restore point seq={seq}"))
            } else {
                (false, "SRSetRestorePointW failed".into())
            }
        }
    }
}

/// Relaunch current exe elevated via ShellExecuteW "runas".
///
/// On failure returns a stable `elevate:<kind>:<code>` string for UI localization:
/// denied / cancelled / not_found / failed.
pub fn elevate_relaunch(args: &[String]) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = args;
        Err("elevate:failed:0".into())
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let exe = std::env::current_exe().map_err(|_| "elevate:not_found:2".to_string())?;
        let exe_w: Vec<u16> = exe
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let verb: Vec<u16> = "runas\0".encode_utf16().collect();
        let params = args.join(" ");
        let params_w: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();
        let empty: Vec<u16> = vec![0];
        unsafe {
            let rc = ShellExecuteW(
                None,
                PCWSTR(verb.as_ptr()),
                PCWSTR(exe_w.as_ptr()),
                if params.is_empty() {
                    PCWSTR::null()
                } else {
                    PCWSTR(params_w.as_ptr())
                },
                PCWSTR(empty.as_ptr()),
                SW_SHOWNORMAL,
            );
            // HINSTANCE > 32 means success
            if (rc.0 as isize) > 32 {
                Ok(())
            } else {
                Err(elevate_error_token(rc.0 as isize))
            }
        }
    }
}

/// Map ShellExecuteW failure codes to stable tokens for the UI.
#[cfg(windows)]
fn elevate_error_token(code: isize) -> String {
    // ShellExecute SE_ERR_* values (and ERROR_CANCELLED from UAC).
    let kind = match code {
        2 | 3 => "not_found",
        5 => "denied",
        1223 => "cancelled",
        _ => "failed",
    };
    format!("elevate:{kind}:{code}")
}

#[cfg(test)]
mod tests {
    /// These helpers mutate real system state (PendingFileRenameOperations / restore
    /// point). Default suite skips the call so CI and `cargo test` stay side-effect free
    /// (NEW-D). Opt in locally with REMOVA_TEST_ALLOW_SYS_MUTATION=1.
    fn allow_sys_mutation() -> bool {
        std::env::var_os("REMOVA_TEST_ALLOW_SYS_MUTATION").is_some()
    }

    #[test]
    fn schedule_missing_path_no_panic() {
        if !allow_sys_mutation() {
            return;
        }
        let _ = super::schedule_delete_on_reboot(r"C:\remova_no_such_file_xyz");
    }

    #[test]
    fn restore_point_nonfatal() {
        if !allow_sys_mutation() {
            return;
        }
        let _ = super::create_restore_point("Remova test");
    }

    #[cfg(windows)]
    #[test]
    fn elevate_error_tokens() {
        assert_eq!(super::elevate_error_token(5), "elevate:denied:5");
        assert_eq!(super::elevate_error_token(1223), "elevate:cancelled:1223");
        assert_eq!(super::elevate_error_token(2), "elevate:not_found:2");
        assert_eq!(super::elevate_error_token(99), "elevate:failed:99");
    }
}

const CONTEXT_MENU_KEY: &str = r"HKCU\Software\Classes\*\shell\RemovaDeepUninstall";

/// Register Explorer right-click “Remova Deep Uninstall”.
pub fn register_context_menu() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().to_string();
    crate::regops::create_reg_sz(CONTEXT_MENU_KEY, "MUIVerb", "Remova Deep Uninstall")?;
    crate::regops::create_reg_sz(CONTEXT_MENU_KEY, "Icon", &format!("\"{exe}\""))?;
    let cmd_key = format!(r"{CONTEXT_MENU_KEY}\command");
    crate::regops::create_reg_sz(&cmd_key, "", &format!("\"{exe}\" --analyze \"%1\""))?;
    Ok(())
}

pub fn unregister_context_menu() -> Result<(), String> {
    crate::regops::delete_key(CONTEXT_MENU_KEY)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyRow {
    pub path: String,
    pub kind: String,
    pub still_there: bool,
}

/// SOP §7: re-check selected paths after cleanup (checklist evidence).
pub fn verify_cleanup_leftovers(items: &[crate::scanner::CleanupItem]) -> Vec<VerifyRow> {
    let mut out = Vec::new();
    for it in items {
        let still = match it.kind {
            crate::scanner::ItemKind::Registry => {
                let path = it.path.split('|').next().unwrap_or(&it.path);
                crate::regscan::list_subkeys(path).len() + crate::regscan::list_values(path).len()
                    > 0
            }
            crate::scanner::ItemKind::Path => crate::regops::scrub_path_entry_ok(&it.path),
            _ => std::path::Path::new(&it.path).exists(),
        };
        out.push(VerifyRow {
            path: it.path.clone(),
            kind: format!("{:?}", it.kind).to_lowercase(),
            still_there: still,
        });
    }
    out
}
