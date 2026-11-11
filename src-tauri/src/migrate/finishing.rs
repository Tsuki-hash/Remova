use crate::fsutil::{DirPin, PinnedLocalDeleteTree};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::Wdk::Storage::FileSystem::{
    FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;

use super::{lower, meta_is_reparse, readers_equal, receipt::Receipt, COPY_FILE_CAP};

fn directory_id(pin: &DirPin) -> io::Result<(u32, u64)> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(pin.as_raw_handle()), &mut info) }
        .map_err(io::Error::other)?;
    let id = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    if id == 0 {
        return Err(io::Error::other("volume did not report a file identity"));
    }
    Ok((info.dwVolumeSerialNumber, id))
}

fn rename_to_sibling(root: &DirPin, destination: &Path) -> io::Result<()> {
    let name: Vec<_> = destination
        .file_name()
        .ok_or_else(|| io::Error::other("pending cleanup path has no name"))?
        .encode_wide()
        .collect();
    let name_bytes = std::mem::size_of_val(name.as_slice());
    let bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>() + name_bytes;
    // A simple filename and a NULL RootDirectory rename within the
    // current parent. Never overwrite an existing sibling.
    let mut buffer = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        std::ptr::addr_of_mut!((*info).FileNameLength).write(name_bytes as u32);
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            name.len(),
        );
        let mut status = IO_STATUS_BLOCK::default();
        NtSetInformationFile(
            HANDLE(root.as_raw_handle()),
            &mut status,
            info.cast(),
            bytes as u32,
            FileRenameInformation,
        )
        .ok()
    }
    .map_err(io::Error::other)
}

fn pin_junction(source: &Path, target: &Path) -> io::Result<(Vec<DirPin>, File)> {
    let parent = source
        .parent()
        .ok_or_else(|| io::Error::other("junction has no parent"))?;
    let pins = crate::fsutil::pin_local_scan_path(parent)?;
    let junction = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(source)?;
    if !meta_is_reparse(&junction.metadata()?)
        || lower(&std::fs::read_link(source)?) != lower(target)
    {
        return Err(io::Error::other("junction missing or pointing elsewhere"));
    }
    Ok((pins, junction))
}

fn stage(
    source: &Path,
    backup: &Path,
    pending: &Path,
    target: &Path,
) -> io::Result<((u32, u64), Vec<DirPin>)> {
    let mut snapshot = PinnedLocalDeleteTree::with_security(backup, COPY_FILE_CAP)?;
    let target_pins = snapshot.hold_verified_copy(target, false, readers_equal)?;
    let archive_id = directory_id(snapshot.root_pin())?;
    Receipt::prepare_cleanup(source, target, archive_id, directory_id(&target_pins[0])?)?;
    rename_to_sibling(snapshot.release_children_for_rename(), pending)?;
    Ok((archive_id, target_pins))
}

fn verify_pending(
    source: &Path,
    pending: &Path,
    target: &Path,
) -> io::Result<(PinnedLocalDeleteTree, Vec<DirPin>)> {
    let snapshot = PinnedLocalDeleteTree::with_security(pending, COPY_FILE_CAP)?;
    let target_pins = snapshot.hold_verified_copy(target, false, readers_equal)?;
    if !Receipt::matches_cleanup(
        source,
        target,
        directory_id(snapshot.root_pin())?,
        directory_id(&target_pins[0])?,
    ) {
        return Err(io::Error::other(
            "pending copy has no matching migration receipt",
        ));
    }
    Ok((snapshot, target_pins))
}

fn restore_backup_name(pending: &Path, backup: &Path, archive_id: (u32, u64)) -> io::Result<()> {
    let parent = pending
        .parent()
        .ok_or_else(|| io::Error::other("pending cleanup path has no parent"))?;
    let _parents = crate::fsutil::pin_local_scan_path(parent)?;
    let root = crate::fsutil::pin_private_delete(pending)?;
    if directory_id(&root)? != archive_id {
        return Err(io::Error::other("pending copy identity changed"));
    }
    rename_to_sibling(&root, backup)
}

pub(super) fn finish(
    source: &Path,
    backup: &Path,
    pending: &Path,
    target: &Path,
    staged: bool,
) -> Result<(), String> {
    finish_with(source, backup, pending, target, staged, || {})
}

fn finish_with(
    source: &Path,
    backup: &Path,
    pending: &Path,
    target: &Path,
    staged: bool,
    after_stage: impl FnOnce(),
) -> Result<(), String> {
    let _junction = pin_junction(source, target).map_err(|err| format!("finish refused: {err}"))?;
    super::begin_copy_run();
    let staging = if staged {
        None
    } else {
        Some(
            stage(source, backup, pending, target)
                .map_err(|err| format!("finish refused: {err}"))?,
        )
    };
    after_stage();
    // The original rollback copy has been committed for removal. Reopen and
    // verify the exact staged tree before deleting it, including on a retry.
    let (snapshot, _target_pins) = match verify_pending(source, pending, target) {
        Ok(result) => result,
        Err(err) => {
            if let Some((archive_id, _)) = &staging {
                if restore_backup_name(pending, backup, *archive_id).is_ok() {
                    return Err(format!(
                        "finish refused: {err}; the rollback directory was restored"
                    ));
                }
            }
            return Err(cleanup_pending_error(pending, target, err));
        }
    };
    snapshot
        .remove()
        .map_err(|err| cleanup_pending_error(pending, target, err))
}

fn cleanup_pending_error(pending: &Path, target: &Path, err: io::Error) -> String {
    format!(
        "migrate:cleanup_pending: {}: {err}. The live target remains at {}.",
        pending.display(),
        target.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_staged_copy_restores_the_rollback_name_without_deleting_entries() {
        let _guard = crate::executor::CLEANUP_LOCK.lock().unwrap();
        let fixture = crate::test_support::TempDir::new("migration_stage_change");
        let source = fixture.join("source");
        let target = fixture.subdir("target");
        let backup = super::super::backup_path(&source);
        let pending = super::super::pending_path(&source);
        std::fs::create_dir(&backup).unwrap();
        for dir in [&backup, &target] {
            std::fs::write(dir.join("initial"), b"original").unwrap();
        }
        super::super::make_junction(&source, &target).unwrap();
        let error = finish_with(&source, &backup, &pending, &target, false, || {
            std::fs::write(pending.join("new"), b"added after verification").unwrap();
        })
        .unwrap_err();
        assert!(error.contains("rollback directory was restored"), "{error}");
        assert_eq!(std::fs::read(backup.join("initial")).unwrap(), b"original");
        assert_eq!(
            std::fs::read(backup.join("new")).unwrap(),
            b"added after verification"
        );
        assert_eq!(std::fs::read(source.join("initial")).unwrap(), b"original");
        assert!(!pending.exists());
    }
}
