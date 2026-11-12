//! Write an authenticated restore entry without replacing any live object.

use super::retry::{directory_key, DirectoryRecord, Journal, Receipts};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use windows::core::PWSTR;
use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FileDispositionInformationEx, FileRenameInformation, NtCreateFile, NtSetInformationFile,
    FILE_CREATE, FILE_DELETE_ON_CLOSE, FILE_DIRECTORY_FILE, FILE_DISPOSITION_INFORMATION_EX,
    FILE_DISPOSITION_ON_CLOSE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_DISPOSITION,
    NTCREATEFILE_CREATE_OPTIONS,
};
use windows::Win32::Foundation::{
    HANDLE, NTSTATUS, STATUS_FILE_IS_A_DIRECTORY, STATUS_NOT_A_DIRECTORY, STATUS_NO_SUCH_FILE,
    STATUS_OBJECT_NAME_COLLISION, STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND,
    STATUS_REPARSE_POINT_ENCOUNTERED, UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    FileDispositionInfo, SetFileInformationByHandle, DELETE, FILE_ACCESS_RIGHTS,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO,
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_MODE, FILE_SHARE_NONE, FILE_SHARE_READ,
    FILE_WRITE_DATA, SYNCHRONIZE,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;

#[derive(Debug)]
pub(super) enum Failure {
    Occupied,
    PartialDirectory(io::Error),
    DirectoryConflicts { count: usize, first: PathBuf },
    Io(io::Error),
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::AlreadyExists {
            Self::Occupied
        } else {
            Self::Io(error)
        }
    }
}

fn relative_name(name: &OsStr) -> io::Result<Vec<u16>> {
    let name: Vec<_> = name.encode_wide().collect();
    if name.is_empty()
        || name.len() > 255
        || name.iter().any(|ch| matches!(*ch, 0 | 47 | 58 | 92))
        || name.last().is_some_and(|ch| matches!(*ch, 32 | 46))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid restore name",
        ));
    }
    Ok(name)
}

fn nt_result(status: NTSTATUS) -> io::Result<()> {
    if status == STATUS_OBJECT_NAME_COLLISION {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "restore target already exists",
        ));
    }
    if matches!(
        status,
        STATUS_OBJECT_NAME_NOT_FOUND | STATUS_OBJECT_PATH_NOT_FOUND | STATUS_NO_SUCH_FILE
    ) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "restore target does not exist",
        ));
    }
    if matches!(
        status,
        STATUS_NOT_A_DIRECTORY | STATUS_REPARSE_POINT_ENCOUNTERED
    ) {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "restore path has a different type",
        ));
    }
    if status == STATUS_FILE_IS_A_DIRECTORY {
        return Err(io::Error::new(
            io::ErrorKind::IsADirectory,
            "restore path has a different type",
        ));
    }
    status.ok().map_err(io::Error::other)
}

const READ_ENTRY: u32 = FILE_READ_DATA.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;

fn relative_entry(
    parent: &impl AsRawHandle,
    name: &OsStr,
    access: u32,
    share: FILE_SHARE_MODE,
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
    kind: NTCREATEFILE_CREATE_OPTIONS,
) -> io::Result<File> {
    let mut name = relative_name(name)?;
    let length = (name.len() * 2) as u16;
    let unicode = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: PWSTR(name.as_mut_ptr()),
    };
    let attrs = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: HANDLE(parent.as_raw_handle()),
        ObjectName: &unicode,
        // OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE. The name is one component.
        Attributes: 0x40 | 0x1000,
        ..Default::default()
    };
    let mut handle = HANDLE::default();
    let mut status = IO_STATUS_BLOCK::default();
    nt_result(unsafe {
        NtCreateFile(
            &mut handle,
            FILE_ACCESS_RIGHTS(access),
            &attrs,
            &mut status,
            None,
            FILE_ATTRIBUTE_NORMAL,
            share,
            disposition,
            NTCREATEFILE_CREATE_OPTIONS(
                kind.0 | FILE_OPEN_REPARSE_POINT.0 | FILE_SYNCHRONOUS_IO_NONALERT.0,
            ),
            None,
            0,
        )
    })?;
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

