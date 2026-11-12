use super::finishing::{check_junction, rename_to_sibling};
use super::{is_reparse, make_junction, readers_equal, COPY_FILE_CAP};
use crate::fsutil::PinnedLocalDeleteTree;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FileDispositionInfo, SetFileInformationByHandle, DELETE, FILE_DISPOSITION_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
};

type Failure = (&'static str, String);

fn verification_refused(err: io::Error) -> Failure {
    (
        "verification_refused",
        format!("migrate:rollback_conflict: {err}"),
    )
}

fn unlink_junction(junction: File) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFO {
        DeleteFile: true.into(),
    };
    unsafe {
        SetFileInformationByHandle(
            HANDLE(junction.as_raw_handle()),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    }
    .map_err(io::Error::other)
}

fn restore_junction(source: &Path, target: &Path, backup: &Path, detail: &str) -> Failure {
    if target.is_dir() && !is_reparse(target) && make_junction(source, target).is_ok() {
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
            format!("{detail}; the junction could not be restored. The backup remains at {} and the target remains at {}", backup.display(), target.display()),
        )
    }
}

pub(super) fn switch(
    source: &Path,
    backup: &Path,
    target: &Path,
    after_verification: impl FnOnce(),
) -> Result<(), Failure> {
    super::begin_copy_run();
    let mut archive = PinnedLocalDeleteTree::with_security(backup, COPY_FILE_CAP)
        .map_err(verification_refused)?;
    let _verified_target = archive
        .hold_verified_copy(target, true, readers_equal)
        .map_err(verification_refused)?;
    let identities = archive
        .relative_identities()
        .map_err(verification_refused)?;
    // The held archive also pins the common parent of the backup and junction.
    // Open the link itself exclusively, with no competing handle that could
    // keep a delete pending after our handle closes.
    let junction = OpenOptions::new()
        .access_mode(DELETE.0 | FILE_READ_ATTRIBUTES.0)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(source)
        .map_err(|err| {
            (
                "junction_refused",
                format!("junction could not be opened: {err}"),
            )
        })?;
    check_junction(&junction, source, target)
        .map_err(|err| ("junction_refused", err.to_string()))?;

    // NTFS refuses to rename a directory while descendants are open. Keep
    // the same archive root held, then recheck its descendants after the move.
    archive.release_children_for_rename();
    after_verification();
    check_junction(&junction, source, target)
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
            target,
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
            let target_pins = published.hold_verified_copy(target, true, readers_equal)?;
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
                    format!("{detail}; could not restore the backup name ({rename}). The moved directory remains at {} and the verified target remains at {}", source.display(), target.display()),
                ));
            }
            Err(restore_junction(source, target, backup, &detail))
        }
    }
}
