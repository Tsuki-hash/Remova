//! Privileged installation key. Never repairs or replaces an existing key store.
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::Storage::FileSystem::*;
use zeroize::Zeroizing;
mod recovery;

pub(super) struct Store {
    pub key: Zeroizing<[u8; 32]>,
    root: PathBuf,
    // Keep the verified parent/store/key objects pinned through signing/staging.
    _pins: Vec<File>,
}

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(HLOCAL(self.0 .0));
        }
    }
}
fn wide(p: &Path) -> Result<Vec<u16>, String> {
    use std::os::windows::ffi::OsStrExt;
    let out: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    if out[..out.len() - 1].contains(&0) {
        return Err("seal:key_bad_path".into());
    }
    Ok(out)
}
fn descriptor(directory: bool) -> Result<Descriptor, String> {
    let sddl = if directory {
        windows::core::w!("O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
    } else {
        windows::core::w!("O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)")
    };
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, 1, &mut sd, None) }
        .map_err(|_| "seal:key_descriptor")?;
    Ok(Descriptor(sd))
}
fn sid_string(sid: PSID) -> Result<String, String> {
    let mut raw = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut raw).map_err(|_| "seal:key_sid")?;
        let text = raw.to_string().map_err(|_| "seal:key_sid");
        let _ = LocalFree(HLOCAL(raw.0.cast()));
        Ok(text?)
    }
}
fn privileged(sid: PSID) -> Result<bool, String> {
    Ok(matches!(
        sid_string(sid)?.as_str(),
        "S-1-5-18"
            | "S-1-5-32-544"
            | "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
    ))
}
/// Validate security on the pinned object, including the parent's DELETE_CHILD.
fn validate(file: &impl AsRawHandle, private: bool) -> Result<(), String> {
    validate_object(file, private, true)
}
fn validate_object(file: &impl AsRawHandle, private: bool, protected: bool) -> Result<(), String> {
    unsafe {
        let mut owner = PSID::default();
        let mut acl = std::ptr::null_mut();
        let mut raw = PSECURITY_DESCRIPTOR::default();
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut acl),
            None,
            Some(&mut raw),
        )
        .ok()
        .map_err(|_| "seal:key_security")?;
        let sd = Descriptor(raw);
        if !privileged(owner)? || acl.is_null() {
            return Err("seal:key_untrusted_owner_acl".into());
        }
        if private && !matches!(sid_string(owner)?.as_str(), "S-1-5-18" | "S-1-5-32-544") {
            return Err("seal:key_untrusted_owner_acl".into());
        }
        let mut control = 0;
        let mut revision = 0;
        GetSecurityDescriptorControl(sd.0, &mut control, &mut revision)
            .map_err(|_| "seal:key_security")?;
        if private && protected && control & SE_DACL_PROTECTED.0 == 0 {
            return Err("seal:key_inherited_acl".into());
        }
        let mut trusted_allow = false;
        for i in 0..(*acl).AceCount {
            let mut ace = std::ptr::null_mut();
            GetAce(acl, i.into(), &mut ace).map_err(|_| "seal:key_acl")?;
            let header = &*(ace as *const ACE_HEADER);
            if !private && header.AceFlags & INHERIT_ONLY_ACE.0 as u8 != 0 {
                continue;
            }
            // Standard allow/deny ACEs only; unsupported grants are not guessed.
            if header.AceType == 1 {
                continue;
            }
            if header.AceType != 0 {
                return Err("seal:key_unsupported_acl".into());
            }
            let allow = &*(ace as *const ACCESS_ALLOWED_ACE);
            let trusted = privileged(PSID(std::ptr::addr_of!(allow.SidStart).cast_mut().cast()))?;
            if trusted {
                trusted_allow = true;
            }
            // DELETE, FILE_DELETE_CHILD, WRITE_DAC, WRITE_OWNER, GENERIC_ALL.
            if !trusted && (private || allow.Mask & 0x100d_0040 != 0) {
                return Err("seal:key_weak_acl".into());
            }
        }
        if !trusted_allow {
            return Err("seal:key_no_privileged_access".into());
        }
        Ok(())
    }
}
fn pin(p: &Path, directory: bool) -> Result<File, String> {
    let w = wide(p)?;
    unsafe {
        let handle = CreateFileW(
            PCWSTR(w.as_ptr()),
            0x0002_0080 | if directory { 1 } else { 0x8000_0000 },
            if directory {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            } else {
                FILE_SHARE_READ
            },
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
        .map_err(|_| "seal:key_unavailable_requires_admin")?;
        let file = File::from_raw_handle(handle.0);
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        GetFileInformationByHandle(handle, &mut info).map_err(|_| "seal:key_attributes")?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != directory
        {
            return Err("seal:key_reparse_or_type".into());
        }
        Ok(file)
    }
}
fn mkdir(p: &Path) -> Result<(), String> {
    let sd = descriptor(true)?;
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0 .0,
        bInheritHandle: false.into(),
    };
    let w = wide(p)?;
    unsafe { CreateDirectoryW(PCWSTR(w.as_ptr()), Some(&attrs)) }
        .map_err(|_| "seal:key_create_requires_admin".into())
}
fn publish(root: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "seal:key_clock")?
        .as_nanos();
    let tmp = root.join(format!("{name}.{}.{}.tmp", std::process::id(), unique));
    let sd = descriptor(false)?;
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0 .0,
        bInheritHandle: false.into(),
    };
    let w = wide(&tmp)?;
    let result = (|| {
        let mut file = unsafe {
            let h = CreateFileW(
                PCWSTR(w.as_ptr()),
                0x4000_0000,
                FILE_SHARE_MODE(0),
                Some(&attrs),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .map_err(|_| "seal:key_create")?;
            File::from_raw_handle(h.0)
        };
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "seal:key_write")?;
        drop(file);
        // std::fs::rename replaces an existing file. Use the Win32 primitive
        // with NO replace-existing flag: publishing must never rotate a key.
        move_no_replace(&tmp, &root.join(name)).map_err(|_| "seal:key_publish")
    })();
    let _ = fs::remove_file(&tmp);
    result.map_err(String::from)
}
fn move_no_replace(from: &Path, to: &Path) -> Result<(), String> {
    let from = wide(from)?;
    let to = wide(to)?;
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(|_| "seal:key_publish".into())
}
pub(super) fn program_data() -> Result<PathBuf, String> {
    use windows::Win32::UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let raw = SHGetKnownFolderPath(&FOLDERID_ProgramData, KF_FLAG_DEFAULT, None)
            .map_err(|_| "seal:key_known_folder")?;
        let text = raw.to_string().map_err(|_| "seal:key_known_folder");
        windows::Win32::System::Com::CoTaskMemFree(Some(raw.0.cast()));
        Ok(PathBuf::from(text?))
    }
}
pub(super) fn load(initialize: bool) -> Result<Store, String> {
    #[cfg(test)]
    if let Some(root) = TEST_KEY_ROOT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        return load_at(&program_data()?, &root, &super::seals_root(), initialize);
    }
    #[cfg(test)]
    if let Some(root) = super::TEST_ROOT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        return Ok(Store {
            key: Zeroizing::new([73; 32]),
            root,
            _pins: vec![],
        });
    }
    let parent = program_data()?;
    let root = parent.join("RemovaSealKeys");
    load_at(&parent, &root, &super::seals_root(), initialize)
}

