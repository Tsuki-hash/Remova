//! Shared filesystem helpers (dedupe backup/restore copy).

use std::path::{Path, PathBuf};

/// Whether already-loaded metadata describes a reparse point. Callers that just
/// stat'ed the entry must use this instead of a second `is_reparse_point` call.
pub fn is_reparse_metadata(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT = 0x400 — covers junctions, not just symlinks.
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// A hard link lets an unprivileged user point a user-writable path at a
/// protected file; an elevated writer must refuse it instead of appending.
pub fn has_multiple_links(p: &Path) -> bool {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, FILE_FLAGS_AND_ATTRIBUTES,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_MODE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        let Some(path_str) = p.to_str() else {
            // A path we cannot spell is not one we will write through.
            return true;
        };
        // A file that does not exist yet has no link to hijack; the first
        // append is allowed to create it. Existing files fail closed below.
        if !p.exists() {
            return false;
        }
        unsafe {
            let wide = wide_path_for_create_file(path_str);
            let Ok(handle) = CreateFileW(
                PCWSTR(wide.as_ptr()),
                0,
                FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(
                    FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0,
                ),
                None,
            ) else {
                // Cannot prove single-link ownership: refuse the write.
                return true;
            };
            let mut info = std::mem::zeroed();
            let ok = GetFileInformationByHandle(handle, &mut info).is_ok();
            let _ = CloseHandle(handle);
            !ok || info.nNumberOfLinks > 1
        }
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(p)
            .map(|m| m.nlink() > 1)
            .unwrap_or(true)
    }
}

/// Refuse to create or write through a path whose existing ancestors include a
/// reparse point. Shared ProgramData directories can be pre-planted by a local
/// user; an elevated writer must not follow that junction into another tree.
pub fn ensure_no_reparse_ancestors(p: &Path) -> std::io::Result<()> {
    for ancestor in p.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match std::fs::symlink_metadata(ancestor) {
            Ok(meta) if is_reparse_metadata(&meta) => {
                return Err(std::io::Error::other(format!(
                    "reparse point in path ancestors: {}",
                    ancestor.display()
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Resolve each component relative to the preceding verified directory handle.
/// Sharing pins alone do NOT prevent FILE_WRITE_ATTRIBUTES from converting an
/// empty directory to a junction. Never reuse these pins to authorize path IO.
pub fn pin_local_scan_path(path: &Path) -> std::io::Result<Vec<DirPin>> {
    validate_local_scan_path(path)?;
    let mut pins = Vec::new();
    for component in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let pin = if let Some(parent) = pins.last() {
            open_relative(
                parent,
                component
                    .file_name()
                    .ok_or_else(|| std::io::Error::other("invalid component"))?,
                0x81,
                false,
            )?
        } else {
            pin_scan_component(component)?
        };
        pins.push(pin);
    }
    Ok(pins)
}

pub(crate) fn local_directory_id(path: &Path) -> std::io::Result<(u32, u64)> {
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let pins = pin_local_scan_path(path)?;
        let pin = pins
            .last()
            .ok_or_else(|| std::io::Error::other("empty path"))?;
        if !pin_metadata(pin)?.is_dir() {
            return Err(std::io::Error::other("expected a directory"));
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(pin.handle, &mut info) }
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        if index == 0 {
            return Err(std::io::Error::other(
                "volume did not report a file identity",
            ));
        }
        Ok((info.dwVolumeSerialNumber, index))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(std::io::Error::other("directory identity requires Windows"))
    }
}

#[cfg(windows)]
fn pin_metadata(pin: &DirPin) -> std::io::Result<std::fs::Metadata> {
    use std::os::windows::io::FromRawHandle;
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(pin.handle.0) });
    file.metadata()
}
#[cfg(not(windows))]
fn pin_metadata(_: &DirPin) -> std::io::Result<std::fs::Metadata> {
    Err(std::io::Error::other(
        "handle-relative scanning requires Windows",
    ))
}

/// One name only, no traversal. OBJ_DONT_REPARSE prevents native name resolution
/// from reparsing; OPEN_REPARSE_POINT opens the final link object, not its target.
fn open_relative(
    parent: &DirPin,
    name: &std::ffi::OsStr,
    access: u32,
    allow_reparse: bool,
) -> std::io::Result<DirPin> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PWSTR;
        use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
        use windows::Wdk::Storage::FileSystem::{
            NtCreateFile, FILE_OPEN, NTCREATEFILE_CREATE_OPTIONS,
        };
        use windows::Win32::Foundation::{HANDLE, UNICODE_STRING};
        use windows::Win32::Storage::FileSystem::{
            FILE_ACCESS_RIGHTS, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE,
        };
        let mut wide: Vec<u16> = name.encode_wide().collect();
        if wide.is_empty()
            || wide.len() > 255
            || wide.iter().any(|c| matches!(*c, 0 | 47 | 92 | 58))
            || name == "."
            || name == ".."
        {
            return Err(std::io::Error::other("invalid relative name"));
        }
        let unicode = UNICODE_STRING {
            Length: (wide.len() * 2) as u16,
            MaximumLength: (wide.len() * 2) as u16,
            Buffer: PWSTR(wide.as_mut_ptr()),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.handle,
            ObjectName: &unicode,
            Attributes: 0x40 | 0x1000,
            ..Default::default()
        };
        let mut handle = HANDLE::default();
        let mut status = windows::Win32::System::IO::IO_STATUS_BLOCK::default();
        let result = unsafe {
            NtCreateFile(
                &mut handle,
                FILE_ACCESS_RIGHTS(access | 0x0010_0000),
                &attributes,
                &mut status,
                None,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                FILE_SHARE_MODE(1),
                FILE_OPEN,
                NTCREATEFILE_CREATE_OPTIONS(0x0020_0000 | 0x20),
                None,
                0,
            )
        };
        if result.0 < 0 {
            let kind = match result.0 as u32 {
                0xc0000034 | 0xc000003a | 0xc000000f => std::io::ErrorKind::NotFound,
                _ => std::io::ErrorKind::Other,
            };
            return Err(std::io::Error::new(
                kind,
                format!("relative open NTSTATUS {:08x}", result.0 as u32),
            ));
        }
        let pin = DirPin {
            handle,
            guard: None,
        };
        if !allow_reparse && is_reparse_metadata(&pin_metadata(&pin)?) {
            return Err(std::io::Error::other("relative open refuses reparse point"));
        }
        Ok(pin)
    }
    #[cfg(not(windows))]
    {
        let _ = (parent, name, access, allow_reparse);
        Err(std::io::Error::other(
            "handle-relative scanning requires Windows",
        ))
    }
}

pub(crate) fn scan_path_metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {
    let pins = pin_local_scan_path(path)?;
    pin_metadata(
        pins.last()
            .ok_or_else(|| std::io::Error::other("empty handle chain"))?,
    )
}

pub fn local_scan_is_dir(path: &Path) -> bool {
    scan_path_metadata(path).is_ok_and(|md| md.is_dir())
}

pub fn read_local_scan_link(path: &Path) -> std::io::Result<PathBuf> {
    #[cfg(windows)]
    {
        let pins = pin_local_scan_path(
            path.parent()
                .ok_or_else(|| std::io::Error::other("no parent"))?,
        )?;
        let link = open_relative(
            pins.last().unwrap(),
            path.file_name()
                .ok_or_else(|| std::io::Error::other("no link name"))?,
            0x81,
            true,
        )?;
        let mut buffer = [0u8; 16384];
        let mut returned = 0;
        unsafe {
            windows::Win32::System::IO::DeviceIoControl(
                link.handle,
                0x900a8,
                None,
                0,
                Some(buffer.as_mut_ptr().cast()),
                buffer.len() as u32,
                Some(&mut returned),
                None,
            )
        }
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        let tag = u32::from_le_bytes(buffer[..4].try_into().unwrap());
        let start = match tag {
            0xa0000003 => 16,
            0xa000000c => 20,
            _ => return Err(std::io::Error::other("unknown reparse tag")),
        };
        let offset = u16::from_le_bytes(buffer[8..10].try_into().unwrap()) as usize;
        let len = u16::from_le_bytes(buffer[10..12].try_into().unwrap()) as usize;
        if !len.is_multiple_of(2) || start + offset + len > returned as usize {
            return Err(std::io::Error::other("invalid reparse data"));
        }
        let units: Vec<u16> = buffer[start + offset..start + offset + len]
            .chunks(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        let target = String::from_utf16(&units).map_err(std::io::Error::other)?;
        // Normalize only a local NT DOS prefix; UNC stays a rejected remote form.
        let target = if let Some(rest) = target.strip_prefix(r"\??\") {
            if let Some(unc) = rest.strip_prefix(r"UNC\") {
                format!(r"\\{unc}")
            } else {
                rest.to_string()
            }
        } else {
            target
        };
        Ok(target.into())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(std::io::Error::other("Windows handle required"))
    }
}

fn validate_local_scan_path(path: &Path) -> std::io::Result<()> {
    let denied = || std::io::Error::other("scan requires an ordinary absolute local path");
    let raw = path.to_str().ok_or_else(denied)?;
    #[cfg(windows)]
    {
        let candidate = local_drive_candidate(raw).ok_or_else(denied)?;
        if raw.trim() != raw
            || raw.contains('"')
            || candidate[3..]
                .split('\\')
                .any(|s| s == "." || s == ".." || s.ends_with(['.', ' ']) || s.contains(':'))
        {
            return Err(denied());
        }
        // Deliberately bypass the drive/type and prefix caches used for UI hints.
        let root = to_wide(&candidate[..3]);
        let ty = unsafe {
            windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(root.as_ptr()))
        };
        if !matches!(ty, 2 | 3 | 5 | 6) {
            return Err(denied());
        }
    }
    #[cfg(not(windows))]
    if !path.is_absolute() || raw.contains("..") {
        return Err(denied());
    }
    Ok(())
}

