//! Bind live cleanup targets to their authenticated registry backups.

use super::*;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_KEY_DELETED, HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Registry::{
    RegNotifyChangeKeyValue, RegQueryInfoKeyW, KEY_NOTIFY, KEY_QUERY_VALUE,
    REG_NOTIFY_CHANGE_ATTRIBUTES, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    REG_NOTIFY_CHANGE_SECURITY, REG_NOTIFY_THREAD_AGNOSTIC,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

struct OpenKey(HKEY);

struct Location {
    root: HKEY,
    sub: String,
    view: REG_SAM_FLAGS,
}

impl Location {
    fn open(&self, rights: REG_SAM_FLAGS) -> Result<Option<OpenKey>, String> {
        if self.sub.is_empty() || self.sub.ends_with('\\') {
            return Err("registry key leaf must not be empty".into());
        }
        let wide = to_wide_name(&self.sub)?;
        let mut key = HKEY::default();
        let status = unsafe {
            RegOpenKeyExW(
                self.root,
                PCWSTR(wide.as_ptr()),
                0,
                rights | self.view,
                &mut key,
            )
        };
        match status {
            ERROR_SUCCESS => Ok(Some(OpenKey(key))),
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
            _ => Err(format!("cannot open registry source (Win32 {})", status.0)),
        }
    }
}

impl OpenKey {
    fn exists(&self) -> Result<bool, String> {
        let status = unsafe {
            RegQueryInfoKeyW(
                self.0,
                PWSTR::null(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
        };
        match status {
            ERROR_SUCCESS => Ok(true),
            ERROR_KEY_DELETED => Ok(false),
            _ => Err(format!("cannot check registry source (Win32 {})", status.0)),
        }
    }
}

impl Drop for OpenKey {
    fn drop(&mut self) {
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

struct ChangeEvent(HANDLE);

impl ChangeEvent {
    fn watch(key: HKEY) -> Result<Self, String> {
        let event = Self(
            unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
                .map_err(|error| error.to_string())?,
        );
        let filter = REG_NOTIFY_CHANGE_NAME
            | REG_NOTIFY_CHANGE_LAST_SET
            | REG_NOTIFY_CHANGE_ATTRIBUTES
            | REG_NOTIFY_CHANGE_SECURITY
            | REG_NOTIFY_THREAD_AGNOSTIC;
        unsafe { RegNotifyChangeKeyValue(key, true, filter, event.0, true) }
            .ok()
            .map_err(|error| error.to_string())?;
        Ok(event)
    }

    fn unchanged(&self) -> bool {
        // The event is one-shot. A change anywhere in the subtree permanently
        // invalidates this receipt, including changes reverted before cleanup.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
    }
}

impl Drop for ChangeEvent {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

enum Baseline {
    Tree(ChangeEvent),
    Value {
        name: String,
        raw_digest: String,
        export_digest: String,
        typ: u32,
    },
}

pub(crate) struct RegistrySource {
    key: OpenKey,
    location: Location,
    baseline: Baseline,
}

impl RegistrySource {
    #[cfg(not(test))]
    pub(crate) fn capture(target: &str) -> Result<Option<Self>, String> {
        let (path, name) = if target.contains('|') {
            let (path, name) = split_value_path(target).ok_or("invalid registry value path")?;
            validate_value_export(path, name)?;
            (path, Some(name))
        } else {
            (target, None)
        };
        let (root, sub, view) = parse(path).ok_or("invalid registry path")?;
        Self::capture_at(Location { root, sub, view }, path, name)
    }

    fn capture_at(
        location: Location,
        path: &str,
        name: Option<&str>,
    ) -> Result<Option<Self>, String> {
        let rights = KEY_QUERY_VALUE
            | if name.is_some() {
                KEY_SET_VALUE
            } else {
                KEY_NOTIFY
            };
        let Some(key) = location.open(rights)? else {
            return Ok(None);
        };
        let baseline = if let Some(name) = name {
            let Some((bytes, typ)) = read_open_reg_value(key.0, name)? else {
                return Ok(None);
            };
            let export = encode_reg_value(path, name, &bytes, typ)?;
            Baseline::Value {
                name: name.into(),
                raw_digest: crate::path_seal::publication_reader_digest(bytes.as_slice())?,
                export_digest: crate::path_seal::publication_reader_digest(export.as_slice())?,
                typ,
            }
        } else {
            Baseline::Tree(ChangeEvent::watch(key.0)?)
        };
        Ok(Some(Self {
            key,
            location,
            baseline,
        }))
    }

    pub(crate) fn matches_backup(&self, digest: &str) -> bool {
        match &self.baseline {
            Baseline::Tree(event) => event.unchanged(),
            Baseline::Value { export_digest, .. } => export_digest == digest,
        }
    }

    /// true means the live source can still be removed, false means it is gone.
    pub(crate) fn verify(&self) -> Result<bool, String> {
        if !self.key.exists()? {
            return if self.location.open(KEY_QUERY_VALUE)?.is_none() {
                Ok(false)
            } else {
                Err("registry key was replaced".into())
            };
        }
        match &self.baseline {
            Baseline::Tree(event) if event.unchanged() => Ok(true),
            Baseline::Tree(_) => Err("registry key or subtree changed".into()),
            Baseline::Value {
                name,
                raw_digest,
                typ,
                ..
            } => match read_open_reg_value(self.key.0, name)? {
                None => Ok(false),
                Some((current, kind)) => {
                    if kind == *typ
                        && crate::path_seal::publication_reader_digest(current.as_slice())?
                            == *raw_digest
                    {
                        Ok(true)
                    } else {
                        Err("registry value changed".into())
                    }
                }
            },
        }
    }

    pub(crate) fn delete_value(&self, target: &str) -> Option<Result<bool, String>> {
        let Baseline::Value { name, .. } = &self.baseline else {
            return None;
        };
        Some((|| {
            crate::safety::is_safe_to_delete_registry(target)
                .map_err(|error| format!("registry delete blocked: {error}"))?;
            let wide = to_wide_name(name)?;
            let status = unsafe { RegDeleteValueW(self.key.0, PCWSTR(wide.as_ptr())) };
            match status {
                ERROR_SUCCESS => Ok(true),
                ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_KEY_DELETED => Ok(false),
                _ => Err(format!("registry delete failed (Win32 {})", status.0)),
            }
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;
    use windows::Win32::System::Registry::{
        RegCreateKeyExW, RegDeleteTreeW, RegLoadAppKeyW, RegSetValueExW, KEY_ALL_ACCESS,
        REG_BINARY, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_PROCESS_APPKEY, REG_VALUE_TYPE,
    };

    fn app_hive(directory: &TempDir) -> OpenKey {
        let path = directory.path().join("fixture.hive");
        let wide = to_wide_name(path.to_str().unwrap()).unwrap();
        let mut key = HKEY::default();
        let status = unsafe {
            RegLoadAppKeyW(
                PCWSTR(wide.as_ptr()),
                &mut key,
                KEY_ALL_ACCESS.0,
                REG_PROCESS_APPKEY,
                0,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        OpenKey(key)
    }

    fn create(root: HKEY, sub: &str) -> OpenKey {
        let wide = to_wide_name(sub).unwrap();
        let mut key = HKEY::default();
        let status = unsafe {
            RegCreateKeyExW(
                root,
                PCWSTR(wide.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut key,
                None,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        OpenKey(key)
    }

    fn write(key: HKEY, name: &str, typ: REG_VALUE_TYPE, data: &[u8]) {
        let wide = to_wide_name(name).unwrap();
        assert_eq!(
            unsafe { RegSetValueExW(key, PCWSTR(wide.as_ptr()), 0, typ, Some(data)) },
            ERROR_SUCCESS
        );
    }

    #[test]
    fn backed_registry_checks_subtree_changes_and_same_name_recreation() {
        let temp = TempDir::new("registry-tree-receipt");
        let hive = app_hive(&temp);
        let root = create(hive.0, "Software\\Vendor");
        let capture = || {
            RegistrySource::capture_at(
                Location {
                    root: hive.0,
                    sub: "Software\\Vendor".into(),
                    view: REG_SAM_FLAGS(0),
                },
                "HKCU\\Software\\Vendor",
                None,
            )
            .unwrap()
            .unwrap()
        };
        let receipt = capture();
        assert!(receipt.matches_backup("authenticated tree export"));
        assert_eq!(receipt.verify(), Ok(true));
        drop(create(root.0, "LateChild"));
        assert!(receipt.verify().unwrap_err().contains("subtree changed"));
        let receipt = capture();
        let child = create(root.0, "LateChild");
        write(child.0, "setting", REG_BINARY, b"new");
        assert!(receipt.verify().unwrap_err().contains("subtree changed"));
        let receipt = capture();
        let wide = to_wide_name("Software\\Vendor").unwrap();
        assert_eq!(
            unsafe { RegDeleteTreeW(hive.0, PCWSTR(wide.as_ptr())) },
            ERROR_SUCCESS
        );
        assert_eq!(receipt.verify(), Ok(false));
        drop(create(hive.0, "Software\\Vendor"));
        assert!(receipt.verify().unwrap_err().contains("replaced"));
    }

    #[test]
    fn backed_registry_value_checks_raw_type_and_bytes_without_freezing_siblings() {
        let temp = TempDir::new("registry-value-receipt");
        let hive = app_hive(&temp);
        let root = create(hive.0, "Software\\Vendor");
        let name = "Setting";
        let path = r"HKCU\Software\Vendor";
        let data = [0, 1, 2, 3];
        write(root.0, name, REG_BINARY, &data);
        let receipt = RegistrySource::capture_at(
            Location {
                root: hive.0,
                sub: "Software\\Vendor".into(),
                view: REG_SAM_FLAGS(0),
            },
            path,
            Some(name),
        )
        .unwrap()
        .unwrap();
        let export = encode_reg_value(path, name, &data, REG_BINARY.0).unwrap();
        assert!(receipt.matches_backup(
            &crate::path_seal::publication_reader_digest(export.as_slice()).unwrap()
        ));
        assert!(!receipt.matches_backup("different export"));
        write(root.0, "Other", REG_BINARY, b"updated");
        assert_eq!(receipt.verify(), Ok(true));
        write(root.0, name, REG_DWORD, &data);
        assert!(receipt.verify().unwrap_err().contains("value changed"));
        write(root.0, name, REG_BINARY, b"changed");
        assert!(receipt.verify().unwrap_err().contains("value changed"));
        let wide = to_wide_name(name).unwrap();
        assert_eq!(
            unsafe { RegDeleteValueW(root.0, PCWSTR(wide.as_ptr())) },
            ERROR_SUCCESS
        );
        assert_eq!(receipt.verify(), Ok(false));
        write(root.0, name, REG_BINARY, &data);
        assert_eq!(receipt.verify(), Ok(true));
        assert_eq!(
            receipt.delete_value(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run|Setting"),
            Some(Ok(true))
        );
        assert_eq!(receipt.verify(), Ok(false));
        assert!(read_open_reg_value(root.0, "Other").unwrap().is_some());
    }
}