fn create_file(parent: &impl AsRawHandle, name: &OsStr) -> io::Result<File> {
    relative_entry(
        parent,
        name,
        READ_ENTRY | FILE_WRITE_DATA.0 | DELETE.0,
        FILE_SHARE_NONE,
        FILE_CREATE,
        NTCREATEFILE_CREATE_OPTIONS(FILE_NON_DIRECTORY_FILE.0 | FILE_DELETE_ON_CLOSE.0),
    )
}

fn open_directory(parent: &impl AsRawHandle, name: &OsStr) -> io::Result<File> {
    relative_entry(
        parent,
        name,
        READ_ENTRY,
        FILE_SHARE_READ,
        FILE_OPEN,
        FILE_DIRECTORY_FILE,
    )
}

fn commit_file(file: &File, name: &OsStr) -> io::Result<()> {
    let name = relative_name(name)?;
    let name_bytes = std::mem::size_of_val(name.as_slice());
    let bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>() + name_bytes;
    let mut buffer = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    // Zero-initialized ReplaceIfExists and RootDirectory mean an atomic
    // no-replace rename within the exact handle's current parent.
    unsafe {
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        std::ptr::addr_of_mut!((*info).FileNameLength).write(name_bytes as u32);
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            name.len(),
        );
        let mut status = IO_STATUS_BLOCK::default();
        nt_result(NtSetInformationFile(
            HANDLE(file.as_raw_handle()),
            &mut status,
            info.cast(),
            bytes as u32,
            FileRenameInformation,
        ))
    }
}

fn keep_published_entry(file: &File) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFORMATION_EX {
        Flags: FILE_DISPOSITION_ON_CLOSE,
    };
    let mut status = IO_STATUS_BLOCK::default();
    // Remove the on-close flag only after the exclusive handle has renamed
    // the fully prepared object. A process exit before this point is safe to retry.
    nt_result(unsafe {
        NtSetInformationFile(
            HANDLE(file.as_raw_handle()),
            &mut status,
            std::ptr::from_ref(&info).cast(),
            std::mem::size_of_val(&info) as u32,
            FileDispositionInformationEx,
        )
    })
}

pub(super) fn discard_temporary_file(file: &File) {
    let info = FILE_DISPOSITION_INFO {
        DeleteFile: true.into(),
    };
    // A failure can only leave the private-name temporary. Never delete by
    // pathname, since another process could then replace that name.
    let _ = unsafe {
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    };
}

fn check_cancelled() -> io::Result<()> {
    if crate::executor::cleanup_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "restore cancelled",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn stop_at_test_checkpoint(name: &str) {
    if std::env::var("REMOVA_TEST_RESTORE_PUBLICATION_CHECKPOINT").as_deref() == Ok(name) {
        std::process::exit(23);
    }
}

fn temporary_file(parent: &impl AsRawHandle, target: &OsStr) -> io::Result<File> {
    for _ in 0..32 {
        check_cancelled()?;
        let path = crate::fsutil::atomic_tmp_path(Path::new(target));
        let name = path.file_name().expect("temporary file has a name");
        if name
            .to_str()
            .zip(target.to_str())
            .is_some_and(|(name, target)| name.eq_ignore_ascii_case(target))
        {
            continue;
        }
        match create_file(parent, name) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            result => return result,
        }
    }
    Err(io::Error::other("restore temporary name collisions"))
}

fn write_new_file(source: &Path, parent: &impl AsRawHandle, name: &OsStr) -> io::Result<()> {
    let mut source = crate::fsutil::open_local_regular_file(source)?;
    crate::fsutil::reject_named_data_streams(&source)?;
    let mut temporary = temporary_file(parent, name)?;
    let result = (|| {
        let mut chunk = [0; 64 * 1024];
        loop {
            check_cancelled()?;
            let count = match source.read(&mut chunk) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            if count == 0 {
                break;
            }
            temporary.write_all(&chunk[..count])?;
            #[cfg(test)]
            stop_at_test_checkpoint("file-chunk");
        }
        temporary.sync_all()?;
        crate::fsutil::reject_named_data_streams(&source)?;
        crate::fsutil::reject_named_data_streams(&temporary)?;
        check_cancelled()?;
        #[cfg(test)]
        stop_at_test_checkpoint("file-before-rename");
        commit_file(&temporary, name)?;
        keep_published_entry(&temporary)
    })();
    if result.is_err() {
        discard_temporary_file(&temporary);
    }
    result
}