fn pin_scan_component(path: &Path) -> std::io::Result<DirPin> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::IntoRawHandle;
        let file = std::fs::OpenOptions::new()
            .access_mode(0x81) // READ_DATA/LIST_DIRECTORY + ATTRIBUTES: enforce WRITE sharing
            .share_mode(1) // No ordinary write/rename; attribute-only mutation is handled by relative IO
            .custom_flags(0x0220_0000) // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
            .open(path)?;
        if is_reparse_metadata(&file.metadata()?) {
            return Err(std::io::Error::other("scan refuses reparse point"));
        }
        Ok(DirPin {
            handle: windows::Win32::Foundation::HANDLE(file.into_raw_handle()),
            guard: None,
        })
    }
    #[cfg(not(windows))]
    {
        let md = std::fs::symlink_metadata(path)?;
        if is_reparse_metadata(&md) {
            return Err(std::io::Error::other("scan refuses symlink"));
        }
        Ok(DirPin {})
    }
}

/// Compatibility predicate only. IO callers must retain `pin_local_scan_path`.
pub fn is_safe_local_scan_root(path: &Path) -> bool {
    pin_local_scan_path(path).is_ok()
}

pub fn read_local_scan_text(path: &Path) -> std::io::Result<String> {
    let pins = pin_local_scan_path(path)?;
    read_pin_text(
        pins.last()
            .ok_or_else(|| std::io::Error::other("empty handle chain"))?,
    )
}

/// Open an ordinary local file by its verified final handle, without reopening its path.
#[cfg(windows)]
pub(crate) fn open_local_regular_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::io::FromRawHandle;
    let pins = pin_local_scan_path(path)?;
    let pin = pins
        .last()
        .ok_or_else(|| std::io::Error::other("empty handle chain"))?;
    if !pin_metadata(pin)?.is_file() {
        return Err(std::io::Error::other("not an ordinary local file"));
    }
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(pin.handle.0) });
    file.try_clone()
}

fn read_pin_text(pin: &DirPin) -> std::io::Result<String> {
    #[cfg(windows)]
    {
        use std::io::{Read, Seek, SeekFrom};
        use std::os::windows::io::FromRawHandle;
        let borrowed =
            std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(pin.handle.0) });
        let mut source = borrowed.try_clone()?;
        source.seek(SeekFrom::Start(0))?;
        let mut text = String::new();
        source.take(1_048_577).read_to_string(&mut text)?;
        if text.len() > 1_048_576 {
            return Err(std::io::Error::other("text exceeds scan budget"));
        }
        Ok(text)
    }
    #[cfg(not(windows))]
    {
        let _ = pin;
        Err(std::io::Error::other("Windows handle required"))
    }
}

/// Enumerate the opened directory object, never its potentially redirected path.
fn relative_names(pin: &DirPin) -> std::io::Result<Vec<std::ffi::OsString>> {
    relative_names_with_limit(pin, 20_000)
}

#[cfg(windows)]
pub(crate) fn read_local_entry_names(
    path: &Path,
    limit: usize,
) -> std::io::Result<Vec<std::ffi::OsString>> {
    let pins = pin_local_scan_path(path)?;
    let pin = pins
        .last()
        .ok_or_else(|| std::io::Error::other("empty handle chain"))?;
    relative_names_with_limit(pin, limit)
}

fn relative_names_with_limit(
    pin: &DirPin,
    limit: usize,
) -> std::io::Result<Vec<std::ffi::OsString>> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use windows::Win32::Storage::FileSystem::{
            FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx,
            FILE_ID_BOTH_DIR_INFO,
        };
        let mut names = Vec::new();
        let mut first = true;
        loop {
            let mut buffer = vec![0u64; 8192]; // native structures require 8-byte alignment
            let result = unsafe {
                GetFileInformationByHandleEx(
                    pin.handle,
                    if first {
                        FileIdBothDirectoryRestartInfo
                    } else {
                        FileIdBothDirectoryInfo
                    },
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 8) as u32,
                )
            };
            first = false;
            if let Err(e) = result {
                if e.code().0 as u32 == 0x80070012 {
                    break;
                } // ERROR_NO_MORE_FILES
                return Err(std::io::Error::other(e.to_string()));
            }
            let mut offset = 0;
            loop {
                // FILE_ID_BOTH_DIR_INFO has a variable UTF-16 tail.
                let entry = unsafe {
                    &*(buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<FILE_ID_BOTH_DIR_INFO>())
                };
                let len = entry.FileNameLength as usize / 2;
                let name_offset =
                    (&entry.FileName as *const _ as usize) - (buffer.as_ptr() as usize);
                if entry.FileNameLength % 2 != 0 || name_offset + len * 2 > buffer.len() * 8 {
                    return Err(std::io::Error::other("invalid directory record"));
                }
                let units = unsafe { std::slice::from_raw_parts(entry.FileName.as_ptr(), len) };
                let name = std::ffi::OsString::from_wide(units);
                if name != "." && name != ".." {
                    names.push(name);
                }
                if names.len() > limit {
                    return Err(std::io::Error::other("directory budget exceeded"));
                }
                if entry.NextEntryOffset == 0 {
                    break;
                }
                let next = offset + entry.NextEntryOffset as usize;
                if next <= offset
                    || next + std::mem::size_of::<FILE_ID_BOTH_DIR_INFO>() > buffer.len() * 8
                {
                    return Err(std::io::Error::other("invalid directory offset"));
                }
                offset = next;
            }
        }
        Ok(names)
    }
    #[cfg(not(windows))]
    {
        let _ = (pin, limit);
        Err(std::io::Error::other("Windows handle required"))
    }
}

pub struct LocalScanEntry {
    path: PathBuf,
    metadata: std::fs::Metadata,
    _pin: DirPin,
}
impl LocalScanEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn file_name(&self) -> std::ffi::OsString {
        self.path.file_name().unwrap().into()
    }
    pub fn file_type(&self) -> std::io::Result<std::fs::FileType> {
        Ok(self.metadata.file_type())
    }
    pub fn metadata(&self) -> std::io::Result<std::fs::Metadata> {
        Ok(self.metadata.clone())
    }
}
pub struct LocalScanDir {
    entries: std::vec::IntoIter<std::io::Result<LocalScanEntry>>,
    _pins: Vec<DirPin>,
}
impl Iterator for LocalScanDir {
    type Item = std::io::Result<LocalScanEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next()
    }
}
pub fn read_local_scan_dir(path: &Path) -> std::io::Result<LocalScanDir> {
    let pins = pin_local_scan_path(path)?;
    let parent = pins
        .last()
        .ok_or_else(|| std::io::Error::other("empty handle chain"))?;
    let entries = relative_names(parent)?
        .into_iter()
        .map(|name| {
            let pin = open_relative(parent, &name, 0x81, true)?;
            let metadata = pin_metadata(&pin)?;
            Ok(LocalScanEntry {
                path: path.join(name),
                metadata,
                _pin: pin,
            })
        })
        .collect::<Vec<_>>()
        .into_iter();
    Ok(LocalScanDir {
        entries,
        _pins: pins,
    })
}

/// Missing is only conclusive with held ordinary parents. An inaccessible or
/// redirected manifest is unknown, never evidence of an orphan package.
pub fn local_scan_file_exists(path: &Path) -> std::io::Result<bool> {
    match scan_path_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
        Ok(md) => Ok(md.is_file()),
    }
}

