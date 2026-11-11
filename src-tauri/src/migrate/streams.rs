//! Compare the native backup streams through already-open file handles.
//! https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-backupread

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    BackupRead, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
};

pub(super) fn open(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1) // FILE_SHARE_READ
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    if super::meta_is_reparse(&file.metadata()?) {
        return Err(io::Error::other(
            "migration comparison refused a reparse point",
        ));
    }
    Ok(file)
}

struct BackupReader {
    file: File,
    context: *mut std::ffi::c_void,
}

impl BackupReader {
    fn new(file: File) -> Self {
        Self {
            file,
            context: std::ptr::null_mut(),
        }
    }

    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut read = 0;
        // A synchronous file handle is held by this reader. Its context is
        // initialized once and released with the documented abort call.
        unsafe {
            BackupRead(
                HANDLE(self.file.as_raw_handle()),
                buffer,
                &mut read,
                false,
                false,
                &mut self.context,
            )
        }
        .map_err(|err| io::Error::other(err.to_string()))?;
        Ok(read as usize)
    }
}

impl Drop for BackupReader {
    fn drop(&mut self) {
        if !self.context.is_null() {
            let mut read = 0;
            let _ = unsafe {
                BackupRead(
                    HANDLE(self.file.as_raw_handle()),
                    &mut [],
                    &mut read,
                    true,
                    false,
                    &mut self.context,
                )
            };
        }
    }
}

pub(super) fn equal(left: File, right: File) -> io::Result<bool> {
    if left.metadata()?.is_dir() && !super::security::directories_equal(&left, &right)? {
        return Ok(false);
    }
    let mut left = BackupReader::new(left);
    let mut right = BackupReader::new(right);
    let mut left_buffer = [0; 64 * 1024];
    let mut right_buffer = [0; 64 * 1024];
    loop {
        if super::copy_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "migrate:cancelled",
            ));
        }
        let a = left.read(&mut left_buffer)?;
        let b = right.read(&mut right_buffer)?;
        if left_buffer[..a] != right_buffer[..b] {
            return Ok(false);
        }
        if a == 0 {
            return Ok(true);
        }
    }
}