fn claim_directory(
    parent: &impl AsRawHandle,
    name: &OsStr,
    relative: &Path,
    journal: &mut Journal,
) -> io::Result<File> {
    for _ in 0..32 {
        check_cancelled()?;
        let temporary = crate::fsutil::atomic_tmp_path(Path::new(name));
        let temporary_name = temporary
            .file_name()
            .expect("temporary directory has a name");
        if temporary_name.eq_ignore_ascii_case(name) {
            continue;
        }
        let directory = match relative_entry(
            parent,
            temporary_name,
            READ_ENTRY | DELETE.0,
            FILE_SHARE_READ,
            FILE_CREATE,
            NTCREATEFILE_CREATE_OPTIONS(FILE_DIRECTORY_FILE.0 | FILE_DELETE_ON_CLOSE.0),
        ) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            result => result?,
        };
        let result = (|| {
            let key = directory_key(relative)?;
            journal.remember(key, &directory)?;
            check_cancelled()?;
            #[cfg(test)]
            stop_at_test_checkpoint("directory-before-rename");
            commit_file(&directory, name)?;
            keep_published_entry(&directory)
        })();
        if let Err(error) = result {
            // Only the unpublished object we created can be disposed. If a
            // late child makes it nonempty, leave it and its data intact.
            discard_temporary_file(&directory);
            return Err(error);
        }
        return Ok(directory);
    }
    Err(io::Error::other(
        "restore temporary directory name collisions",
    ))
}

fn matching_file(mut existing: File, source: &Path) -> io::Result<bool> {
    let metadata = existing.metadata()?;
    if crate::fsutil::is_reparse_metadata(&metadata) {
        return Ok(false);
    }
    let mut source = crate::fsutil::open_local_regular_file(source)?;
    if metadata.len() != source.metadata()?.len() {
        return Ok(false);
    }
    let mut expected = [0; 64 * 1024];
    let mut actual = [0; 64 * 1024];
    loop {
        check_cancelled()?;
        let count = source.read(&mut expected)?;
        if count == 0 {
            return Ok(true);
        }
        existing.read_exact(&mut actual[..count])?;
        if actual[..count] != expected[..count] {
            return Ok(false);
        }
    }
}

struct DirectoryRestore<F> {
    journal: Journal,
    left: usize,
    retained: Vec<(File, File)>,
    conflicts: usize,
    first_conflict: Option<PathBuf>,
    after_file: F,
}

impl<F: FnMut(&Path) -> io::Result<()>> DirectoryRestore<F> {
    fn conflict(&mut self, relative: &Path) {
        self.conflicts += 1;
        self.first_conflict
            .get_or_insert_with(|| relative.to_owned());
    }