pub fn local_scan_exists(path: &Path) -> std::io::Result<bool> {
    match scan_path_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
        Ok(_) => Ok(true),
    }
}

/// Bounded CLI size estimate. Each descendant is pinned before any IO; unlike
/// a generic size walker this cannot enter a junction swapped in mid-scan.
pub fn local_scan_size(root: &Path) -> (i64, bool) {
    fn walk_pin(
        pin: &DirPin,
        depth: usize,
        left: &mut usize,
        bytes: &mut u64,
    ) -> std::io::Result<()> {
        let md = pin_metadata(pin)?;
        if is_reparse_metadata(&md) {
            return Err(std::io::Error::other("size refuses reparse"));
        }
        if md.is_file() {
            *bytes = bytes.saturating_add(md.len());
        } else if md.is_dir() {
            for name in relative_names(pin)? {
                if *left == 0 || depth >= 8 || crate::scan_task::cancelled() {
                    return Err(std::io::Error::other("size capped"));
                }
                *left -= 1;
                let child = open_relative(pin, &name, 0x81, false)?;
                walk_pin(&child, depth + 1, left, bytes)?;
            }
        }
        Ok(())
    }
    fn walk(path: &Path, depth: usize, left: &mut usize, bytes: &mut u64) -> std::io::Result<()> {
        if *left == 0 || depth > 8 || crate::scan_task::cancelled() {
            return Err(std::io::Error::other("size scan capped"));
        }
        *left -= 1;
        let pins = pin_local_scan_path(path)?;
        let pin = pins
            .last()
            .ok_or_else(|| std::io::Error::other("empty handle chain"))?;
        walk_pin(pin, depth, left, bytes)?;
        Ok(())
    }
    let mut bytes = 0;
    let capped = walk(root, 0, &mut 5000, &mut bytes).is_err();
    (bytes.div_ceil(1024).min(i64::MAX as u64) as i64, capped)
}

/// Windows reparse point (junction / mount / symlink). Never follow when copying.
pub fn is_reparse_point(p: &Path) -> bool {
    std::fs::symlink_metadata(p)
        .map(|m| is_reparse_metadata(&m))
        .unwrap_or(false)
}

/// Deterministic candidate order for directory scans.
///
/// `read_dir` order is filesystem-dependent, so any `take(n)` applied during
/// enumeration — or any result cap applied while pushing — silently drops a
/// different subset on different machines, and on the same machine across runs.
/// Callers collect everything first, order it here, then truncate.
pub fn order_candidates(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_cached_key(|p| p.to_string_lossy().to_lowercase());
    paths.dedup();
    paths
}

/// Recursively copy a directory tree (files + dirs). Symlinks and other reparse points are skipped.
pub fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    copy_dir_before_file(src, dest, &mut |_| Ok(()))
}

/// Create one level at a time, holding every existing/created ancestor against
/// replacement. Never follow a junction planted above an output directory.
pub fn create_dirs_pinned(p: &Path) -> std::io::Result<Vec<DirPin>> {
    let mut pins = vec![];
    let ancestors: Vec<&Path> = p.ancestors().collect();
    let total = ancestors.len();
    // regression fix: a drive root has no parent to hold the next component, so the
    // "ancestors are non-empty by construction" argument does not apply to it —
    // never drop a marker into `C:\` when it is itself the target.
    let is_drive_root = !target_can_be_marked(p);
    for (index, ancestor) in ancestors.into_iter().rev().enumerate() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if !ancestor.exists() {
            std::fs::create_dir(ancestor)?;
        }
        let mut pin = pin_dir_with_access(ancestor, 0x0081)?;
        // only the output directory itself is guarded even when it is
        // non-empty. Ancestors (including `C:\` and `%ProgramData%`) get a
        // marker only when they are actually empty — a marker left behind by a
        // crash must never land in a shared system directory.
        pin.guard_empty(index + 1 == total && !is_drive_root)?;
        pins.push(pin);
    }
    Ok(pins)
}

fn copy_dir_before_file(
    src: &Path,
    dest: &Path,
    before_file: &mut impl FnMut(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    // Per-level pin (same as the delete path): a child swapped for a junction
    // between the reparse check and this open is refused, not followed.
    let mut pin = pin_dir_with_access(src, 0x0081)?;
    pin.guard_empty(true)?;
    let _dest_pins = create_dirs_pinned(dest)?;
    for entry in std::fs::read_dir(src)? {
        if crate::executor::cleanup_cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cleanup cancelled",
            ));
        }
        let entry = entry?;
        #[cfg(windows)]
        if pin
            .guard
            .as_ref()
            .is_some_and(|(name, _)| entry.file_name() == std::ffi::OsStr::new(name))
        {
            continue;
        }
        let path = entry.path();
        // junction/mount reparse: `file_type().is_symlink()` is false on Windows — must use attributes.
        if is_reparse_point(&path) {
            continue;
        }
        let ty = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_before_file(&path, &target, before_file)?;
        } else if ty.is_file() {
            // the reparse check above is a path stat — a child
            // swapped for a link before `fs::copy` re-opens the path would be
            // followed. The dedicated primitive opens by non-following handle.
            before_file(&path)?;
            copy_file_no_reparse(&path, &target)?;
        }
    }
    Ok(())
}

/// Copy a single file; refuse reparse sources so `fs::copy` cannot follow a swapped junction.
/// Stream `reader` into a sibling tmp file, fsync, then atomically replace
/// `dest`. A mid-copy failure leaves any pre-existing `dest` bytes intact and
/// removes the tmp — never truncate-in-place.
fn copy_stream_atomic<R: std::io::Read>(mut reader: R, dest: &Path) -> std::io::Result<u64> {
    // pid+nonce tmp: a fixed sibling name let a second Remova process (or a
    // concurrent restore in-process) truncate and interleave into the same
    // tmp file, tearing the restored/backed-up bytes.
    let (tmp, mut out) = create_atomic_tmp_with(|| atomic_tmp_path(dest))?;
    let result = (|| {
        use std::io::Write;
        let mut copied = 0u64;
        let mut chunk = [0u8; 64 * 1024];
        loop {
            if crate::executor::cleanup_cancelled() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "cleanup cancelled",
                ));
            }
            let n = match reader.read(&mut chunk) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            if n == 0 {
                break;
            }
            out.write_all(&chunk[..n])?;
            copied += n as u64;
        }
        out.sync_all()?;
        if crate::executor::cleanup_cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cleanup cancelled",
            ));
        }
        drop(out);
        rename_with_retry(&tmp, dest)?;
        Ok(copied)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn copy_file_no_reparse(src: &Path, dest: &Path) -> std::io::Result<u64> {
    copy_file_no_reparse_impl(src, dest, true)
}

/// Copy into a directory tree the caller already pinned. Used by restore so a
/// multi-file session does not re-open the whole ancestor chain per file.
pub fn copy_file_no_reparse_pinned(src: &Path, dest: &Path) -> std::io::Result<u64> {
    copy_file_no_reparse_impl(src, dest, false)
}

fn copy_file_no_reparse_impl(src: &Path, dest: &Path, pin_parent: bool) -> std::io::Result<u64> {
    if crate::executor::cleanup_cancelled() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "cleanup cancelled",
        ));
    }
    if is_reparse_point(src) {
        return Err(std::io::Error::other("refusing to copy reparse point"));
    }
    // Read through a handle that never follows a link swapped in after the
    // check (streamed — no whole-file buffering).
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(src)?;
        use std::os::windows::io::AsRawHandle;
        let mut source_info =
            windows::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
        unsafe {
            windows::Win32::Storage::FileSystem::GetFileInformationByHandle(
                windows::Win32::Foundation::HANDLE(f.as_raw_handle()),
                &mut source_info,
            )
        }
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        if source_info.dwFileAttributes & (0x400 | 0x10) != 0 {
            return Err(std::io::Error::other(
                "refusing reparse/directory copy source",
            ));
        }
        let parent = dest
            .parent()
            .ok_or_else(|| std::io::Error::other("missing copy parent"))?;
        let _parent_pins = if pin_parent {
            Some(create_dirs_pinned(parent)?)
        } else {
            None
        };
        if is_reparse_point(dest) {
            return Err(std::io::Error::other(
                "refusing reparse/directory copy destination",
            ));
        }
        copy_stream_atomic(&mut f, dest)
    }
    #[cfg(not(windows))]
    {
        let _ = pin_parent;
        copy_stream_atomic(std::fs::File::open(src)?, dest)
    }
}

