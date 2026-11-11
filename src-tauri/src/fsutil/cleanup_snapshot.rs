use super::{DirPin, PinnedLocalDeleteTree};
use crate::path_seal::FileManifest;
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use windows::Win32::Storage::FileSystem::{FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO};

type ObjectIdentity = (u64, [u8; 16]);

pub(crate) struct CleanupTreeIdentity(BTreeMap<PathBuf, ObjectIdentity>);

fn object_identity(pin: &DirPin) -> io::Result<ObjectIdentity> {
    let mut info = FILE_ID_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            pin.handle,
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
            let suffix = path
                .strip_prefix(&self.path)
                .ok()
                .and_then(Path::to_str)
                .ok_or_else(|| io::Error::other("invalid backup source path"))?;
            let key = if suffix.is_empty() {
                root_key.to_owned()
            } else {
                format!("{root_key}/{}", suffix.replace('\\', "/"))
            };
            let current = object_identity(pin)?;
            if identities.0.get(path) != Some(&current) {
                return Err(io::Error::other(format!(
                    "{}: object changed",
                    path.display()
                )));
            }
            let file = std::mem::ManuallyDrop::new(unsafe { File::from_raw_handle(pin.handle.0) });
            let digest = if *directory {
                None
            } else {
                Some(
                    crate::path_seal::publication_reader_digest(&*file)
                        .map_err(io::Error::other)?,
                )
            };
            if manifest.get(&key) != Some(&digest) {
                return Err(io::Error::other(format!(
                    "{}: contents changed or were not backed up",
                    path.display()
                )));
            }
        }
        Ok(())
    }
}
