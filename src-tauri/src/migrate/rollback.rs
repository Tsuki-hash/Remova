use super::finishing::rename_to_sibling;
use super::junction::{Junction, Target};
use super::receipt::{Receipt, Transition};
use super::{make_junction, COPY_FILE_CAP};
use crate::fsutil::{DirPin, PinnedLocalDeleteTree};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use windows::Win32::Storage::FileSystem::{
    FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
};

type Failure = (&'static str, String);

fn verification_refused(err: io::Error) -> Failure {
    (
        "verification_refused",
        format!("migrate:rollback_conflict: {err}"),
    )
}

fn verify_restored_archive(
    archive: &PinnedLocalDeleteTree,
    source: &Path,
    target: &Target,
    identities: &BTreeMap<PathBuf, (u32, u64)>,
) -> io::Result<(PinnedLocalDeleteTree, Vec<DirPin>)> {
    let published = archive.resnapshot_renamed(source, COPY_FILE_CAP)?;
    if published.relative_identities()? != *identities {
        return Err(io::Error::other("backup entry identities changed"));
    }
    let pins = target.verify_copy(&published, true)?;
    Ok((published, pins))
}

pub(super) fn unlink_junction(junction: Junction) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFO {
        DeleteFile: true.into(),
    };
    unsafe {
        SetFileInformationByHandle(
            junction.handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    }
    .map_err(io::Error::other)
}

fn restore_junction(source: &Path, target: &Target, backup: &Path, detail: &str) -> Failure {
    let restored = make_junction(source, target.path()).is_ok()
        && Junction::open(source, false)
            .and_then(|junction| junction.verify_target(target))
            .is_ok();
    if restored {
        (
            "junction_restored",
            format!(
                "{detail}; the junction was restored and the backup remains at {}",
                backup.display()
            ),
        )
    } else {
        (
            "junction_restore_failed",
            format!("{detail}; the junction could not be verified after restoration. The backup remains at {} and the target remains at {}", backup.display(), target.path().display()),
        )
    }
}

pub(super) fn switch(
    source: &Path,
    backup: &Path,
    target: &Path,
    after_verification: impl FnOnce(),
    after_unlink: impl FnOnce(),
) -> Result<(), Failure> {
    let target = Target::open(target).map_err(verification_refused)?;
    let mut archive = PinnedLocalDeleteTree::with_security(backup, COPY_FILE_CAP)
        .map_err(verification_refused)?;
    let _verified_target = target
        .verify_copy(&archive, true)
        .map_err(verification_refused)?;
    let identities = archive
        .relative_identities()
        .map_err(verification_refused)?;
    // The held archive also pins the common parent of the backup and junction.
    // Open the link itself exclusively, with no competing handle that could
    // keep a delete pending after our handle closes.
    let junction = Junction::open(source, true).map_err(|err| {
        (
            "junction_refused",
            format!("junction could not be opened: {err}"),
        )
    })?;
    let linked = junction
        .verify_target(&target)
        .map_err(|err| ("junction_refused", err.to_string()))?;
    let transition = Receipt::prepare_transition(
        source,
        target.path(),
        archive
            .root_pin()
            .file_identity()
            .map_err(verification_refused)?,
        target.receipt_id().map_err(verification_refused)?,
        Transition::Rollback,
    )
    .map_err(receipt_failed)?;

    // NTFS refuses to rename a directory while descendants are open. Keep
    // the same archive root held, then recheck its descendants after the move.
    archive.release_children_for_rename();
    after_verification();
    let _linked_now = junction
        .verify_target(&target)
        .map_err(|err| ("junction_refused", err.to_string()))?;
    unlink_junction(junction).map_err(|err| {
        (
            "junction_removal_failed",
            format!("junction removal failed: {err}"),
        )
    })?;
    after_unlink();
    if let Err(err) = rename_to_sibling(archive.root_pin(), backup, source) {
        return Err(restore_junction(
            source,
            &linked,
            backup,
            &format!("backup rename failed: {err}"),
        ));
    }

    match verify_restored_archive(&archive, source, &target, &identities) {
        Ok((_published, _target_pins)) => transition
            .complete_transition(target.path())
            .map_err(receipt_failed),
        Err(err) => {
            let detail =
                format!("migrate:rollback_conflict: backup changed during the move: {err}");
            if let Err(rename) = rename_to_sibling(archive.root_pin(), source, backup) {
                return Err((
                    "backup_rename_restore_failed",
                    format!("{detail}; could not restore the backup name ({rename}). The moved directory remains at {} and the verified target remains at {}", source.display(), target.path().display()),
                ));
            }
            Err(restore_junction(source, &linked, backup, &detail))
        }
    }
}

fn receipt_failed(error: io::Error) -> Failure {
    (
        "receipt_failed",
        format!("migrate:transition_receipt_failed: {error}"),
    )
}

pub(super) fn recover_interrupted(
    source: &Path,
    backup: &Path,
    target: &Path,
) -> Result<(), Failure> {
    let target = Target::open(target).map_err(verification_refused)?;
    let mut archive = PinnedLocalDeleteTree::with_security(backup, COPY_FILE_CAP)
        .map_err(verification_refused)?;
    let transition = Receipt::interrupted(
        source,
        target.path(),
        archive
            .root_pin()
            .file_identity()
            .map_err(verification_refused)?,
        target.receipt_id().map_err(verification_refused)?,
    )
    .ok_or(("receipt_missing", "migrate:recovery_refused".into()))?;
    let _target_pins = target
        .verify_copy(&archive, true)
        .map_err(verification_refused)?;
    let identities = archive
        .relative_identities()
        .map_err(verification_refused)?;
    archive.release_children_for_rename();
    // The source can be claimed after the status check. The held backup can
    // only be renamed to an unoccupied sibling, even in that race.
    rename_to_sibling(archive.root_pin(), backup, source).map_err(verification_refused)?;
    match verify_restored_archive(&archive, source, &target, &identities) {
        Ok((_published, _pins)) => transition
            .complete_transition(target.path())
            .map_err(receipt_failed),
        Err(error) => {
            if let Err(rename) = rename_to_sibling(archive.root_pin(), source, backup) {
                return Err(("backup_rename_restore_failed", format!(
                    "migrate:rollback_conflict: {error}. Could not restore the backup name ({rename}). Both directories remain at {} and {}",
                    source.display(), target.path().display()
                )));
            }
            Err(verification_refused(error))
        }
    }
}
