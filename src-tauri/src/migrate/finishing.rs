use crate::fsutil::{DirPin, PinnedLocalDeleteTree};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::Wdk::Storage::FileSystem::{
    FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::IO::IO_STATUS_BLOCK;

use super::{
    junction::{Junction, Target},
    receipt::Receipt,
    COPY_FILE_CAP,
};

pub(super) fn rename_to_sibling(root: &DirPin, destination: &Path) -> io::Result<()> {
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

fn stage(
    source: &Path,
    backup: &Path,
    pending: &Path,
    target: &Target,
    junction: &Junction,
) -> io::Result<((u32, u64), Vec<DirPin>)> {
    let mut snapshot = PinnedLocalDeleteTree::with_security(backup, COPY_FILE_CAP)?;
    let target_pins = target.verify_copy(&snapshot, false)?;
    let archive_id = snapshot.root_pin().file_identity()?;
    let linked = junction.verify_target(target)?;
    Receipt::prepare_cleanup(source, linked.path(), archive_id, target.receipt_id()?)?;
    rename_to_sibling(snapshot.release_children_for_rename(), pending)?;
    Ok((archive_id, target_pins))
}

fn verify_pending(
    source: &Path,
    pending: &Path,
    target: &Target,
    junction: &Junction,
) -> io::Result<(PinnedLocalDeleteTree, Vec<DirPin>, Target)> {
    let snapshot = PinnedLocalDeleteTree::with_security(pending, COPY_FILE_CAP)?;
    let target_pins = target.verify_copy(&snapshot, false)?;
    let linked = junction.verify_target(target)?;
    if !Receipt::matches_cleanup(
        source,
        [linked.path(), target.path()],
        snapshot.root_pin().file_identity()?,
        target.receipt_id()?,
    ) {
        return Err(io::Error::other(
            "pending copy has no matching migration receipt",
        ));
    }
    Ok((snapshot, target_pins, linked))
}

fn restore_backup_name(pending: &Path, backup: &Path, archive_id: (u32, u64)) -> io::Result<()> {
    let parent = pending
        .parent()
        .ok_or_else(|| io::Error::other("pending cleanup path has no parent"))?;
    let _parents = crate::fsutil::pin_local_scan_path(parent)?;
    let root = crate::fsutil::pin_private_delete(pending)?;
    if root.file_identity()? != archive_id {
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
    let target = Target::open(target).map_err(|err| format!("finish refused: {err}"))?;
    let junction = Junction::open(source, false).map_err(|err| format!("finish refused: {err}"))?;
    let _linked = junction
        .verify_target(&target)
        .map_err(|err| format!("finish refused: {err}"))?;
    let staging = if staged {
        None
    } else {
        Some(
            stage(source, backup, pending, &target, &junction)
                .map_err(|err| format!("finish refused: {err}"))?,
        )
    };
    after_stage();
    // The original rollback copy has been committed for removal. Reopen and
    // verify the exact staged tree before deleting it, including on a retry.
    let verified = verify_pending(source, pending, &target, &junction);
    let (snapshot, _target_pins, _linked_now) = match verified {
        Ok(result) => result,
        Err(err) => {
            if let Some((archive_id, _)) = &staging {
                if restore_backup_name(pending, backup, *archive_id).is_ok() {
                    return Err(format!(
                        "finish refused: {err}; the rollback directory was restored"
                    ));
                }
            }
            return Err(cleanup_pending_error(pending, target.path(), err));
        }
    };
    snapshot
        .remove()
        .map_err(|err| cleanup_pending_error(pending, target.path(), err))
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