/// a handle that pins a directory (or file) while it is being
/// cleared. Opened with FILE_FLAG_OPEN_REPARSE_POINT — a swapped-in junction
/// is opened as the link itself, never its target — and a share mode WITHOUT
/// FILE_SHARE_DELETE, so the path cannot be renamed away while pinned: the
/// object verified below is exactly the object that will be removed.
pub struct DirPin {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(windows)]
    guard: Option<(String, std::fs::File)>,
}

impl DirPin {
    fn guard_empty(&mut self, mark_even_if_non_empty: bool) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            self.guard = guard_empty_directory(self.handle, mark_even_if_non_empty)?;
        }
        Ok(())
    }
}

/// A marker name that is unique per attempt and always short (A10 keeps it
/// testable): pid + nanosecond stamp + attempt offset.
#[cfg(windows)]
fn guard_marker_name(attempt: u32) -> String {
    format!(
        ".remova-guard-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            + attempt as u128
    )
}

/// Raw NtCreateFile errors carry an NTSTATUS-derived HRESULT (`0xD0000022`
/// for ACCESS_DENIED), not the Win32 `0x80070005` that `GetLastError` would
/// return (A4). Accept both so the fallback is reachable and its intent clear.
#[cfg(windows)]
fn is_access_denied_error(error: &windows::core::Error) -> bool {
    matches!(error.code().0 as u32, 0x8007_0005 | 0xD000_0022)
}

/// An empty directory can be changed into a junction even while a handle pins
/// its name. Create an exclusive delete-on-close marker through that verified
/// handle, so the directory cannot become empty under us. Never resolve its
/// pathname a second time.
///
/// `mark_even_if_non_empty`: the directory may be emptied by this operation, so
/// mark it even when it is currently non-empty. Ancestors pass `false` and get
/// no marker, so a shared system directory like `C:\` or `%ProgramData%` is
/// never written to.
#[cfg(windows)]
pub(crate) fn guard_empty_directory(
    handle: windows::Win32::Foundation::HANDLE,
    mark_even_if_non_empty: bool,
) -> std::io::Result<Option<(String, std::fs::File)>> {
    use std::os::windows::io::FromRawHandle;
    use windows::core::PWSTR;
    use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows::Wdk::Storage::FileSystem::{
        NtCreateFile, FILE_CREATE, FILE_DELETE_ON_CLOSE, FILE_NON_DIRECTORY_FILE,
        FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_OPTIONS,
    };
    use windows::Win32::Foundation::{HANDLE, UNICODE_STRING};
    use windows::Win32::Storage::FileSystem::*;
    use windows::Win32::System::IO::IO_STATUS_BLOCK;
    // only the operation's own target is marked. Ancestors are non-empty
    // by construction (they hold the next path component), and writing a marker
    // into `C:\` or `%ProgramData%` just to learn they are non-empty would need
    // list access the pin does not have.
    if !mark_even_if_non_empty {
        return Ok(None);
    }
    // regression fix: the marker name carries a nanosecond stamp, but two threads can
    // still stamp in the same tick; a name collision is worth one retry with a
    // fresh stamp rather than failing the whole operation.
    for attempt in 0..3u32 {
        let name = guard_marker_name(attempt);
        let mut wide: Vec<u16> = name.encode_utf16().collect();
        let unicode = UNICODE_STRING {
            Length: (wide.len() * 2) as u16,
            MaximumLength: (wide.len() * 2) as u16,
            Buffer: PWSTR(wide.as_mut_ptr()),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: handle,
            ObjectName: &unicode,
            Attributes: 0x40 | 0x1000,
            ..Default::default()
        };
        let mut marker = HANDLE::default();
        let mut status = IO_STATUS_BLOCK::default();
        let marker_result = unsafe {
            NtCreateFile(
                &mut marker,
                FILE_ACCESS_RIGHTS(0x0011_0082),
                &attributes,
                &mut status,
                None,
                FILE_FLAGS_AND_ATTRIBUTES(0x102),
                FILE_SHARE_MODE(0),
                FILE_CREATE,
                NTCREATEFILE_CREATE_OPTIONS(
                    FILE_DELETE_ON_CLOSE.0
                        | FILE_NON_DIRECTORY_FILE.0
                        | FILE_OPEN_REPARSE_POINT.0
                        | FILE_SYNCHRONOUS_IO_NONALERT.0,
                ),
                None,
                0,
            )
            .ok()
        };
        match marker_result {
            Ok(()) => {
                return Ok(Some((name, unsafe {
                    std::fs::File::from_raw_handle(marker.0)
                })))
            }
            // This process cannot create the marker, which usually means the
            // directory's ACL denies it. That does not strictly prove a swap is
            // impossible (a creator-owner ACL could allow another process to
            // empty and convert the target), but refusing here would break
            // backups into protected roots; the name pin remains the backstop.
            Err(e) if is_access_denied_error(&e) => return Ok(None),
            Err(_) if attempt + 1 < 3 => continue,
            Err(e) => return Err(std::io::Error::other(e.to_string())),
        }
    }
    unreachable!("the retry loop always returns")
}

impl Drop for DirPin {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
impl std::os::windows::io::AsRawHandle for DirPin {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.handle.0
    }
}

/// Reserve deletion and security inspection together; active staging pins refuse this open.
#[cfg(windows)]
pub(crate) fn pin_private_delete(p: &Path) -> std::io::Result<DirPin> {
    pin_dir_with_access(p, 0x0003_0081)
}

/// Open `p` and verify it is not a reparse point — by handle, not by path.
pub fn pin_dir_no_reparse(p: &Path) -> std::io::Result<DirPin> {
    pin_dir_with_access(p, 0x0001_0080)
}

/// Validate a locked target with read access so denied DELETE sharing is enforced.
pub fn pin_target_readonly(p: &Path) -> std::io::Result<DirPin> {
    pin_dir_with_access(p, 0x0081)
}

fn pin_dir_with_access(p: &Path, desired_access: u32) -> std::io::Result<DirPin> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, FILE_FLAGS_AND_ATTRIBUTES,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_MODE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        // DELETE (0x00010000) is not exported by this windows crate build (same
        // as regops); FILE_READ_ATTRIBUTES (0x0080) for the by-handle check.
        // lossy conversion would pin a U+FFFD-replaced path — fail closed.
        let Some(path_str) = p.to_str() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains unpaired surrogates",
            ));
        };
        unsafe {
            let wide = wide_path_for_create_file(path_str);
            // Denying DELETE sharing prevents later rename/delete opens.
            let handle = CreateFileW(
                PCWSTR(wide.as_ptr()),
                desired_access,
                FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(
                    FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0,
                ),
                None,
            )
            .map_err(|e| std::io::Error::other(e.to_string()))?;
            let mut info = std::mem::zeroed();
            if GetFileInformationByHandle(handle, &mut info).is_err()
                || info.dwFileAttributes & 0x400 != 0
            {
                let _ = CloseHandle(handle);
                return Err(std::io::Error::other(
                    "refusing to delete through reparse point",
                ));
            }
            Ok(DirPin {
                handle,
                guard: None,
            })
        }
    }
    #[cfg(not(windows))]
    {
        let _ = desired_access;
        if is_reparse_point(p) {
            return Err(std::io::Error::other(
                "refusing to delete through reparse point",
            ));
        }
        Ok(DirPin {})
    }
}

/// A path can carry a guard marker only when it has a parent directory. Drive
/// roots and UNC share roots cannot be converted to a junction in place and
/// must never receive a marker; a pure unit test pins this.
fn target_can_be_marked(p: &Path) -> bool {
    p.parent().is_some()
}

/// A NUL-terminated UTF-16 path for a raw `CreateFileW`.
///
/// Raw Win32 calls do not get std's automatic `\\?\` conversion, so a
/// destination whose absolute length passes MAX_PATH failed even when every
/// component was valid (A2). Short paths keep their ordinary form; long
/// absolute paths are promoted to an extended-length form. Relative paths are
/// left alone — they cannot be made verbatim without changing their meaning.
#[cfg(windows)]
fn wide_path_for_create_file(raw: &str) -> Vec<u16> {
    let path = raw.replace('/', "\\");
    let long = path.encode_utf16().count() >= 240;
    let promoted = if !long || path.starts_with(r"\\?\") {
        path
    } else if let Some(rest) = path.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{rest}")
    } else if path.as_bytes().get(1) == Some(&b':') {
        format!(r"\\?\{path}")
    } else {
        path
    };
    promoted.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A NUL-terminated UTF-16 path for a raw Win32 security call.