    fn write_contents(
        &mut self,
        source: &Path,
        directory: File,
        relative: &Path,
        depth: usize,
    ) -> io::Result<()> {
        let guard = guard_restored_directory(&directory)?;
        let source_pin = crate::fsutil::pin_target_readonly(source)?;
        source_pin.check_data_streams()?;
        for name in crate::fsutil::read_local_entry_names(source, self.left)? {
            check_cancelled()?;
            if self.left == 0 || depth >= 64 {
                return Err(io::Error::other(
                    "restore directory exceeds verification limit",
                ));
            }
            self.left -= 1;
            let child = source.join(&name);
            let relative = relative.join(&name);
            let metadata = crate::fsutil::scan_path_metadata(&child)?;
            if metadata.is_dir() {
                let key = directory_key(&relative)?;
                let dest = match open_directory(&directory, &name) {
                    Ok(existing) if self.journal.owns(&key, &existing)? => existing,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        match claim_directory(&directory, &name, &relative, &mut self.journal) {
                            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                                self.conflict(&relative);
                                continue;
                            }
                            result => result?,
                        }
                    }
                    _ => {
                        self.conflict(&relative);
                        continue;
                    }
                };
                if !self.journal.owns(&key, &dest)? {
                    return Err(io::Error::other("restore directory identity changed"));
                }
                self.write_contents(&child, dest, &relative, depth + 1)?;
            } else {
                match relative_entry(
                    &directory,
                    &name,
                    READ_ENTRY,
                    FILE_SHARE_READ,
                    FILE_OPEN,
                    FILE_NON_DIRECTORY_FILE,
                ) {
                    Ok(existing) => match matching_file(existing, &child) {
                        Ok(true) => {}
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                            return Err(error)
                        }
                        _ => self.conflict(&relative),
                    },
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        match write_new_file(&child, &directory, &name) {
                            Ok(()) => (self.after_file)(&relative)?,
                            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                                self.conflict(&relative)
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    Err(_) => self.conflict(&relative),
                }
            }
        }
        source_pin.check_data_streams()?;
        // Keep the exact object and its marker pinned through publication.
        self.retained.push((directory, guard));
        Ok(())
    }
}

fn guard_restored_directory(directory: &File) -> io::Result<File> {
    let check_directory = || {
        if directory.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(io::Error::other(
                "new restore directory became a reparse point",
            ));
        }
        Ok(())
    };
    check_directory()?;
    let (_, guard) = crate::fsutil::guard_empty_directory(HANDLE(directory.as_raw_handle()), true)?
        .ok_or_else(|| io::Error::other("could not guard the new restore directory"))?;
    check_directory()?;
    Ok(guard)
}

