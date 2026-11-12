use super::{meta_is_directory, meta_is_reparse, readers_equal};
use crate::fsutil::{self, DirPin, PinnedLocalDeleteTree};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FileIdInfo, GetFileInformationByHandleEx, DELETE, FILE_ID_INFO, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ,
};

type DirectoryIdentity = (u64, [u8; 16]);

fn identity(handle: HANDLE) -> io::Result<DirectoryIdentity> {
    let mut info = FILE_ID_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    }
    .map_err(io::Error::other)?;
    if info.FileId.Identifier == [0; 16] {
        return Err(io::Error::other(
            "volume did not report a directory identity",
        ));
    }
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}

pub(super) struct Target {
    path: PathBuf,
    pins: Vec<DirPin>,
    id: DirectoryIdentity,
}

impl Target {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        let pins = fsutil::pin_local_scan_path(path)?;
        let root = pins
            .last()
            .ok_or_else(|| io::Error::other("no target root"))?;
        let metadata = fsutil::pin_metadata(root)?;
        if !meta_is_directory(&metadata) || meta_is_reparse(&metadata) {
            return Err(io::Error::other("target must be an ordinary directory"));
        }
        let id = identity(HANDLE(root.as_raw_handle()))?;
        Ok(Self {
            path: path.into(),
            pins,
            id,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn receipt_id(&self) -> io::Result<(u32, u64)> {
        self.pins.last().expect("held target root").file_identity()
    }

    pub(super) fn verify_copy(
        &self,
        archive: &PinnedLocalDeleteTree,
        exact: bool,
    ) -> io::Result<Vec<DirPin>> {
        let verified = archive.hold_verified_copy(&self.path, exact, readers_equal)?;
        let root = verified
            .first()
            .ok_or_else(|| io::Error::other("missing verified target"))?;
        if identity(HANDLE(root.as_raw_handle()))? != self.id {
            return Err(io::Error::other("verified target directory changed"));
        }
        Ok(verified)
    }
}

pub(super) struct Junction {
    pin: DirPin,
    _parents: Vec<DirPin>,
}

impl Junction {
    pub(super) fn open(source: &Path, for_removal: bool) -> io::Result<Self> {
        let (access, share) = if for_removal {
            (FILE_READ_ATTRIBUTES.0 | DELETE.0, 0)
        } else {
            (FILE_READ_ATTRIBUTES.0, FILE_SHARE_READ.0)
        };
        let (parents, pin) = fsutil::open_local_scan_leaf(source, access, share)?;
        let metadata = fsutil::pin_metadata(&pin)?;
        if !meta_is_directory(&metadata) || !meta_is_reparse(&metadata) {
            return Err(io::Error::other("source is not a directory junction"));
        }
        Ok(Self {
            pin,
            _parents: parents,
        })
    }

    /// Resolve the actual substitute name of this link without crossing any
    /// other reparse point, and keep its directory chain held for the caller.
    pub(super) fn verify_target(&self, target: &Target) -> io::Result<Target> {
        let (tag, path) = fsutil::read_reparse_target(self.handle())?;
        if tag != 0xa0000003 {
            return Err(io::Error::other("source is not a directory junction"));
        }
        let linked = Target::open(&path)?;
        if linked.id != target.id {
            return Err(io::Error::other(
                "junction does not point at the stated target",
            ));
        }
        Ok(linked)
    }

    pub(super) fn handle(&self) -> HANDLE {
        HANDLE(self.pin.as_raw_handle())
    }
}
