//! Locking-process diagnosis via the Windows Restart Manager (SOP §9.3).
//!
//! Read-only: it names the processes preventing a delete so the user can close
//! them. It never terminates anything.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockingProcess {
    pub pid: u32,
    pub name: String,
}

#[cfg(windows)]
pub fn diagnose_locked_path(path: &str) -> Result<Vec<LockingProcess>, String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
        RM_PROCESS_INFO,
    };

    if path.trim().is_empty() {
        return Err("empty path".into());
    }
    crate::fsutil::validate_native_name(path)?;
    let wide = crate::fsutil::to_wide(path);
    let mut handle = 0u32;
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    let started = unsafe { RmStartSession(&mut handle, 0, PWSTR(key.as_mut_ptr())) };
    if started != ERROR_SUCCESS {
        return Err(format!("RmStartSession failed: {}", started.0));
    }
    let files = [windows::core::PCWSTR(wide.as_ptr())];
    let registered = unsafe { RmRegisterResources(handle, Some(&files), None, None) };
    if registered != ERROR_SUCCESS {
        unsafe {
            let _ = RmEndSession(handle);
        }
        return Err(format!("RmRegisterResources failed: {}", registered.0));
    }
    let mut needed = 0u32;
    let mut count = 0u32;
    let mut reasons = 0u32;
    let probe = unsafe { RmGetList(handle, &mut needed, &mut count, None, &mut reasons) };
    if probe != ERROR_MORE_DATA && probe != ERROR_SUCCESS {
        unsafe {
            let _ = RmEndSession(handle);
        }
        return Err(format!("RmGetList failed: {}", probe.0));
    }
    let mut apps: Vec<RM_PROCESS_INFO> = vec![RM_PROCESS_INFO::default(); needed as usize];
    let status = unsafe {
        RmGetList(
            handle,
            &mut needed,
            &mut count,
            Some(apps.as_mut_ptr()),
            &mut reasons,
        )
    };
    unsafe {
        let _ = RmEndSession(handle);
    }
    if status != ERROR_SUCCESS {
        return Err(format!("RmGetList failed: {}", status.0));
    }
    apps.truncate(count.min(needed) as usize);
    Ok(apps
        .into_iter()
        .map(|app| {
            let end = app
                .strAppName
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(app.strAppName.len());
            LockingProcess {
                pid: app.Process.dwProcessId,
                name: String::from_utf16_lossy(&app.strAppName[..end]),
            }
        })
        .collect())
}

#[cfg(not(windows))]
pub fn diagnose_locked_path(_path: &str) -> Result<Vec<LockingProcess>, String> {
    Err("not windows".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_path_is_rejected() {
        assert!(super::diagnose_locked_path("  ").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn missing_path_has_no_locking_processes() {
        let result = super::diagnose_locked_path(r"C:\RemovaNoSuchFile_xyz\missing.bin");
        assert!(result.is_ok(), "{result:?}");
        assert!(result.unwrap().is_empty());
    }
}
