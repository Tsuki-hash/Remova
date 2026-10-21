//! Locking-process diagnosis via the Windows Restart Manager (SOP §9.3).
//!
//! Diagnosis is read-only. Ending a process is a separate, explicit command
//! that re-reads the live image path and refuses anything outside the app's
//! install directory.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockingProcess {
    pub pid: u32,
    pub name: String,
    /// Full image path, empty when the process already exited or access is denied.
    #[serde(default)]
    pub path: String,
}

#[cfg(windows)]
fn image_path_for(handle: windows::Win32::Foundation::HANDLE) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{QueryFullProcessImageNameW, PROCESS_NAME_WIN32};

    let mut buf = vec![0u16; 32_768];
    let mut len = buf.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()?;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

#[cfg(windows)]
fn query_image_path(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let path = image_path_for(handle);
    unsafe {
        let _ = CloseHandle(handle);
    }
    path
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
                path: query_image_path(app.Process.dwProcessId).unwrap_or_default(),
            }
        })
        .collect())
}

/// The only processes this app may end: a live `.exe` image that sits inside
/// a real install directory (never a drive root, system root or reparse shape).
fn can_end_process(image: &str, install_root: &str) -> bool {
    let Some(img) = crate::fsutil::path_key(std::path::Path::new(image)) else {
        return false;
    };
    let Some(root) = crate::fsutil::path_key(std::path::Path::new(install_root)) else {
        return false;
    };
    if !img.ends_with(".exe") || img == root {
        return false;
    }
    for part in [&img, &root] {
        if part.split('\\').any(|seg| seg == ".." || seg == ".") {
            return false;
        }
    }
    if !crate::safety::is_safe_fs(std::path::Path::new(image))
        || !crate::safety::is_safe_fs(std::path::Path::new(install_root))
    {
        return false;
    }
    img.starts_with(&format!("{root}\\"))
}

fn same_path(a: &str, b: &str) -> bool {
    match (
        crate::fsutil::path_key(std::path::Path::new(a)),
        crate::fsutil::path_key(std::path::Path::new(b)),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// End one process after verifying the live image path. The caller must have
/// obtained user confirmation; this is never called automatically.
#[cfg(windows)]
pub fn end_locking_process(
    pid: u32,
    expected_path: &str,
    install_root: &str,
) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    if pid == 0 || pid == std::process::id() {
        return Err("refusing to end this process".into());
    }
    crate::fsutil::validate_native_name(expected_path)?;
    crate::fsutil::validate_native_name(install_root)?;
    if !can_end_process(expected_path, install_root) {
        return Err("process image is not a program under the install directory".into());
    }
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            false,
            pid,
        )
    }
    .map_err(|e| format!("OpenProcess failed: {e}"))?;
    let result = (|| {
        let current = image_path_for(handle).ok_or("cannot read process image path")?;
        if !same_path(&current, expected_path) {
            return Err("process image changed; refresh the lock list".to_string());
        }
        if !can_end_process(&current, install_root) {
            return Err("process image is outside the install directory".to_string());
        }
        unsafe { TerminateProcess(handle, 1) }.map_err(|e| format!("TerminateProcess failed: {e}"))
    })();
    unsafe {
        let _ = CloseHandle(handle);
    }
    result
}

#[cfg(not(windows))]
pub fn diagnose_locked_path(_path: &str) -> Result<Vec<LockingProcess>, String> {
    Err("not windows".into())
}

#[cfg(not(windows))]
pub fn end_locking_process(
    _pid: u32,
    _expected_path: &str,
    _install_root: &str,
) -> Result<(), String> {
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

    #[cfg(windows)]
    #[test]
    fn only_images_under_a_real_install_root_may_be_ended() {
        let root = r"C:\Apps\Demo";
        assert!(super::can_end_process(r"C:\Apps\Demo\demo.exe", root));
        assert!(super::can_end_process(r"C:\Apps\Demo\bin\demo.exe", root));
        assert!(!super::can_end_process(r"C:\Apps\DemoTool\demo.exe", root));
        assert!(!super::can_end_process(
            r"C:\Windows\System32\demo.exe",
            root
        ));
        assert!(!super::can_end_process(r"C:\Apps\Demo\demo.dll", root));
        assert!(!super::can_end_process(r"C:\Apps\Demo", root));
        assert!(!super::can_end_process(
            r"C:\Apps\Demo\..\Windows\demo.exe",
            root
        ));
        assert!(!super::can_end_process(r"C:\Apps\demo.exe", r"C:\Apps"));
        assert!(super::same_path(
            r"C:\Apps\Demo\Demo.EXE",
            r"c:/apps/demo/demo.exe"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn end_process_refuses_self_and_unrelated_images() {
        assert!(super::end_locking_process(0, r"C:\Apps\Demo\demo.exe", r"C:\Apps\Demo").is_err());
        let this = std::process::id();
        assert!(
            super::end_locking_process(this, r"C:\Apps\Demo\demo.exe", r"C:\Apps\Demo").is_err()
        );
        assert!(
            super::end_locking_process(999_999, r"C:\Other\demo.exe", r"C:\Apps\Demo").is_err()
        );
    }
}