///
/// Like [`wide_path_for_create_file`], the path is promoted to an
/// extended-length form once it approaches MAX_PATH: `SetFileSecurityW` has the
/// same limit and otherwise fails with a bogus "invalid name" error (123) on a
/// deep backup tree that every component of which is valid.
#[cfg(windows)]
pub fn wide_path_for_security(path: &Path) -> Vec<u16> {
    let raw = path.to_string_lossy();
    let promoted = raw.replace('/', "\\");
    let long = promoted.encode_utf16().count() >= 240;
    let promoted = if !long || promoted.starts_with(r"\\?\") {
        promoted
    } else if let Some(rest) = promoted.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{rest}")
    } else if promoted.as_bytes().get(1) == Some(&b':') {
        format!(r"\\?\{promoted}")
    } else {
        promoted
    };
    promoted.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A pin on a directory plus its by-handle resolved final path. While held,
/// the pinned object cannot be renamed away (no FILE_SHARE_DELETE), and the
/// caller can re-run string gates against the *resolved* location instead of
/// the requested one (restore write-back hardening).
pub struct DirResolvedPin {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    final_path: String,
}

impl DirResolvedPin {
    pub fn final_path(&self) -> &str {
        &self.final_path
    }
}

impl Drop for DirResolvedPin {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// Open `p` (following reparse chains, unlike [`pin_dir_no_reparse`]) and
/// return the held pin plus the resolved final path. The caller compares the
/// resolved path against the requested one: a mismatch means a junction or
/// symlink was in the chain and the write must not proceed.
pub fn pin_dir_resolved(p: &Path) -> std::io::Result<DirResolvedPin> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, GetFinalPathNameByHandleW,
            FILE_ATTRIBUTE_DIRECTORY, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, GETFINALPATHNAMEBYHANDLE_FLAGS,
            OPEN_EXISTING,
        };
        const FILE_READ_ATTR: u32 = 0x0000_0081; // attributes + directory listing (sharing is enforced)
        let Some(path_str) = p.to_str() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains unpaired surrogates",
            ));
        };
        unsafe {
            let wide = wide_path_for_create_file(path_str);
            // No FILE_FLAG_OPEN_REPARSE_POINT: the handle must land on the
            // chain target so the final path exposes any planted junction.
            let handle = CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_READ_ATTR,
                FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(FILE_FLAG_BACKUP_SEMANTICS.0),
                None,
            )
            .map_err(|e| std::io::Error::other(e.to_string()))?;
            let mut info = std::mem::zeroed();
            if GetFileInformationByHandle(handle, &mut info).is_err()
                || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
            {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return Err(std::io::Error::other("restore parent is not a directory"));
            }
            // windows 0.58 binding: (HANDLE, &mut [u16], flags) — an empty
            // slice makes the API return the required length (incl. NUL).
            let need =
                GetFinalPathNameByHandleW(handle, &mut [], GETFINALPATHNAMEBYHANDLE_FLAGS(0));
            if need == 0 {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return Err(std::io::Error::other("final path query failed"));
            }
            let mut buf = vec![0u16; need as usize];
            let written =
                GetFinalPathNameByHandleW(handle, &mut buf, GETFINALPATHNAMEBYHANDLE_FLAGS(0));
            if written == 0 || written as usize > buf.len() {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return Err(std::io::Error::other("final path query failed"));
            }
            buf.truncate(written as usize);
            while buf.last().copied() == Some(0) {
                buf.pop();
            }
            let raw = String::from_utf16_lossy(&buf);
            // `\\?\C:\...` → `C:\...`, `\\?\UNC\srv\share` → `\\srv\share`.
            let final_path = raw
                .strip_prefix(r"\\?\UNC\")
                .map(|rest| format!(r"\\{rest}"))
                .or_else(|| raw.strip_prefix(r"\\?\").map(str::to_string))
                .unwrap_or(raw);
            Ok(DirResolvedPin { handle, final_path })
        }
    }
    #[cfg(not(windows))]
    {
        if is_reparse_point(p) {
            return Err(std::io::Error::other("refusing reparse restore parent"));
        }
        Ok(DirResolvedPin {
            final_path: p.to_string_lossy().into_owned(),
        })
    }
}

/// long (expanded) form of an existing path — `GetLongPathNameW`
/// turns 8.3 components back into their long names so restore-target
/// comparisons can match a short-name request against the resolved form.
#[cfg(windows)]
pub fn long_path_form(p: &Path) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetLongPathNameW;
    let s = p.to_str()?;
    let wide = to_wide(s);
    unsafe {
        let need = GetLongPathNameW(PCWSTR(wide.as_ptr()), None);
        if need == 0 {
            return None;
        }
        let mut buf = vec![0u16; need as usize];
        let written = GetLongPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf));
        if written == 0 || written as usize > buf.len() {
            return None;
        }
        while buf.last().copied() == Some(0) {
            buf.pop();
        }
        Some(String::from_utf16_lossy(&buf))
    }
}

/// companion: short (8.3) form of an existing path, for tests that
/// exercise short-name handling (`None`/identity when the volume has 8.3
/// name generation disabled).
#[cfg(windows)]
pub fn short_path_form(p: &Path) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let s = p.to_str()?;
    let wide = to_wide(s);
    unsafe {
        let need = GetShortPathNameW(PCWSTR(wide.as_ptr()), None);
        if need == 0 {
            return None;
        }
        let mut buf = vec![0u16; need as usize];
        let written = GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf));
        if written == 0 || written as usize > buf.len() {
            return None;
        }
        while buf.last().copied() == Some(0) {
            buf.pop();
        }
        Some(String::from_utf16_lossy(&buf))
    }
}

/// Delete a tree without following reparse points.
/// Every level is pinned (no-DELETE-share handle + by-handle reparse check)
/// before its children are cleared, so the path cannot be swapped for a
/// junction mid-recursion; child junctions are unlinked as links only.
pub fn remove_tree_no_reparse(p: &Path) -> std::io::Result<()> {
    let _parents = pin_existing_parents(p)?;
    remove_tree_pinned(p)
}

/// Hold every ancestor from the volume root before resolving a deletion path.
/// Do not create missing ancestors or follow junctions in the chain.
pub fn pin_existing_parents(p: &Path) -> std::io::Result<Vec<DirPin>> {
    if !p.is_absolute() {
        return Err(std::io::Error::other("deletion requires an absolute path"));
    }
    let mut pins = Vec::new();
    for ancestor in p
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let mut pin = pin_dir_with_access(ancestor, 0x0081)?;
        pin.guard_empty(false)?;
        pins.push(pin);
    }
    Ok(pins)
}

fn remove_tree_pinned(p: &Path) -> std::io::Result<()> {
    remove_tree_with_pin(p, pin_dir_no_reparse(p)?)
}

/// Consume the already verified deletion pin without reopening against its sharing mode.
pub(crate) fn remove_tree_with_pin(p: &Path, mut pin: DirPin) -> std::io::Result<()> {
    let is_dir = {
        let meta = std::fs::symlink_metadata(p)?;
        if meta.is_dir() {
            pin.guard_empty(true)?;
            for entry in std::fs::read_dir(p)? {
                let entry = entry?;
                #[cfg(windows)]
                if pin
                    .guard
                    .as_ref()
                    .is_some_and(|(name, _)| entry.file_name() == std::ffi::OsStr::new(name))
                {
                    continue;
                }
                let child = entry.path();
                if is_reparse_point(&child) {
                    // Unlink the reparse itself (file link / dir junction) without descending.
                    if remove_file_with_readonly_retry(&child).is_err() {
                        remove_dir_with_readonly_retry(&child)?;
                    }
                    continue;
                }
                let ty = entry.file_type()?;
                if ty.is_dir() {
                    remove_tree_pinned(&child)?;
                } else {
                    remove_file_with_readonly_retry(&child)?;
                }
            }
            true
        } else {
            false
        }
    };
    drop(pin);
    // Pin is dropped first: removing needs a DELETE-open, which our own
    // no-DELETE-share pin would otherwise block. The window left here is
    // benign — a swapped-in junction link or empty dir is removed as-is,
    // never followed.
    if is_dir {
        remove_dir_with_readonly_retry(p)
    } else {
        remove_file_with_readonly_retry(p)
    }
}

/// An immutable, non-following snapshot held from authorization through backup
/// and deletion. Unlike `remove_tree_with_pin` (whose final unlink is path based),
/// this consumes the exact verified objects by handle. Windows only: the Unix
/// DirPin implementation does not offer the required sharing/identity contract.
pub(crate) struct PinnedLocalDeleteTree {
    path: PathBuf,
    is_dir: bool,
    objects: Vec<(PathBuf, bool, DirPin)>,
    _parents: Vec<DirPin>,
}

