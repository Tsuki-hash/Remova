use super::finishing::rename_to_sibling;
use super::junction::{Junction, Target};
use super::{make_junction, COPY_FILE_CAP};
use crate::fsutil::PinnedLocalDeleteTree;
use std::io;
use std::path::Path;
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
    if let Err(err) = rename_to_sibling(archive.root_pin(), source) {
        return Err(restore_junction(
            source,
            &linked,
            backup,
            &format!("backup rename failed: {err}"),
        ));
    }

    let checked = archive
        .resnapshot_renamed(source, COPY_FILE_CAP)
        .and_then(|published| {
            if published.relative_identities()? != identities {
                return Err(io::Error::other("backup entry identities changed"));
            }
            let target_pins = target.verify_copy(&published, true)?;
            Ok((published, target_pins))
        });
    match checked {
        Ok((_published, _target_pins)) => Ok(()),
        Err(err) => {
            let detail =
                format!("migrate:rollback_conflict: backup changed during the move: {err}");
            if let Err(rename) = rename_to_sibling(archive.root_pin(), backup) {
                return Err((
                    "backup_rename_restore_failed",
                    format!("{detail}; could not restore the backup name ({rename}). The moved directory remains at {} and the verified target remains at {}", source.display(), target.path().display()),
                ));
            }
            Err(restore_junction(source, &linked, backup, &detail))
        }
    }
}
