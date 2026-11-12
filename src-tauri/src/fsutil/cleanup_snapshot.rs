use super::{DirPin, PinnedLocalDeleteTree};
use crate::path_seal::FileManifest;
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use windows::Win32::Storage::FileSystem::{
    FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    BY_HANDLE_FILE_INFORMATION, FILE_ID_INFO,
};

type ObjectIdentity = (u64, [u8; 16]);

pub(crate) struct CleanupTreeIdentity(BTreeMap<PathBuf, ObjectIdentity>);

pub(crate) fn remove_verified_backup_session(
    path: &Path,
    verify: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    remove_verified_backup_session_with(path, verify, || {})
}

fn remove_verified_backup_session_with(
    path: &Path,
    verify: impl FnOnce() -> Result<(), String>,
    before_reopen: impl FnOnce(),
) -> Result<(), String> {
    let pins = super::pin_local_scan_path(path).map_err(|error| error.to_string())?;
    let root = pins.last().ok_or("backup session has no root")?;
    let id = object_identity(root).map_err(|error| error.to_string())?;
    verify()?;
    drop(pins);
    before_reopen();
    let tree =
        PinnedLocalDeleteTree::with_entry_limit(path, crate::backup::SECURE_TREE_BUDGET as usize)
            .map_err(|error| error.to_string())?;
    if !tree.is_dir() || object_identity(tree.root_pin()).map_err(|error| error.to_string())? != id
    {
        return Err("backup session was replaced during verification; files were kept".into());
    }
    tree.remove_backed_up().map_err(|error| error.to_string())
}

pub(crate) fn remove_empty_backup_session(path: &Path) -> io::Result<()> {
    let tree = PinnedLocalDeleteTree::with_entry_limit(path, 3)?;
    // Bind deletion to the observed shell. A late payload cannot enter the
    // captured handles or be removed as part of an empty-directory disposition.
    if tree.objects.iter().any(|(entry, is_dir, _)| {
        !is_dir
            || (entry != path
                && !(entry.parent() == Some(path)
                    && entry
                        .file_name()
                        .is_some_and(|name| name == "files" || name == "registry")))
    }) {
        return Err(io::Error::other("unsealed backup is no longer empty"));
    }
    tree.remove_backed_up()
}

pub(crate) fn object_identity(object: &impl AsRawHandle) -> io::Result<ObjectIdentity> {
    let mut info = FILE_ID_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            windows::Win32::Foundation::HANDLE(object.as_raw_handle()),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .map_err(io::Error::other)?;
    if info.FileId.Identifier == [0; 16] {
        return Err(io::Error::other("volume did not report a file identity"));
    }
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}

fn manifest_key(root: &Path, path: &Path, root_key: &str) -> io::Result<String> {
    let suffix = path
        .strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .ok_or_else(|| io::Error::other("invalid backup payload path"))?;
    Ok(if suffix.is_empty() {
        root_key.to_owned()
    } else {
        format!("{root_key}/{}", suffix.replace('\\', "/"))
    })
}

fn verify_manifest_entry(
    path: &Path,
    directory: bool,
    pin: &DirPin,
    manifest: &FileManifest,
    key: &str,
) -> io::Result<()> {
    let digest = if directory {
        None
    } else {
        let file = std::mem::ManuallyDrop::new(unsafe { File::from_raw_handle(pin.handle.0) });
        Some(crate::path_seal::publication_reader_digest(&*file).map_err(io::Error::other)?)
    };
    if manifest.get(key) != Some(&digest) {
        return Err(io::Error::other(format!(
            "{}: contents changed or were not backed up",
            path.display()
        )));
    }
    Ok(())
}

impl CleanupTreeIdentity {
    pub(crate) fn capture(path: &Path, limit: usize) -> io::Result<Self> {
        let tree = PinnedLocalDeleteTree::collect_with(path, limit, 0x81, &mut |_| {
            if crate::executor::cleanup_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "cleanup cancelled",
                ));
            }
            Ok(())
        })?;
        let identities = tree
            .objects
            .iter()
            .map(|(path, _, pin)| Ok((path.clone(), object_identity(pin)?)))
            .collect::<io::Result<_>>()?;
        Ok(Self(identities))
    }
}

impl PinnedLocalDeleteTree {
    pub(crate) fn verify_cleanup_backup(
        &self,
        identities: &CleanupTreeIdentity,
        manifest: &FileManifest,
        root_key: &str,
    ) -> io::Result<()> {
        for (path, directory, pin) in &self.objects {
            if crate::executor::cleanup_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "cleanup cancelled",
                ));
            }
            let key = manifest_key(&self.path, path, root_key)?;
            let current = object_identity(pin)?;
            if identities.0.get(path) != Some(&current) {
                return Err(io::Error::other(format!(
                    "{}: object changed",
                    path.display()
                )));
            }
            verify_manifest_entry(path, *directory, pin, manifest, &key)?;
        }
        Ok(())
    }

    /// Hold the authenticated copy through source removal. Relative opens deny
    /// ordinary write and delete sharing, and never follow a reparse point.
    pub(crate) fn hold_backup_payload(
        path: &Path,
        manifest: &FileManifest,
        root_key: &str,
    ) -> io::Result<Self> {
        let prefix = format!("{root_key}/");
        let descendants = manifest
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .count();
        let count = 1 + descendants;
        let tree = Self::collect_with(path, count, 0x81, &mut |_| {
            if crate::executor::cleanup_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "cleanup cancelled",
                ));
            }
            Ok(())
        })?;
        if tree.objects.len() != count {
            return Err(io::Error::other(
                "backup payload is missing recorded entries",
            ));
        }
        for (path, directory, pin) in &tree.objects {
            if !directory {
                let mut info = BY_HANDLE_FILE_INFORMATION::default();
                unsafe { GetFileInformationByHandle(pin.handle, &mut info) }
                    .map_err(io::Error::other)?;
                if info.nNumberOfLinks != 1 {
                    return Err(io::Error::other(format!(
                        "{}: backup file has multiple links",
                        path.display()
                    )));
                }
            }
            pin.check_data_streams()?;
            let key = manifest_key(&tree.path, path, root_key)?;
            verify_manifest_entry(path, *directory, pin, manifest, &key)?;
            pin.check_data_streams()?;
        }
        Ok(tree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_session_removal_keeps_a_same_name_replacement_after_verification() {
        let fixture = crate::test_support::TempDir::new("backup-delete-identity");
        let session = fixture.subdir("session");
        let alternate = fixture.subdir("replacement");
        let original = fixture.join("original");
        std::fs::write(session.join("known.bin"), b"original").unwrap();
        std::fs::write(alternate.join("new.bin"), b"new data").unwrap();
        let error = remove_verified_backup_session_with(
            &session,
            || Ok(()),
            || {
                std::fs::rename(&session, &original).unwrap();
                std::fs::rename(&alternate, &session).unwrap();
            },
        )
        .unwrap_err();
        assert!(error.contains("replaced during verification"), "{error}");
        assert_eq!(
            std::fs::read(original.join("known.bin")).unwrap(),
            b"original"
        );
        assert_eq!(std::fs::read(session.join("new.bin")).unwrap(), b"new data");
    }
}