#[cfg(test)]
static TEST_KEY_ROOT: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

fn load_at(
    parent: &Path,
    root: &Path,
    public_seals: &Path,
    initialize: bool,
) -> Result<Store, String> {
    let mut parent_pins = vec![];
    for ancestor in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let parent_pin = pin(ancestor, true)?;
        validate(&parent_pin, false)?;
        if let Some((_, guard)) =
            crate::fsutil::guard_empty_directory(HANDLE(parent_pin.as_raw_handle()))
                .map_err(|_| "seal:key_parent_guard")?
        {
            parent_pins.push(guard);
        }
        parent_pins.push(parent_pin);
    }
    let created = if !root.exists() && initialize {
        // Existing v2 seals indicate lost trusted state, not a fresh installation.
        if fs::read_dir(public_seals)
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| fs::read(e.path()).is_ok_and(|b| b.starts_with(b"RSEAL2")))
        {
            return Err("seal:key_lost".into());
        }
        match mkdir(root) {
            Ok(()) => true,
            Err(e) if !root.exists() => return Err(e),
            Err(_) => false,
        }
    } else {
        false
    };
    let root_pin = pin(root, true)?;
    validate(&root_pin, true)?;
    if created {
        let mut key = Zeroizing::new([0; 32]);
        unsafe {
            windows::Win32::Security::Cryptography::BCryptGenRandom(
                None,
                key.as_mut(),
                windows::Win32::Security::Cryptography::BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        }
        .ok()
        .map_err(|_| "seal:key_rng")?;
        publish(root, "seal.key", key.as_ref())?;
    }
    // Bounded concurrent first-run wait. Missing state never triggers generation.
    let mut loaded = None;
    for attempt in 0..20 {
        match pin(&root.join("seal.key"), false) {
            Ok(f) => {
                loaded = Some(f);
                break;
            }
            Err(_) if initialize && attempt < 19 => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            Err(e) => return Err(e),
        }
    }
    let mut key_pin = loaded.ok_or("seal:key_unavailable_requires_admin")?;
    validate(&key_pin, true)?;
    if key_pin.metadata().map_err(|_| "seal:key_read")?.len() != 32 {
        return Err("seal:key_length".into());
    }
    let mut key = Zeroizing::new([0; 32]);
    key_pin
        .read_exact(key.as_mut())
        .map_err(|_| "seal:key_read")?;
    let id = super::v2::sha256(key.as_ref())?;
    let marker_path = root.join("initialized.v2");
    if !marker_path.exists() && initialize {
        if let Err(e) = publish(root, "initialized.v2", id.as_bytes()) {
            if !marker_path.exists() {
                return Err(e);
            }
        }
    }
    let mut marker = pin(&marker_path, false)?;
    validate(&marker, true)?;
    if marker.metadata().map_err(|_| "seal:key_marker")?.len() != 64 {
        return Err("seal:key_marker".into());
    }
    let mut recorded = [0; 64];
    marker
        .read_exact(&mut recorded)
        .map_err(|_| "seal:key_marker")?;
    if recorded != id.as_bytes() {
        return Err("seal:key_marker_mismatch".into());
    }
    parent_pins.extend([root_pin, key_pin, marker]);
    Ok(Store {
        key,
        root: root.to_path_buf(),
        _pins: parent_pins,
    })
}