fn collect_owned_directories(
    source: &Path,
    directory: &File,
    relative: &Path,
    journal: &Journal,
    live: &mut BTreeSet<DirectoryRecord>,
    left: &mut usize,
) -> io::Result<()> {
    let Some(record) = journal.owned_record(&directory_key(relative)?, directory)? else {
        return Ok(());
    };
    live.insert(record);
    for name in crate::fsutil::read_local_entry_names(source, *left)? {
        check_cancelled()?;
        *left -= 1;
        let child = source.join(&name);
        if !crate::fsutil::scan_path_metadata(&child)?.is_dir() {
            continue;
        }
        match open_directory(directory, &name) {
            Ok(target) => {
                collect_owned_directories(
                    &child,
                    &target,
                    &relative.join(&name),
                    journal,
                    live,
                    left,
                )?;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn publish(
    source: &Path,
    parent: &impl AsRawHandle,
    name: &OsStr,
    receipts: &Receipts<'_>,
    entry: &str,
) -> Result<(), Failure> {
    check_cancelled()?;
    if crate::fsutil::scan_path_metadata(source)?.is_dir() {
        publish_directory(source, parent, name, receipts, entry, |_| Ok(()))
    } else {
        write_new_file(source, parent, name).map_err(Failure::from)
    }
}

pub(super) fn publish_directory(
    source: &Path,
    parent: &impl AsRawHandle,
    name: &OsStr,
    receipts: &Receipts<'_>,
    entry: &str,
    after_file: impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), Failure> {
    let root = Path::new("");
    let key = directory_key(root)?;
    let (directory, journal) = match open_directory(parent, name) {
        Ok(directory) => {
            let mut journal = match receipts.open_writer(entry, false) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(Failure::Occupied)
                }
                result => result?,
            };
            if !journal.owns(&key, &directory)? {
                return Err(Failure::Occupied);
            }
            if journal.needs_compaction() {
                let mut live = BTreeSet::new();
                collect_owned_directories(
                    source,
                    &directory,
                    root,
                    &journal,
                    &mut live,
                    &mut (crate::path_seal::MAX_PAYLOAD_NODES - 1),
                )?;
                journal = journal.compact(receipts, entry, live)?;
            }
            (directory, journal)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut journal = receipts.open_writer(entry, true)?;
            if journal.needs_compaction()
                && open_directory(parent, name)
                    .is_err_and(|err| err.kind() == io::ErrorKind::NotFound)
            {
                journal.reset()?;
            }
            let directory = claim_directory(parent, name, root, &mut journal)?;
            (directory, journal)
        }
        Err(_) => return Err(Failure::Occupied),
    };
    if !journal.owns(&key, &directory)? {
        return Err(io::Error::other("restore directory identity changed").into());
    }
    let mut run = DirectoryRestore {
        journal,
        left: crate::path_seal::MAX_PAYLOAD_NODES - 1,
        retained: Vec::new(),
        conflicts: 0,
        first_conflict: None,
        after_file,
    };
    run.write_contents(source, directory, root, 0)
        .map_err(Failure::PartialDirectory)?;
    if let Some(first) = run.first_conflict {
        return Err(Failure::DirectoryConflicts {
            count: run.conflicts,
            first,
        });
    }
    run.journal.finish();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_directory(parent: &impl AsRawHandle, name: &OsStr) -> File {
        relative_entry(
            parent,
            name,
            READ_ENTRY,
            FILE_SHARE_READ,
            FILE_CREATE,
            FILE_DIRECTORY_FILE,
        )
        .unwrap()
    }

    fn set_junction(path: &Path, target: &Path) -> io::Result<()> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_WRITE,
            FILE_WRITE_ATTRIBUTES,
        };
        let handle = std::fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES.0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)?;
        let substitute: Vec<_> = format!(r"\??\{}", target.display())
            .encode_utf16()
            .collect();
        let print: Vec<_> = target.as_os_str().encode_wide().collect();
        let substitute_bytes = std::mem::size_of_val(substitute.as_slice()) as u16;
        let print_bytes = std::mem::size_of_val(print.as_slice()) as u16;
        let data_len = 8 + substitute_bytes + 2 + print_bytes + 2;
        let mut data = 0xa000_0003u32.to_le_bytes().to_vec(); // IO_REPARSE_TAG_MOUNT_POINT
        for field in [
            data_len,
            0,
            0,
            substitute_bytes,
            substitute_bytes + 2,
            print_bytes,
        ] {
            data.extend(field.to_le_bytes());
        }
        for code in substitute.into_iter().chain([0]).chain(print).chain([0]) {
            data.extend(code.to_le_bytes());
        }
        let mut returned = 0;
        unsafe {
            windows::Win32::System::IO::DeviceIoControl(
                HANDLE(handle.as_raw_handle()),
                0x0009_00a4, // FSCTL_SET_REPARSE_POINT
                Some(data.as_ptr().cast()),
                data.len() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        }
        .map_err(io::Error::other)
    }

    #[test]
    fn an_empty_restore_directory_is_guarded_against_in_place_junction_conversion() {
        let root = crate::test_support::TempDir::new("restore-directory-guard");
        let outside = root.subdir("outside");
        let pins = crate::fsutil::pin_local_scan_path(root.path()).unwrap();
        let parent = pins.last().unwrap();
        let unguarded = create_directory(parent, OsStr::new("unguarded"));
        let unguarded_path = root.join("unguarded");
        set_junction(&unguarded_path, &outside).unwrap();
        assert!(crate::fsutil::is_reparse_point(&unguarded_path));
        assert!(guard_restored_directory(&unguarded).is_err());
        drop(unguarded);
        std::fs::remove_dir(unguarded_path).unwrap();

        let guarded = create_directory(parent, OsStr::new("guarded"));
        let guarded_path = root.join("guarded");
        let guard = guard_restored_directory(&guarded).unwrap();
        assert!(set_junction(&guarded_path, &outside).is_err());
        assert!(!crate::fsutil::is_reparse_point(&guarded_path));
        drop(guard);
        assert_eq!(std::fs::read_dir(guarded_path).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    }

    #[test]
    fn completed_children_remain_guarded_until_their_restore_entry_finishes() {
        let root = crate::test_support::TempDir::new("restore-child-guard");
        let outside = root.subdir("outside");
        let source = root.subdir("source");
        std::fs::create_dir(source.join("empty-child")).unwrap();
        let pins = crate::fsutil::pin_local_scan_path(root.path()).unwrap();
        let directory = create_directory(pins.last().unwrap(), OsStr::new("restored"));
        let receipt = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.join("receipt"))
            .unwrap();
        let mut run = DirectoryRestore {
            journal: Journal::open(receipt, File::create(root.join("lock")).unwrap()).unwrap(),
            left: 1,
            retained: Vec::new(),
            conflicts: 0,
            first_conflict: None,
            after_file: |_: &Path| Ok(()),
        };
        run.write_contents(&source, directory, Path::new(""), 0)
            .unwrap();
        let child = root.join("restored/empty-child");
        assert_eq!(run.retained.len(), 2);
        assert!(set_junction(&child, &outside).is_err());
        assert!(!crate::fsutil::is_reparse_point(&child));
        drop(run);
        assert_eq!(std::fs::read_dir(&child).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(root.join("restored")).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    }

    #[test]
    fn a_directory_child_created_after_staging_keeps_its_contents() {
        let root = crate::test_support::TempDir::new("restore-late-child");
        let pins = crate::fsutil::pin_local_scan_path(root.path()).unwrap();
        let parent = pins.last().unwrap();
        let directory = create_directory(parent, OsStr::new("restored"));
        let child = OsStr::new("child.txt");
        let mut staged = temporary_file(&directory, child).unwrap();
        staged.write_all(b"backed-up bytes").unwrap();
        staged.sync_all().unwrap();
        let target = root.join("restored").join(child);
        std::fs::write(&target, b"created after the directory was claimed").unwrap();
        assert_eq!(
            commit_file(&staged, child).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"created after the directory was claimed"
        );
        discard_temporary_file(&staged);
        drop(staged);
        assert_eq!(std::fs::read_dir(root.join("restored")).unwrap().count(), 1);
    }

    #[test]
    fn file_publication_reads_from_a_separate_volume_when_the_workspace_is_on_one() {
        let root = crate::test_support::TempDir::new("restore-target-volume");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let pins = crate::fsutil::pin_local_scan_path(root.path()).unwrap();
        let name = OsStr::new("restored.toml");
        write_new_file(&source, pins.last().unwrap(), name).unwrap();
        assert_eq!(
            std::fs::read(root.join(name)).unwrap(),
            std::fs::read(&source).unwrap()
        );
        eprintln!(
            "copy volumes {:?} -> {:?}",
            source.components().next(),
            root.components().next()
        );
    }

    #[test]
    fn directory_retry_reads_from_a_separate_volume_when_the_workspace_is_on_one() {
        let root = crate::test_support::TempDir::new("restore-directory-volume");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let seal = crate::path_seal::PathMapSeal {
            session: "1700000000_volume".into(),
            map_digest: "test".into(),
            targets: Vec::new(),
            reg_digests: Default::default(),
            file_digests: None,
            file_manifest_digest: None,
        };
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("windows");
        let pins = crate::fsutil::pin_local_scan_path(root.path()).unwrap();
        let parent = pins.last().unwrap();
        let name = OsStr::new("restored");
        let first = publish_directory(
            &source,
            parent,
            name,
            &Receipts::new(&seal),
            "volume",
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "fixture interruption",
                ))
            },
        );
        assert!(
            matches!(first, Err(Failure::PartialDirectory(_))),
            "{first:?}"
        );
        publish(&source, parent, name, &Receipts::new(&seal), "volume").unwrap();
        for file in ["installer.nsi", "installer-hooks.nsh"] {
            assert_eq!(
                std::fs::read(root.join("restored").join(file)).unwrap(),
                std::fs::read(source.join(file)).unwrap()
            );
        }
        eprintln!(
            "directory retry volumes {:?} -> {:?}",
            source.components().next(),
            root.components().next()
        );
    }
}
