//! Write an authenticated restore entry without replacing any live object.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use windows::core::PWSTR;
use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FileRenameInformation, NtCreateFile, NtSetInformationFile, FILE_CREATE, FILE_DIRECTORY_FILE,
    FILE_NON_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION,
    FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_OPTIONS,
};
use windows::Win32::Foundation::{HANDLE, NTSTATUS, STATUS_OBJECT_NAME_COLLISION, UNICODE_STRING};
use windows::Win32::Storage::FileSystem::{
    FileDispositionInfo, SetFileInformationByHandle, DELETE, FILE_ACCESS_RIGHTS,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO,
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_NONE, FILE_SHARE_READ, FILE_WRITE_DATA,
    SYNCHRONIZE,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;

pub(super) enum Failure {
    Occupied,
    PartialDirectory(io::Error),
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
    status.ok().map_err(io::Error::other)
}

fn create_entry(parent: &impl AsRawHandle, name: &OsStr, directory: bool) -> io::Result<File> {
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
    let read = FILE_READ_DATA.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;
    let (access, share, kind) = if directory {
        (read, FILE_SHARE_READ, FILE_DIRECTORY_FILE)
    } else {
        (
            read | FILE_WRITE_DATA.0 | DELETE.0,
            FILE_SHARE_NONE,
            FILE_NON_DIRECTORY_FILE,
        )
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
            FILE_CREATE,
            NTCREATEFILE_CREATE_OPTIONS(
                kind.0 | FILE_OPEN_REPARSE_POINT.0 | FILE_SYNCHRONOUS_IO_NONALERT.0,
            ),
            None,
            0,
        )
    })?;
    Ok(unsafe { File::from_raw_handle(handle.0) })
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

fn discard_temporary_file(file: &File) {
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
        match create_entry(parent, name, false) {
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
        }
        temporary.sync_all()?;
        crate::fsutil::reject_named_data_streams(&source)?;
        crate::fsutil::reject_named_data_streams(&temporary)?;
        check_cancelled()?;
        commit_file(&temporary, name)
    })();
    if result.is_err() {
        discard_temporary_file(&temporary);
    }
    result
}

fn write_new_directory_contents(
    source: &Path,
    directory: File,
    left: &mut usize,
    depth: usize,
    retained: &mut Vec<(File, File)>,
) -> io::Result<()> {
    let guard = guard_restored_directory(&directory)?;
    let source_pin = crate::fsutil::pin_target_readonly(source)?;
    source_pin.check_data_streams()?;
    for name in crate::fsutil::read_local_entry_names(source, *left)? {
        check_cancelled()?;
        if *left == 0 || depth >= 64 {
            return Err(io::Error::other(
                "restore directory exceeds verification limit",
            ));
        }
        *left -= 1;
        let child = source.join(&name);
        let metadata = crate::fsutil::scan_path_metadata(&child)?;
        if metadata.is_dir() {
            let dest = create_entry(&directory, &name, true)?;
            write_new_directory_contents(&child, dest, left, depth + 1, retained)?;
        } else {
            write_new_file(&child, &directory, &name)?;
        }
    }
    source_pin.check_data_streams()?;
    // Keep the exact directory and its marker pinned after its recursion
    // returns, until the whole restore entry has finished publishing.
    retained.push((directory, guard));
    Ok(())
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

pub(super) fn publish(
    source: &Path,
    parent: &impl AsRawHandle,
    name: &OsStr,
) -> Result<(), Failure> {
    check_cancelled()?;
    if crate::fsutil::scan_path_metadata(source)?.is_dir() {
        let directory = create_entry(parent, name, true)?;
        let mut left = crate::path_seal::MAX_PAYLOAD_NODES - 1;
        let mut retained = Vec::new();
        write_new_directory_contents(source, directory, &mut left, 0, &mut retained)
            .map_err(Failure::PartialDirectory)
    } else {
        write_new_file(source, parent, name).map_err(Failure::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let unguarded = create_entry(parent, OsStr::new("unguarded"), true).unwrap();
        let unguarded_path = root.join("unguarded");
        set_junction(&unguarded_path, &outside).unwrap();
        assert!(crate::fsutil::is_reparse_point(&unguarded_path));
        assert!(guard_restored_directory(&unguarded).is_err());
        drop(unguarded);
        std::fs::remove_dir(unguarded_path).unwrap();

        let guarded = create_entry(parent, OsStr::new("guarded"), true).unwrap();
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
        let directory = create_entry(pins.last().unwrap(), OsStr::new("restored"), true).unwrap();
        let mut retained = Vec::new();
        write_new_directory_contents(&source, directory, &mut 1, 0, &mut retained).unwrap();
        let child = root.join("restored/empty-child");
        assert_eq!(retained.len(), 2);
        assert!(set_junction(&child, &outside).is_err());
        assert!(!crate::fsutil::is_reparse_point(&child));
        drop(retained);
        assert_eq!(std::fs::read_dir(&child).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(root.join("restored")).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    }

    #[test]
    fn a_directory_child_created_after_staging_keeps_its_contents() {
        let root = crate::test_support::TempDir::new("restore-late-child");
        let pins = crate::fsutil::pin_local_scan_path(root.path()).unwrap();
        let parent = pins.last().unwrap();
        let directory = create_entry(parent, OsStr::new("restored"), true).unwrap();
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
}