pub(super) struct Stage {
    pub path: PathBuf,
    _store: Store,
    pin: Option<File>,
}
impl Store {
    pub fn stage(self) -> Result<Stage, String> {
        self.cleanup_abandoned();
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "seal:key_clock")?
            .as_nanos();
        let path = self
            .root
            .join(format!("stage_{}_{}", std::process::id(), n));
        #[cfg(test)]
        if self._pins.is_empty() {
            fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            return Ok(Stage {
                path,
                _store: self,
                pin: None,
            });
        }
        mkdir(&path)?;
        let pin = pin(&path, true)?;
        validate(&pin, true)?;
        Ok(Stage {
            path,
            _store: self,
            pin: Some(pin),
        })
    }
}
impl Stage {
    pub fn verify_snapshots(&self) -> Result<(), String> {
        #[cfg(test)]
        if self._store._pins.is_empty() {
            return Ok(());
        }
        fn walk(path: &Path) -> Result<(), String> {
            let meta = fs::symlink_metadata(path).map_err(|_| "seal:stage_metadata")?;
            let file = pin(path, meta.is_dir())?;
            // Children can inherit this privileged-only ACL, but their owner
            // must still be SYSTEM/Administrators, never the standard-user SID.
            validate_object(&file, true, false)?;
            if meta.is_dir() {
                for entry in fs::read_dir(path).map_err(|_| "seal:stage_read")? {
                    walk(&entry.map_err(|_| "seal:stage_read")?.path())?;
                }
            }
            Ok(())
        }
        walk(&self.path)
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        self.pin.take();
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_publish_never_replaces_an_existing_key() {
        let root = std::env::temp_dir().join(format!("remova_publish_{}", std::process::id()));
        let pins = crate::fsutil::create_dirs_pinned(&root).unwrap();
        let source = root.join("new");
        let target = root.join("key");
        fs::write(&source, b"new key").unwrap();
        fs::write(&target, b"existing key").unwrap();
        assert!(move_no_replace(&source, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"existing key");
        assert_eq!(fs::read(&source).unwrap(), b"new key");
        fs::remove_file(&target).unwrap();
        move_no_replace(&source, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new key");
        assert!(!source.exists());
        drop(pins);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn user_owned_directory_is_never_promoted_to_trusted_store() {
        let root = std::env::temp_dir().join(format!("remova_acl_reject_{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let file = pin(&root, true).unwrap();
        assert!(validate(&file, true).is_err());
        drop(file);
        fs::remove_dir(&root).unwrap();
    }

    #[test]
    #[ignore = "requires elevated Windows token; creates an isolated ProgramData ACL fixture"]
    fn privileged_key_store_acl_and_missing_state_regression() {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let parent = program_data().unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = parent.join(format!(
            "RemovaSealKeys_test_{}_{}",
            std::process::id(),
            nonce
        ));
        let public = std::env::temp_dir().join(format!("remova_key_public_{nonce}"));
        fs::create_dir(&public).unwrap();
        let store =
            load_at(&parent, &root, &public, true).expect("elevated key initialization required");
        let id = super::super::v2::sha256(store.key.as_ref()).unwrap();
        let _seal_store = super::super::TestStore::new(public.join("seals"));
        *TEST_KEY_ROOT.lock().unwrap() = Some(root.clone());
        struct ClearKeyOverride;
        impl Drop for ClearKeyOverride {
            fn drop(&mut self) {
                *TEST_KEY_ROOT.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }
        let _key_override = ClearKeyOverride;
        // Execute the production backup/restore chain with this actual ACL key,
        // not the synthetic protocol key used by default permission-free tests.
        let source = public.join("source/file.txt");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"privileged native backup").unwrap();
        let session = public.join("1700000000_native");
        fs::create_dir_all(session.join("files")).unwrap();
        fs::create_dir_all(session.join("registry")).unwrap();
        let item = crate::scanner::CleanupItem {
            path: source.to_string_lossy().into_owned(),
            kind: crate::scanner::ItemKind::File,
            score: 90,
            confidence: crate::scanner::Confidence::Confirmed,
            risk: crate::scanner::RiskLevel::Low,
            reason: "test".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        };
        let (ok, fail, errors) = crate::backup::backup_items(&[item], &session);
        assert_eq!((ok, fail), (1, 0), "{errors:?}");
        fs::write(&source, b"changed").unwrap();
        crate::restore::restore_session(&session).unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"privileged native backup");
        fs::write(&source, b"standard token must not restore").unwrap();
        publish(&root, "publish_sentinel", b"first").unwrap();
        assert!(publish(&root, "publish_sentinel", b"second").is_err());
        assert_eq!(fs::read(root.join("publish_sentinel")).unwrap(), b"first");
        let reused = load_at(&parent, &root, &public, true).unwrap();
        assert_eq!(id, super::super::v2::sha256(reused.key.as_ref()).unwrap());
        drop(reused);
        // Private staging can actually be enumerated/copied while the store pins
        // are held, and its children inherit privileged-only access.
        let stage = load_at(&parent, &root, &public, false)
            .unwrap()
            .stage()
            .unwrap();
        fs::write(stage.path.join("snapshot"), b"trusted").unwrap();
        stage.verify_snapshots().unwrap();
        crate::fsutil::copy_dir(&stage.path, &public.join("published")).unwrap();
        drop(stage);
        let mut token = HANDLE::default();
        let mut restricted = HANDLE::default();
        let mut admin_sid = PSID::default();
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_IMPERSONATE,
                &mut token,
            )
            .unwrap();
            ConvertStringSidToSidW(windows::core::w!("S-1-5-32-544"), &mut admin_sid).unwrap();
            let disable = [SID_AND_ATTRIBUTES {
                Sid: admin_sid,
                Attributes: 0,
            }];
            CreateRestrictedToken(
                token,
                DISABLE_MAX_PRIVILEGE,
                Some(&disable),
                None,
                None,
                &mut restricted,
            )
            .unwrap();
            let _ = LocalFree(HLOCAL(admin_sid.0));
        }
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                unsafe {
                    RevertToSelf().expect("must revert impersonation");
                }
            }
        }
        drop(store); // Exercise ACLs without our handles blocking replacement.
        unsafe {
            ImpersonateLoggedOnUser(restricted).unwrap();
        }
        let revert = Revert;
        // Same user, admin SID disabled and privileges stripped. Check ACLs
        // directly, rather than relying on the elevated process's open pins.
        assert!(fs::read(root.join("seal.key")).is_err());
        assert!(crate::restore::restore_session(&session).is_err());
        assert_eq!(
            fs::read(&source).unwrap(),
            b"standard token must not restore"
        );
        assert!(fs::write(root.join("seal.key"), [0; 32]).is_err());
        assert!(fs::remove_file(root.join("seal.key")).is_err());
        assert!(fs::rename(&root, parent.join(format!("remova_replaced_{nonce}"))).is_err());
        let w = wide(&root).unwrap();
        let security_handle = unsafe {
            CreateFileW(
                PCWSTR(w.as_ptr()),
                0x000c_0000,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        };
        assert!(
            security_handle.is_err(),
            "restricted user must not obtain WRITE_DAC/WRITE_OWNER"
        );
        drop(revert);
        unsafe {
            let _ = CloseHandle(restricted);
            let _ = CloseHandle(token);
        }
        let store = load_at(&parent, &root, &public, false).unwrap();
        assert_eq!(id, super::super::v2::sha256(store.key.as_ref()).unwrap());
        drop(store);
        fs::remove_file(root.join("initialized.v2")).unwrap();
        let recovered = load_at(&parent, &root, &public, true).unwrap();
        assert_eq!(
            id,
            super::super::v2::sha256(recovered.key.as_ref()).unwrap()
        );
        drop(recovered);
        fs::remove_file(root.join("seal.key")).unwrap();
        assert!(load_at(&parent, &root, &public, true).is_err());
        assert!(
            !root.join("seal.key").exists(),
            "lost key must never regenerate"
        );
        let race_root = parent.join(format!("RemovaSealKeys_race_{nonce}"));
        let ids = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                let store = load_at(&parent, &race_root, &public, true).unwrap();
                super::super::v2::sha256(store.key.as_ref()).unwrap()
            });
            let second = scope.spawn(|| {
                let store = load_at(&parent, &race_root, &public, true).unwrap();
                super::super::v2::sha256(store.key.as_ref()).unwrap()
            });
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(
            ids.0, ids.1,
            "concurrent initialization must choose one key"
        );
        fs::remove_dir_all(&race_root).unwrap();
        let prebuilt = parent.join(format!("RemovaSealKeys_prebuilt_{nonce}"));
        fs::create_dir(&prebuilt).unwrap();
        fs::write(prebuilt.join("seal.key"), [42; 32]).unwrap();
        assert!(load_at(&parent, &prebuilt, &public, true).is_err());
        assert_eq!(
            fs::read(prebuilt.join("seal.key")).unwrap(),
            [42; 32],
            "never repair/re-sign an attacker-known key with weak inherited permissions"
        );
        fs::remove_dir_all(&prebuilt).unwrap();
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&public).unwrap();
    }
}
