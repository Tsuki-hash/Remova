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
        // An unsafe immediate delete must never fall through to an unchecked
        // reboot-delete request. Keep the validated chain pinned while queuing.
        let p = std::path::Path::new(path);
        let Ok(_parents) = crate::fsutil::pin_existing_parents(p) else {
            return false;
        };
        let Ok(_target) = crate::fsutil::pin_target_readonly(p) else {
            return false;
        };
        let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(w.as_ptr()),
                PCWSTR::null(),
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
/// On failure returns a stable `elevate:<kind>:<code>` string for UI localization:
/// denied / cancelled / not_found / failed.
/// Quote one Windows command-line argument.
/// Follows CommandLineToArgvW: `\"` for embedded quotes, and trailing
/// backslashes before a closing quote are doubled.
fn quote_win_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    if !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // n backslashes + quote → 2n+1 backslashes + escaped quote.
                for _ in 0..backslashes * 2 + 1 {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            _ => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                out.push(c);
                backslashes = 0;
            }
        }
    }
    // Trailing backslashes sit against the closing quote — double them.
    for _ in 0..backslashes * 2 {
        out.push('\\');
    }
    out.push('"');
    out
}

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
        // quote each arg — `join(" ")` splits paths with spaces into extra argv.
        let params = args
            .iter()
            .map(|a| quote_win_arg(a))
            .collect::<Vec<_>>()
            .join(" ");
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
    /// Creating a restore point requires explicit local opt-in.
    fn allow_sys_mutation() -> bool {
        std::env::var_os("REMOVA_TEST_ALLOW_SYS_MUTATION").is_some()
    }

    #[test]
    fn schedule_missing_path_no_panic() {
        // A missing target is refused before MoveFileEx, with no queue write.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("remova-missing-{nonce}"));
        assert!(!path.exists());
        assert!(!super::schedule_delete_on_reboot(path.to_str().unwrap()));
    }

    #[test]
    #[ignore = "real-system side effects; opt in with REMOVA_TEST_ALLOW_SYS_MUTATION=1 and --ignored"]
    fn restore_point_nonfatal() {
        if !allow_sys_mutation() {
            panic!("REMOVA_TEST_ALLOW_SYS_MUTATION=1 is required; refusing silent pass");
        }
        // Non-fatal helper: must return a flag + message, never panic.
        let (ok, msg) = super::create_restore_point("Remova test");
        assert!(!msg.is_empty() || ok, "restore point should report status");
    }

    #[cfg(windows)]
    #[test]
    fn elevate_error_tokens() {
        assert_eq!(super::elevate_error_token(5), "elevate:denied:5");
        assert_eq!(super::elevate_error_token(1223), "elevate:cancelled:1223");
        assert_eq!(super::elevate_error_token(2), "elevate:not_found:2");
        assert_eq!(super::elevate_error_token(99), "elevate:failed:99");
    }

    #[test]
    fn quote_win_arg_spaces_and_quotes() {
        assert_eq!(super::quote_win_arg("plain"), "plain");
        assert_eq!(
            super::quote_win_arg("C:\\Program Files\\App"),
            "\"C:\\Program Files\\App\""
        );
        assert_eq!(super::quote_win_arg(""), "\"\"");
        // CommandLineToArgvW: embedded quote is `\"`, not CSV-style `""`.
        assert_eq!(super::quote_win_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    /// trailing backslash must not eat the closing quote.
    #[test]
    fn quote_win_arg_doubles_trailing_backslash() {
        // No metacharacters — pass through (trailing `\` is fine unquoted).
        assert_eq!(super::quote_win_arg(r"C:\Dir\"), r"C:\Dir\");
        assert_eq!(super::quote_win_arg("a b"), "\"a b\"");
        // Quoted + trailing backslash → double the backslashes before `"`.
        assert_eq!(
            super::quote_win_arg(r"C:\Program Files\App\"),
            "\"C:\\Program Files\\App\\\\\""
        );
        // Embedded quote with no preceding backslash → `\"`; trailing `\` doubles.
        // (The `\` before `a` is a literal path backslash, not a quote-escape.)
        assert_eq!(super::quote_win_arg("C:\\a\"b\\"), "\"C:\\a\\\"b\\\\\"");
        // Two backslashes immediately before a quote → 2*2+1 = 5, then `"`.
        assert_eq!(
            super::quote_win_arg("C:\\a\\\\\"b"),
            "\"C:\\a\\\\\\\\\\\"b\""
        );
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyRow {
    pub path: String,
    pub kind: String,
    pub still_there: bool,
    /// When set, presence is unknown; still_there stays true for older clients.
    pub error: Option<String>,
}

/// SOP §7: re-check selected paths after cleanup (checklist evidence).
pub fn verify_cleanup_leftovers(items: &[crate::scanner::CleanupItem]) -> Vec<VerifyRow> {
    verify_cleanup_leftovers_with(items, crate::regscan::target_exists)
}

fn verify_cleanup_leftovers_with(
    items: &[crate::scanner::CleanupItem],
    mut registry_exists: impl FnMut(&str) -> Result<bool, String>,
) -> Vec<VerifyRow> {
    let mut out = Vec::new();
    for it in items {
        let still = match it.kind {
            crate::scanner::ItemKind::Registry => registry_exists(&it.path),
            crate::scanner::ItemKind::Path => crate::regops::path_entry_presence(&it.path),
            _ => std::path::Path::new(&it.path)
                .try_exists()
                .map_err(|e| e.to_string()),
        };
        out.push(VerifyRow {
            path: it.path.clone(),
            kind: format!("{:?}", it.kind).to_lowercase(),
            still_there: still.as_ref().copied().unwrap_or(true),
            error: still.err(),
        });
    }
    out
}

#[cfg(test)]
mod verification_tests {
    #[test]
    fn native_registry_value_absence_does_not_inherit_parent_contents() {
        let name = format!(
            "HKCU\\Software|RemovaVerifyAbsent_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        assert!(crate::regscan::target_exists(r"HKCU\Software").unwrap());
        assert!(!crate::regscan::target_exists(&name).unwrap());
        assert!(crate::regscan::target_exists("INVALID").is_err());
        assert!(crate::regscan::target_exists("HKCU\\Software|a|b").is_err());
        assert!(crate::regscan::target_exists("HKCU\\Software|bad\0name").is_err());
    }

    #[test]
    fn registry_verification_preserves_exact_target_and_reports_unknown() {
        let paths = [
            "HKCU\\Fixture|MissingValue",
            "HKLM32\\EmptyKey",
            "HKCU\\Binary|Flag",
            "HKCU\\Denied|Value",
        ];
        let items: Vec<crate::scanner::CleanupItem> = paths
            .iter()
            .map(|path| {
                serde_json::from_value(
                    serde_json::json!({"path":path, "kind":"registry", "score":90,
                "confidence":"confirmed", "risk":"low", "reason":"fixture", "evidence":[]}),
                )
                .unwrap()
            })
            .collect();
        let mut queried = vec![];
        let rows = super::verify_cleanup_leftovers_with(&items, |path| {
            queried.push(path.to_string());
            match path {
                p if p == paths[0] => Ok(false),
                p if p == paths[3] => Err("access denied".into()),
                _ => Ok(true),
            }
        });
        assert_eq!(queried, paths);
        assert!(!rows[0].still_there);
        assert!(rows[1].still_there && rows[2].still_there);
        assert_eq!(rows[3].error.as_deref(), Some("access denied"));
        assert!(rows[3].still_there);
        assert!(rows[..3].iter().all(|row| row.error.is_none()));
    }
}

/// Elevated-relaunch handshake: the freshly spawned admin instance waits for
/// the old (non-admin) process to terminate before starting, so the
/// single-instance lock is free when Tauri initializes. Returns false on
/// timeout — the old run stayed alive (e.g. the user cancelled the busy
/// confirm) and the caller should exit quietly.
pub fn wait_for_process_exit(pid: u32, timeout_ms: u32) -> bool {
    #[cfg(not(windows))]
    {
        let _ = (pid, timeout_ms);
        true
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        unsafe {
            // A vanished old instance (fast clean exit) must proceed — an
            // unopenable PID is treated as "already gone".
            let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
                return true;
            };
            let waited = WaitForSingleObject(handle, timeout_ms);
            let _ = CloseHandle(handle);
            waited == WAIT_OBJECT_0
        }
    }
}