impl PinnedLocalDeleteTree {
    pub(crate) fn new(path: &Path) -> std::io::Result<Self> {
        Self::new_with(path, &mut |_| Ok(()))
    }

    fn new_with(
        path: &Path,
        after_open: &mut impl FnMut(&Path) -> std::io::Result<()>,
    ) -> std::io::Result<Self> {
        Self::collect_with(path, 20_000, after_open)
    }

    pub(crate) fn with_entry_limit(path: &Path, limit: usize) -> std::io::Result<Self> {
        Self::collect_with(path, limit, &mut |_| Ok(()))
    }

    fn collect_with(
        path: &Path,
        limit: usize,
        after_open: &mut impl FnMut(&Path) -> std::io::Result<()>,
    ) -> std::io::Result<Self> {
        let _parents = pin_local_scan_path(
            path.parent()
                .ok_or_else(|| std::io::Error::other("cannot delete a volume root"))?,
        )?;
        let root = open_relative(
            _parents
                .last()
                .ok_or_else(|| std::io::Error::other("empty handle chain"))?,
            path.file_name()
                .ok_or_else(|| std::io::Error::other("invalid target"))?,
            0x10081,
            false,
        )?;
        let mut objects = Vec::new();
        fn collect(
            path: &Path,
            pin: DirPin,
            depth: usize,
            remaining: &mut usize,
            objects: &mut Vec<(PathBuf, bool, DirPin)>,
            after_open: &mut impl FnMut(&Path) -> std::io::Result<()>,
        ) -> std::io::Result<bool> {
            if depth > 128 || *remaining == 0 {
                return Err(std::io::Error::other("tree snapshot budget exceeded"));
            }
            *remaining -= 1;
            let md = pin_metadata(&pin)?;
            if is_reparse_metadata(&md) || !(md.is_dir() || md.is_file()) {
                return Err(std::io::Error::other(
                    "package snapshot refuses reparse/special entry",
                ));
            }
            let is_dir = md.is_dir();
            after_open(path)?;
            // An empty object may have become a junction at this exact point.
            // Enumeration and every child open still use that object handle.
            if is_dir {
                for name in relative_names_with_limit(&pin, *remaining)? {
                    let child = open_relative(&pin, &name, 0x10081, false)?;
                    collect(
                        &path.join(name),
                        child,
                        depth + 1,
                        remaining,
                        objects,
                        after_open,
                    )?;
                }
            }
            // Postorder storage; removal consumes children before their parents.
            objects.push((path.into(), is_dir, pin));
            Ok(is_dir)
        }
        let mut remaining = limit;
        let is_dir = collect(path, root, 0, &mut remaining, &mut objects, after_open)?;
        objects.reverse();
        Ok(Self {
            path: path.into(),
            is_dir,
            objects,
            _parents,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn is_dir(&self) -> bool {
        self.is_dir
    }

    pub(crate) fn contains_file(&self, path: &Path) -> bool {
        self.objects.iter().any(|(p, dir, _)| p == path && !dir)
    }

    pub(crate) fn read_text(&self, path: &Path) -> std::io::Result<String> {
        let (_, _, pin) = self
            .objects
            .iter()
            .find(|(p, dir, _)| p == path && !dir)
            .ok_or_else(|| std::io::Error::other("file not in held snapshot"))?;
        read_pin_text(pin)
    }

    /// Verify the counterpart of each held object. The returned read pins
    /// keep the verified target files stable until the caller releases them.
    #[cfg(windows)]
    pub(crate) fn hold_verified_copy(
        &self,
        destination: &Path,
        exact: bool,
        mut compare: impl FnMut(std::fs::File, std::fs::File) -> std::io::Result<bool>,
    ) -> std::io::Result<Vec<DirPin>> {
        use std::os::windows::io::FromRawHandle;
        fn clone_file(pin: &DirPin) -> std::io::Result<std::fs::File> {
            let borrowed = std::mem::ManuallyDrop::new(unsafe {
                std::fs::File::from_raw_handle(pin.handle.0)
            });
            borrowed.try_clone()
        }

        let mut pins = pin_local_scan_path(destination)?;
        let target_root_index = pins.len() - 1;
        let mut positions = std::collections::BTreeMap::new();
        let mut children = std::collections::BTreeMap::new();
        for (path, is_dir, backup) in &self.objects {
            if path != &self.path {
                let parent_path = path
                    .parent()
                    .ok_or_else(|| std::io::Error::other("missing snapshot parent"))?;
                if exact {
                    *children.entry(parent_path).or_insert(0) += 1;
                }
                let parent = positions
                    .get(parent_path)
                    .copied()
                    .ok_or_else(|| std::io::Error::other("missing snapshot parent"))?;
                let name = path
                    .file_name()
                    .ok_or_else(|| std::io::Error::other("missing entry name"))?;
                pins.push(
                    open_relative(&pins[parent], name, 0x81, false).map_err(|err| {
                        std::io::Error::other(format!("{}: {err}", path.display()))
                    })?,
                );
            }
            let index = pins
                .len()
                .checked_sub(1)
                .ok_or_else(|| std::io::Error::other("missing target snapshot"))?;
            let target = &pins[index];
            let metadata = pin_metadata(target)?;
            let same_type = if *is_dir {
                metadata.is_dir()
            } else {
                metadata.is_file()
            };
            if is_reparse_metadata(&metadata)
                || !same_type
                || !compare(clone_file(backup)?, clone_file(target)?)?
            {
                return Err(std::io::Error::other(format!(
                    "{}: target entry differs",
                    path.display()
                )));
            }
            if *is_dir {
                positions.insert(path.clone(), index);
            }
        }
        if exact {
            for (directory, index) in positions {
                let expected = children.get(directory.as_path()).copied().unwrap_or(0);
                let actual = relative_names_with_limit(&pins[index], expected + 1)?.len();
                if actual != expected {
                    return Err(std::io::Error::other(format!(
                        "{}: target directory entries differ",
                        directory.display()
                    )));
                }
            }
        }
        // The target objects are still held. Release the ancestors above the
        // target so a sibling rename (migration rollback) can open its parent.
        pins.drain(..target_root_index);
        Ok(pins)
    }

    /// Copy the exact held file handles, never reopening a source pathname.
    pub(crate) fn copy_to(&self, destination: &Path) -> std::io::Result<()> {
        for (path, is_dir, pin) in &self.objects {
            let rel = path
                .strip_prefix(&self.path)
                .map_err(std::io::Error::other)?;
            let dest = if rel.as_os_str().is_empty() {
                destination.into()
            } else {
                destination.join(rel)
            };
            if *is_dir {
                std::fs::create_dir_all(&dest)?;
            } else {
                #[cfg(windows)]
                {
                    use std::os::windows::io::FromRawHandle;
                    // Borrow ownership only for try_clone; the DirPin owns the original.
                    let borrowed = std::mem::ManuallyDrop::new(unsafe {
                        std::fs::File::from_raw_handle(pin.handle.0)
                    });
                    let mut source = borrowed.try_clone()?;
                    use std::io::{Seek, SeekFrom};
                    source.seek(SeekFrom::Start(0))?;
                    let mut out = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&dest)?;
                    std::io::copy(&mut source, &mut out)?;
                    out.sync_all()?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn remove(mut self) -> std::io::Result<()> {
        // Children close before their parents, which must be empty for disposition.
        while let Some((_path, _is_dir, pin)) = self.objects.pop() {
            #[cfg(windows)]
            {
                use windows::Win32::Storage::FileSystem::{
                    FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
                };
                let info = FILE_DISPOSITION_INFO {
                    DeleteFile: true.into(),
                };
                unsafe {
                    SetFileInformationByHandle(
                        pin.handle,
                        FileDispositionInfo,
                        (&info as *const FILE_DISPOSITION_INFO).cast(),
                        std::mem::size_of_val(&info) as u32,
                    )
                }
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
            drop(pin);
        }
        Ok(())
    }
}

/// SOP §9.3: a READONLY attribute makes delete fail like a lock. Clear
/// READONLY/HIDDEN/SYSTEM on the target and retry once — never on a reparse
/// point, and never outside the tree being removed.
fn remove_file_with_readonly_retry(p: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(first) => {
            if should_clear_attributes(&first, delete_blocking_attributes(p)) {
                clear_attributes_for_delete(p);
                std::fs::remove_file(p)
            } else {
                Err(first)
            }
        }
    }
}

fn remove_dir_with_readonly_retry(p: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir(p) {
        Ok(()) => Ok(()),
        Err(first) => {
            if should_clear_attributes(&first, delete_blocking_attributes(p)) {
                clear_attributes_for_delete(p);
                std::fs::remove_dir(p)
            } else {
                Err(first)
            }
        }
    }
}

/// Only an ACCESS_DENIED on an object that actually carries READONLY/HIDDEN/
/// SYSTEM is an attribute problem. Sharing violations (32/33) and ACL-only
/// failures must not silently strip attributes from a file we fail to delete.
fn should_clear_attributes(error: &std::io::Error, attributes: u32) -> bool {
    // Never rewrite a reparse link's attributes; the caller unlinks it as-is.
    error.raw_os_error() == Some(5) && attributes & 0x7 != 0 && attributes & 0x400 == 0
}

#[cfg(windows)]
fn delete_blocking_attributes(p: &Path) -> u32 {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(p)
        .map(|m| m.file_attributes())
        .unwrap_or(0)
}

#[cfg(not(windows))]
fn delete_blocking_attributes(_p: &Path) -> u32 {
    0
}

#[cfg(windows)]
fn clear_attributes_for_delete(p: &Path) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_NORMAL};
    let Some(text) = p.to_str() else {
        return false;
    };
    let wide = to_wide(text);
    unsafe { SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_NORMAL).is_ok() }
}

#[cfg(not(windows))]
fn clear_attributes_for_delete(_p: &Path) -> bool {
    false
}

/// CSV field escape: wrap in quotes when needed; double internal quotes.
/// OWASP CSV injection: a leading `= + - @` (or tab) executes as a spreadsheet
/// formula when the export is opened in Excel — neutralize with a leading
/// apostrophe before any quoting so it stays part of the cell value.
pub fn csv_escape(s: &str) -> String {
    let safe = if matches!(
        s.chars().next(),
        Some('=' | '+' | '-' | '@' | '\t' | '\r' | '\n')
    ) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if safe.contains(',') || safe.contains('"') || safe.contains('\n') || safe.contains('\r') {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

/// FNV-1a 64 — unique enough for cache/backup names (not crypto).
pub fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Strict UTF-8 path string (backslashes). `None` on non-UTF-8 — callers must fail closed.
pub fn path_utf8(p: &Path) -> Option<String> {
    Some(p.to_str()?.replace('/', "\\"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathProbe {
    Safe,
    Missing,
    Reparse,
    Error,
}

pub(crate) fn local_drive_candidate(raw: &str) -> Option<String> {
    let s = raw.trim().trim_matches('"').replace('/', "\\");
    let b = s.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
        return None;
    }
    if s.starts_with("\\\\") || s.starts_with("\\\\?\\") || s.starts_with("\\\\.\\") {
        return None;
    }
    if s.split('\\').any(|seg| seg == "..") {
        return None;
    }
    Some(s)
}

/// Injectable policy core: tests model drive classification and reparse probes
/// without querying mapped shares or touching any network.
fn is_local_drive_path_with(
    raw: &str,
    drive_is_local: impl FnOnce(&str) -> bool,
    mut probe: impl FnMut(&str) -> PathProbe,
) -> bool {
    let Some(path) = local_drive_candidate(raw) else {
        return false;
    };
    let root = &path[..3];
    if !drive_is_local(root) {
        return false;
    }

    // Inspect each existing prefix from the local root outward. Rejecting every
    // reparse point is conservative, and prevents opening a later component
    // through a junction that could target UNC/WebDAV without contacting it.
    let mut prefix = root.to_string();
    for component in path[3..].split('\\').filter(|part| !part.is_empty()) {
        prefix.push_str(component);
        match probe(&prefix) {
            PathProbe::Safe => {}
            PathProbe::Missing => return true,
            PathProbe::Reparse | PathProbe::Error => return false,
        }
        prefix.push('\\');
    }
    true
}

/// Per-thread drive-type cache for `is_local_drive_path` (M1 hot-path item).
///
/// Batch scans classify thousands of candidate paths on the same handful of
/// drive roots, and each classification used to pay a `GetDriveTypeW` syscall.
/// A drive's shape rarely changes; 30s of staleness bounds USB hot-plug within
/// one scan run, mirroring the service-snapshot batch reuse. Thread-local so
/// scan workers never contend on it.
#[cfg(windows)]
mod drive_cache {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };

    const TTL: Duration = Duration::from_secs(30);

    thread_local! {
        static CACHE: RefCell<HashMap<String, (u32, Instant)>> =
            RefCell::new(HashMap::new());
    }

    /// Cache core, separated so the TTL is testable with a fake clock and a
    /// counting query instead of the real syscall.
    pub(super) fn get<F: FnMut(&str) -> u32>(
        cache: &mut HashMap<String, (u32, Instant)>,
        root: &str,
        now: Instant,
        mut query: F,
    ) -> u32 {
        let key = root.to_lowercase();
        if let Some(&(ty, stamped)) = cache.get(&key) {
            if now.duration_since(stamped) < TTL {
                return ty;
            }
        }
        let ty = query(&key);
        cache.insert(key, (ty, now));
        ty
    }

    pub(super) fn drive_type(root: &str) -> u32 {
        CACHE.with(|c| {
            let mut map = c.borrow_mut();
            get(&mut map, root, Instant::now(), |key| {
                let wide = super::to_wide(key);
                unsafe {
                    windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(
                        wide.as_ptr(),
                    ))
                }
            })
        })
    }

    // Prefix-probe cache: same batch-reuse rationale as the drive type — a
    // batch scan probes the same directory prefixes thousands of times, and
    // each miss was a `GetFileAttributesW`. Only stable topology outcomes
    // (Safe/Reparse/Missing) are cached; `Error` is transient by nature and
    // always re-queried. 30s TTL bounds a reparse-point created mid-batch.
    // These probes gate read-side classification, never the delete path.
    thread_local! {
        static PROBE_CACHE: RefCell<HashMap<String, (super::PathProbe, Instant)>> =
            RefCell::new(HashMap::new());
    }

    /// Testable core of the probe cache. `None` from the query means a
    /// transient error: not cached.
    pub(super) fn cached_probe<F: FnOnce(&str) -> Option<super::PathProbe>>(
        cache: &mut HashMap<String, (super::PathProbe, Instant)>,
        prefix: &str,
        now: Instant,
        query: F,
    ) -> super::PathProbe {
        let key = prefix.to_lowercase();
        if let Some(&(probe, stamped)) = cache.get(&key) {
            if now.duration_since(stamped) < TTL {
                return probe;
            }
        }
        let Some(probe) = query(&key) else {
            return super::PathProbe::Error;
        };
        cache.insert(key, (probe, now));
        probe
    }

    pub(super) fn probe(prefix: &str) -> super::PathProbe {
        PROBE_CACHE.with(|c| {
            let mut map = c.borrow_mut();
            cached_probe(&mut map, prefix, Instant::now(), |key| {
                let wide = super::to_wide(key);
                let attrs = unsafe { GetFileAttributesW(PCWSTR(wide.as_ptr())) };
                if attrs != INVALID_FILE_ATTRIBUTES {
                    if attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                        Some(super::PathProbe::Reparse)
                    } else {
                        Some(super::PathProbe::Safe)
                    }
                } else {
                    match unsafe { GetLastError().0 } {
                        2 | 3 => Some(super::PathProbe::Missing),
                        _ => None,
                    }
                }
            })
        })
    }
}

/// True only for an absolute path on a local volume with no reparse point in
/// its existing prefix chain. Registry-controlled read probes (icons, sizes,
/// Explorer) must not touch UNC/device/verbatim shares or mapped drives,
/// otherwise rendering a list can trigger SMB/WebDAV authentication. Local
/// junctions are conservatively refused too, so a later component is never
/// opened through a redirection whose target could be remote.
pub fn is_local_drive_path(raw: &str) -> bool {
    #[cfg(windows)]
    {
        const DRIVE_REMOVABLE: u32 = 2;
        const DRIVE_FIXED: u32 = 3;
        const DRIVE_REMOTE: u32 = 4;
        const DRIVE_CDROM: u32 = 5;
        const DRIVE_RAMDISK: u32 = 6;

        is_local_drive_path_with(
            raw,
            |root| {
                let drive_type = drive_cache::drive_type(root);
                drive_type != DRIVE_REMOTE
                    && matches!(
                        drive_type,
                        DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_RAMDISK | DRIVE_CDROM
                    )
            },
            drive_cache::probe,
        )
    }
    #[cfg(not(windows))]
    {
        is_local_drive_path_with(raw, |_| true, |_| PathProbe::Safe)
    }
}

/// The single definition of "these two paths name the same thing".
///
/// Four call sites used to carry their own copy of this logic
/// (`scan_allow::normalize`, `orphans::normalize_orphan_key`,
/// `regops::normalize_path_entry`, `safety::path_utf8_lower`). Three of them were
/// byte-for-byte identical, and none of those three stripped Win32's per-segment
/// trailing dots and spaces — the one property that decides whether
/// `C:\Windows.`, `C:\Windows ` and `c:\windows` collide.
///
/// The consequence was fail-closed by luck rather than by design: a scan that
/// reported `C:\Foo\Bar.` authorized the key `c:\foo\bar.`, so the delete request
/// for `C:\Foo\Bar` was refused — the user is told an item exists and then cannot
/// remove it. The reverse direction is worse to reason about, because the red-line
/// checks in `safety` *do* strip those characters while the allow-list did not, so
/// the two halves of the gate disagreed about what a path even was. Unifying them
/// makes that agreement intentional.
///
/// `None` on non-UTF-8 input: a path we cannot name is a path we must not match.
pub fn path_key(p: &Path) -> Option<String> {
    let raw = p.to_str()?;
    let slashed = raw.replace('/', "\\");
    let unquoted = slashed.trim().trim_matches('"');
    let lowered = unquoted.to_lowercase();
    let body = lowered.trim_end_matches('\\');
    // `.` and `..` are kept verbatim so traversal checks still see them; every
    // other segment is trimmed the way Win32 trims it when resolving.
    let key = body
        .split('\\')
        .map(|seg| {
            if seg == "." || seg == ".." {
                seg
            } else {
                seg.trim_end_matches(['.', ' '])
            }
        })
        .collect::<Vec<_>>()
        .join("\\");
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

/// Shared `to_wide` for Windows APIs.
#[cfg(windows)]
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Names passed as PCWSTR must have exactly the same extent as policy input.
pub fn validate_native_name(name: &str) -> Result<(), String> {
    if name.contains('\0') {
        return Err(crate::error::safety_err("native name contains NUL").to_ipc());
    }
    Ok(())
}

#[cfg(windows)]
pub fn to_wide_name(name: &str) -> Result<Vec<u16>, String> {
    validate_native_name(name)?;
    Ok(to_wide(name))
}

/// Decode a REG_SZ / REG_EXPAND_SZ UTF-16LE blob, trimming the trailing NUL.
#[cfg(windows)]
pub fn wstring_from_reg_data(data: &[u8]) -> String {
    if data.len() < 2 {
        return String::new();
    }
    let mut u16s: Vec<u16> = Vec::with_capacity(data.len() / 2);
    let mut i = 0;
    while i + 1 < data.len() {
        u16s.push(u16::from_le_bytes([data[i], data[i + 1]]));
        i += 2;
    }
    while u16s.last().copied() == Some(0) {
        u16s.pop();
    }
    String::from_utf16_lossy(&u16s)
}

/// Whether a candidate directory holds any payload worth cleaning.
///
/// Registry-driven cache scanners select directories by *name* (e.g. any
/// `*-updater` under LocalAppData). An updater leaves its directory behind as an
/// empty shell once it has applied its update, so a name match alone reports
/// dozens of 0 KB rows the user cannot act on. A directory counts as non-empty
/// only when it contains a regular file, directly or in a descendant subtree —
/// empty subdirectories are still an empty shell.
///
/// Bounded: `max_entries` caps entries visited per directory and `max_depth`
/// caps descent, so a pathological tree cannot stall a scan. The answer is
/// conservative in the safe direction: an unreadable level, a truncated level,
/// and queued subtrees beyond the depth cap all answer `true` (keep the row)
/// rather than silently discarding a real cache.
pub fn dir_has_payload(root: &Path, max_entries: usize, max_depth: usize) -> bool {
    let mut level: Vec<PathBuf> = vec![root.to_path_buf()];
    for _ in 0..=max_depth {
        let mut next: Vec<PathBuf> = Vec::new();
        for dir in level {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                // Unreadable: do not claim emptiness we cannot prove.
                return true;
            };
            for (seen, entry) in rd.flatten().enumerate() {
                if seen >= max_entries {
                    // Truncated level: assume there may be payload below.
                    return true;
                }
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                // A reparse child is a link, not payload of this directory.
                if is_reparse_metadata(&meta) {
                    continue;
                }
                if meta.is_dir() {
                    next.push(entry.path());
                } else if meta.is_file() && meta.len() > 0 {
                    return true;
                }
            }
        }
        if next.is_empty() {
            // Every queued subtree was read and held no file.
            return false;
        }
        level = next;
    }
    // Depth cap reached with subtrees still unexplored: assume there may be
    // payload below rather than silently discarding a real cache.
    true
}

/// M2 (SOP §8-22): remove empty ancestor shells left after deleting an app
/// directory (e.g. `Program Files\Vendor`). Only directories that are empty,
/// below one of the known roots, not reparse points, and whose name matches a
/// product/publisher token are removed; the root itself is never touched.
pub fn prune_empty_ancestors(path: &Path, tokens: &[String]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for env in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "PROGRAMDATA",
        "LOCALAPPDATA",
        "APPDATA",
    ] {
        if let Some(v) = std::env::var_os(env) {
            if let Some(key) = path_key(Path::new(&v)) {
                roots.push(PathBuf::from(key));
            }
        }
    }
    prune_empty_ancestors_under(path, tokens, &roots)
}

fn prune_empty_ancestors_under(path: &Path, tokens: &[String], roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let normalized: Vec<String> = tokens
        .iter()
        .map(|t| crate::scanner::normalize_for_match(t))
        .filter(|t| t.len() >= 3)
        .collect();
    if normalized.is_empty() {
        return removed;
    }
    let Some(mut current) = path.parent().map(Path::to_path_buf) else {
        return removed;
    };
    while let Some(dir_key) = path_key(&current) {
        let inside_root = roots.iter().any(|root| {
            let root = root.to_string_lossy();
            dir_key != root.as_ref() && dir_key.starts_with(&format!("{root}\\"))
        });
        if !inside_root {
            break;
        }
        if is_reparse_point(&current) {
            break;
        }
        let name = crate::scanner::normalize_for_match(
            &current
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        );
        if !normalized.iter().any(|t| t == &name) {
            break;
        }
        let mut entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => break,
        };
        if entries.next().is_some() {
            break;
        }
        if std::fs::remove_dir(&current).is_err() {
            break;
        }
        removed.push(current.clone());
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }
    removed
}

/// Sibling temp name for an atomic replace.
///
/// The name deliberately does **not** embed the destination file name: Windows
/// caps one path component at 255 UTF-16 units, and appending
/// `.<pid>.<nonce>.tmp` to an already-truncated 255-unit backup name made the
/// temp component invalid — the exact failure this naming rule prevents (A1).
/// pid + counter keeps the name unique across processes and threads, and it
/// stays in the destination directory so the rename remains atomic.
pub fn atomic_tmp_path(p: &Path) -> PathBuf {
    static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.with_file_name(format!(".remova-tmp-{}-{nonce}", std::process::id()))
}

pub(crate) fn create_atomic_tmp(p: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    create_atomic_tmp_with(|| atomic_tmp_path(p))
}

fn create_atomic_tmp_with(
    mut next_path: impl FnMut() -> PathBuf,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    for _ in 0..32 {
        let tmp = next_path();
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(file) => return Ok((tmp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary file collisions",
    ))
}

/// Rename over an existing file, retrying the two transient Windows failures.
///
/// Antivirus/indexers open a just-written file for a moment, and `rename` then
/// fails with `ERROR_SHARING_VIOLATION` (32) or `ERROR_ACCESS_DENIED` (5) even
/// though nothing in this process holds it. The bounded retry keeps the atomic
/// replace atomic while surviving that scan window; every other error is
/// returned immediately.
fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut delay = std::time::Duration::from_millis(5);
    for attempt in 0..6 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 5 && matches!(error.raw_os_error(), Some(5) | Some(32)) => {
                std::thread::sleep(delay);
                delay *= 2;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the loop returns on the last attempt")
}

/// Write bytes to a unique tmp sibling, fsync, then rename over `p` — a
/// crash mid-write never tears `p`, and a failed write leaves no tmp.
pub fn write_bytes_atomic(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let (tmp, mut f) = create_atomic_tmp_with(|| atomic_tmp_path(p))?;
    let result = (|| -> std::io::Result<()> {
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        rename_with_retry(&tmp, p)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests;
