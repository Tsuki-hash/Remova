//! Shared filesystem helpers (dedupe backup/restore copy).

use std::path::{Path, PathBuf};

/// Windows reparse point (junction / mount / symlink). Never follow when copying.
pub fn is_reparse_point(p: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT = 0x400 — covers junctions, not just symlinks.
        std::fs::symlink_metadata(p)
            .map(|m| m.file_attributes() & 0x400 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::fs::symlink_metadata(p)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }
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
    // R38-4: a drive root has no parent to hold the next component, so the
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
        // F-16: only the output directory itself is guarded even when it is
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
        let n = std::io::copy(&mut reader, &mut out)?;
        out.sync_all()?;
        drop(out);
        std::fs::rename(&tmp, dest)?;
        Ok(n)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn copy_file_no_reparse(src: &Path, dest: &Path) -> std::io::Result<u64> {
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
        let _parent_pins = create_dirs_pinned(
            dest.parent()
                .ok_or_else(|| std::io::Error::other("missing copy parent"))?,
        )?;
        if is_reparse_point(dest) {
            return Err(std::io::Error::other(
                "refusing reparse/directory copy destination",
            ));
        }
        copy_stream_atomic(&mut f, dest)
    }
    #[cfg(not(windows))]
    {
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

/// An empty directory can be changed into a junction even while a handle pins
/// its name. Create an exclusive delete-on-close marker through that verified
/// handle, so the directory cannot become empty under us. Never resolve its
/// pathname a second time.
///
/// `mark_even_if_non_empty`: the directory may be emptied by this operation, so
/// mark it even when it is currently non-empty. Ancestors pass `false` and get
/// no marker, so a shared system directory like `C:\` or `%ProgramData%` is
/// never written to (F-16).
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
    // F-16: only the operation's own target is marked. Ancestors are non-empty
    // by construction (they hold the next path component), and writing a marker
    // into `C:\` or `%ProgramData%` just to learn they are non-empty would need
    // list access the pin does not have.
    if !mark_even_if_non_empty {
        return Ok(None);
    }
    // R38-4: the marker name carries a nanosecond stamp, but two threads can
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
/// must never receive a marker (R38-4, pinned by a pure unit test in A10).
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

/// Delete a tree without following reparse points ( / ).
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
                    if std::fs::remove_file(&child).is_err() {
                        std::fs::remove_dir(&child)?;
                    }
                    continue;
                }
                let ty = entry.file_type()?;
                if ty.is_dir() {
                    remove_tree_pinned(&child)?;
                } else {
                    std::fs::remove_file(&child)?;
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
        std::fs::remove_dir(p)
    } else {
        std::fs::remove_file(p)
    }
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

/// Unique tmp sibling for atomic publish: `p` + `.{pid}.{nonce}.tmp`.
/// The pid keeps a second Remova process from clobbering our tmp; the
/// nonce also unshares concurrent writers of the same target in-process.
/// Sibling temp name for an atomic replace.
///
/// The name deliberately does **not** embed the destination file name: Windows
/// caps one path component at 255 UTF-16 units, and appending
/// `.<pid>.<nonce>.tmp` to an already-truncated 255-unit backup name made the
/// temp component invalid — the exact failure F-13 exists to remove (A1).
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

/// Write bytes to a unique tmp sibling, fsync, then rename over `p` — a
/// crash mid-write never tears `p`, and a failed write leaves no tmp.
pub fn write_bytes_atomic(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let (tmp, mut f) = create_atomic_tmp_with(|| atomic_tmp_path(p))?;
    let result = (|| -> std::io::Result<()> {
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, p)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(windows)]
    fn fixture_junction(link: &Path, target: &Path) {
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .unwrap();
        assert!(status.success(), "junction fixture creation failed");
        assert!(is_reparse_point(link));
    }

    #[test]
    fn atomic_tmp_collisions_never_truncate_or_remove_foreign_files() {
        use std::io::Write;
        let root = unique_tmp("exclusive_tmp");
        let sentinel = root.join("sentinel");
        fs::write(&sentinel, b"original").unwrap();
        let foreign = root.join("foreign.tmp");
        fs::hard_link(&sentinel, &foreign).unwrap();
        let owned = root.join("owned.tmp");
        let mut first = true;
        let (path, mut file) = create_atomic_tmp_with(|| {
            if std::mem::take(&mut first) {
                foreign.clone()
            } else {
                owned.clone()
            }
        })
        .unwrap();
        file.write_all(b"new").unwrap();
        drop(file);
        assert_eq!(path, owned);
        assert_eq!(fs::read(&sentinel).unwrap(), b"original");
        assert_eq!(fs::read(&foreign).unwrap(), b"original");
        let error = create_atomic_tmp_with(|| foreign.clone()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(foreign.exists());
        assert_eq!(fs::read(&sentinel).unwrap(), b"original");
        let destination = root.join("directory");
        fs::create_dir(&destination).unwrap();
        assert!(write_bytes_atomic(&destination, b"cannot publish").is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 4);
        remove_tree_no_reparse(&root).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn reboot_target_pin_accepts_a_read_locked_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!(
            "remova-lock-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, b"locked").unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(pin_dir_no_reparse(&path).is_err());
        let readonly = pin_target_readonly(&path);
        drop(lock);
        let pinned = readonly.unwrap();
        assert!(fs::rename(&path, path.with_extension("moved")).is_err());
        drop(pinned);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn atomic_write_publishes_and_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!("remova-r25-fsutil-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("data.bin");
        write_bytes_atomic(&target, b"one").unwrap();
        write_bytes_atomic(&target, b"two").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"two");
        let leftovers = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0, "failed-or-done writes must not leave tmp");
        // The tmp helper is unique per call even for the same target.
        assert_ne!(atomic_tmp_path(&target), atomic_tmp_path(&target));
        let _ = fs::remove_dir_all(&dir);
    }

    /// A fixture directory that removes itself on drop, including on panic.
    fn unique_tmp(tag: &str) -> crate::test_support::TempDir {
        crate::test_support::TempDir::new(&format!("fsutil_{tag}"))
    }

    /// `path_key` is the one definition of path identity shared by the allow-list,
    /// the PATH-entry comparison and the red-line checks. Each case below is a pair
    /// that Win32 resolves to the same object but that the four hand-rolled
    /// normalizers used to treat as different keys.
    #[test]
    fn path_key_agrees_with_win32_on_the_same_object() {
        let key = |s: &str| path_key(std::path::Path::new(s));
        // Case and slash style.
        assert_eq!(key("C:/Foo/Bar").as_deref(), key("c:\\foo\\bar").as_deref());
        // A trailing separator names the same directory.
        assert_eq!(
            key("C:\\Foo\\Bar\\").as_deref(),
            key("C:\\Foo\\Bar").as_deref()
        );
        // Win32 strips trailing dots and spaces per segment, not just at the end.
        assert_eq!(
            key("C:\\Foo\\Bar.").as_deref(),
            key("C:\\Foo\\Bar").as_deref()
        );
        assert_eq!(
            key("C:\\Foo\\Bar ").as_deref(),
            key("C:\\Foo\\Bar").as_deref()
        );
        assert_eq!(
            key("C:\\Foo. \\Bar. ").as_deref(),
            key("C:\\Foo\\Bar").as_deref()
        );
        // Surrounding whitespace and the quotes a registry PATH value arrives with.
        assert_eq!(
            key("  \"C:\\Foo\\Bar\"  ").as_deref(),
            key("C:\\Foo\\Bar").as_deref()
        );
        // A drive root keeps its colon; it is not collapsed to the empty key.
        assert_eq!(key("C:\\").as_deref(), Some("c:"));
    }

    /// The dot-segment exemption exists so traversal rejection still works — a key
    /// that quietly turned `..` into an empty segment would make `C:\A\..\..\B`
    /// look like `C:\A\B`.
    #[test]
    fn path_key_keeps_dot_segments_for_traversal_checks() {
        let key = |s: &str| path_key(std::path::Path::new(s));
        assert_eq!(key("C:\\A\\..\\..\\B").as_deref(), Some("c:\\a\\..\\..\\b"));
        assert_ne!(
            key("C:\\A\\..\\..\\B").as_deref(),
            key("C:\\A\\B").as_deref()
        );
    }

    /// Distinct paths must stay distinct: the exemptions above must not over-merge.
    #[test]
    fn path_key_does_not_merge_distinct_paths() {
        let key = |s: &str| path_key(std::path::Path::new(s));
        assert_ne!(
            key("C:\\Foo\\Bar").as_deref(),
            key("C:\\Foo\\Bar2").as_deref()
        );
        assert_ne!(
            key("C:\\Foo\\Bar").as_deref(),
            key("C:\\Foo\\Baz").as_deref()
        );
        assert_ne!(key("C:\\Foo").as_deref(), key("C:\\Foo\\Bar").as_deref());
    }

    /// A path that cannot be named cannot be matched, so the caller fails closed
    /// rather than comparing on a lossy replacement.
    #[test]
    fn path_key_is_none_for_unnameable_and_empty_input() {
        use std::os::windows::ffi::OsStringExt;
        let lone_surrogate: std::ffi::OsString =
            std::ffi::OsString::from_wide(&[0xD800, b'x' as u16]);
        assert!(path_key(std::path::Path::new(&lone_surrogate)).is_none());
        assert!(path_key(std::path::Path::new("")).is_none());
        assert!(path_key(std::path::Path::new("   ")).is_none());
        assert!(path_key(std::path::Path::new("\"\"")).is_none());
    }

    #[test]
    fn copy_file_no_reparse_refuses_missing_is_ok_error() {
        let tmp = unique_tmp("nofile");
        let missing = tmp.join("nope.bin");
        assert!(copy_file_no_reparse(&missing, &tmp.join("out.bin")).is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A failing mid-stream copy must never truncate or replace the destination.
    #[test]
    fn copy_stream_failure_preserves_dest_and_leaves_no_tmp() {
        struct FailAfter {
            left: usize,
        }
        impl std::io::Read for FailAfter {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.left == 0 {
                    return Err(std::io::Error::other("injected mid-copy failure"));
                }
                let n = self.left.min(buf.len()).min(4);
                buf[..n].fill(b'N');
                self.left -= n;
                Ok(n)
            }
        }
        let tmp = unique_tmp("atomic_copy");
        let dest = tmp.join("out.bin");
        fs::write(&dest, b"ORIGINAL").unwrap();
        let err = copy_stream_atomic(FailAfter { left: 8 }, &dest).unwrap_err();
        assert!(err.to_string().contains("injected"), "{err}");
        assert_eq!(
            fs::read(&dest).unwrap(),
            b"ORIGINAL",
            "dest must stay intact"
        );
        let leftovers: Vec<_> = fs::read_dir(&tmp)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "no tmp leftovers: {leftovers:?}");
        let _ = fs::remove_dir_all(&tmp);
    }

    /// Successful replace is atomic: dest gets the new bytes, no tmp remains.
    #[test]
    fn copy_stream_success_replaces_dest_without_tmp() {
        let tmp = unique_tmp("atomic_copy_ok");
        let dest = tmp.join("out.bin");
        fs::write(&dest, b"OLD").unwrap();
        let n = copy_stream_atomic(&b"NEWER"[..], &dest).unwrap();
        assert_eq!(n, 5);
        assert_eq!(fs::read(&dest).unwrap(), b"NEWER");
        assert!(!tmp.join("out.bin.removal-tmp").exists());
        assert!(!tmp.join("out.bin.removal-tmp").exists());
        let leftovers: Vec<_> = fs::read_dir(&tmp)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "no tmp leftovers: {leftovers:?}");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires Windows symlink privilege or Developer Mode; run explicitly on the Windows CI runner"]
    fn copy_dir_refuses_file_swapped_to_symlink_after_enumeration() {
        let tmp = unique_tmp("swap_file");
        let source = tmp.join("source");
        let dest = tmp.join("dest");
        fs::create_dir(&source).unwrap();
        let secret = tmp.join("secret.bin");
        fs::write(&secret, b"must never be copied").unwrap();
        let child = source.join("child.bin");
        fs::write(&child, b"safe").unwrap();
        let mut swapped = false;
        let result = copy_dir_before_file(&source, &dest, &mut |path| {
            fs::remove_file(path)?;
            std::os::windows::fs::symlink_file(&secret, path)?;
            swapped = true;
            Ok(())
        });
        // No skip/return: an unavailable fixture is a visible failed test.
        assert!(
            swapped,
            "symlink fixture requires developer mode or symlink privilege: {result:?}"
        );
        assert!(result.is_err());
        assert!(!dest.join("child.bin").exists());
        assert_eq!(fs::read(&secret).unwrap(), b"must never be copied");
        fs::remove_file(child).unwrap();
        fs::remove_file(secret).unwrap();
        fs::remove_dir(source).unwrap();
        fs::remove_dir(dest).unwrap();
        fs::remove_dir(tmp).unwrap();
    }

    #[test]
    fn remove_tree_no_reparse_refuses_root_reparse() {
        let tmp = unique_tmp("rm_reparse");
        let link = tmp.join("link");
        let target = tmp.join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("f.txt"), b"x").unwrap();
        fixture_junction(&link, &target);
        {
            assert!(remove_tree_no_reparse(&link).is_err());
            assert!(
                target.join("f.txt").exists(),
                "must not delete through link"
            );
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    /// the pin itself must refuse reparse points and actually pin
    /// (rename of a pinned directory fails; after drop it succeeds again).
    #[test]
    fn dir_pin_refuses_reparse_and_blocks_rename() {
        let tmp = unique_tmp("pin");
        let dir = tmp.join("d");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), b"a").unwrap();

        let pin = pin_dir_no_reparse(&dir).unwrap();
        #[cfg(windows)]
        {
            assert!(
                fs::rename(&dir, tmp.join("d2")).is_err(),
                "pinned dir must not be renameable (no DELETE share)"
            );
        }
        drop(pin);
        #[cfg(windows)]
        fs::rename(&dir, tmp.join("d2")).unwrap();

        let target = tmp.join("t2");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), b"keep").unwrap();
        let junc = tmp.join("junc");
        fixture_junction(&junc, &target);
        {
            assert!(pin_dir_no_reparse(&junc).is_err());
            assert!(target.join("keep.txt").exists());
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remove_tree_no_reparse_unlinks_child_junction_without_following() {
        let tmp = unique_tmp("rm_child");
        let tree = tmp.join("tree");
        let outside = tmp.join("outside");
        fs::create_dir_all(tree.join("ok")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"keep").unwrap();
        fs::write(tree.join("ok").join("a.txt"), b"a").unwrap();
        let junc = tree.join("junc");
        fixture_junction(&junc, &outside);
        remove_tree_no_reparse(&tree).unwrap();
        assert!(!tree.exists());
        assert!(
            outside.join("keep.txt").exists(),
            "junction target must survive"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn copy_dir_skips_reparse_entries() {
        let tmp = unique_tmp("link");
        let src = tmp.join("src");
        let dest = tmp.join("dest");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("ok.txt"), b"x").unwrap();
        let outside = tmp.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"outside").unwrap();
        fixture_junction(&src.join("link.txt"), &outside);
        copy_dir(&src, &dest).unwrap();
        assert!(dest.join("ok.txt").exists());
        assert!(!dest.join("link.txt").exists());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fnv1a64_stable() {
        assert_eq!(fnv1a64("hello"), fnv1a64("hello"));
        assert_ne!(fnv1a64("a"), fnv1a64("b"));
    }

    #[test]
    fn csv_escape_quotes_and_commas() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        // OWASP CSV injection: formula prefixes are neutralized.
        assert_eq!(csv_escape("=cmd|' /C calc'!A0"), "'=cmd|' /C calc'!A0");
        assert_eq!(csv_escape("+sum"), "'+sum");
        assert_eq!(csv_escape("-flag"), "'-flag");
        assert_eq!(csv_escape("@import"), "'@import");
        assert_eq!(csv_escape("\t=1"), "'\t=1");
        assert_eq!(csv_escape("\r=1+1"), "\"'\r=1+1\"");
        assert_eq!(csv_escape("\n=1+1"), "\"'\n=1+1\"");
        // Quoted fields keep the apostrophe inside the cell value.
        assert_eq!(csv_escape("=a,\"b"), "\"'=a,\"\"b\"");
        // Ordinary names (CJK, digits, drive paths) are untouched.
        assert_eq!(csv_escape("7-Zip 压缩"), "7-Zip 压缩");
        assert_eq!(csv_escape(r"C:\Users\a\b"), r"C:\Users\a\b");
    }

    #[test]
    fn path_utf8_fails_closed_on_wide_junk() {
        #[cfg(windows)]
        {
            use std::ffi::OsString;
            use std::os::windows::ffi::OsStringExt;
            let wide: Vec<u16> = vec!['C' as u16, ':' as u16, 0xD800, 'x' as u16];
            let os = OsString::from_wide(&wide);
            assert!(path_utf8(Path::new(&os)).is_none());
        }
        assert_eq!(
            path_utf8(Path::new(r"C:\Foo/Bar")).as_deref(),
            Some(r"C:\Foo\Bar")
        );
    }

    #[test]
    fn copy_dir_roundtrip() {
        let root = unique_tmp("rt");
        let src = root.join("src");
        let dest = root.join("dest");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("a.txt"), b"hello").unwrap();
        fs::write(src.join("nested/b.txt"), b"world").unwrap();
        copy_dir(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(dest.join("nested/b.txt")).unwrap(),
            "world"
        );
        let _ = fs::remove_dir_all(&root);
    }
    /// R38-4 / A10: the root-skip rule is a pure decision, so it can be tested
    /// without pinning a real volume root.
    #[test]
    fn target_can_be_marked_covers_roots_and_unc_shares() {
        assert!(!target_can_be_marked(std::path::Path::new(r"C:\")));
        assert!(!target_can_be_marked(std::path::Path::new("C:/")));
        assert!(!target_can_be_marked(std::path::Path::new(
            r"\\server\share"
        )));
        assert!(target_can_be_marked(std::path::Path::new(r"C:\Temp")));
        assert!(target_can_be_marked(std::path::Path::new(
            r"C:\Temp\nested"
        )));
        assert!(target_can_be_marked(std::path::Path::new("relative")));
    }

    /// A10 / A4: the marker name is bounded, unique per attempt, and still
    /// parses as a hidden file name.
    #[cfg(windows)]
    #[test]
    fn guard_marker_names_are_short_and_distinct() {
        let names: Vec<String> = (0..3).map(guard_marker_name).collect();
        assert_eq!(names.len(), 3);
        assert_ne!(names[0], names[1]);
        assert_ne!(names[1], names[2]);
        for name in &names {
            assert!(name.starts_with(".remova-guard-"));
            assert!(name.encode_utf16().count() < 255);
        }
    }

    /// A2: raw CreateFileW does not get std's verbatim conversion. Long
    /// absolute paths must be promoted so a valid deep destination can still
    /// be pinned; short and relative paths must keep their ordinary form.
    #[cfg(windows)]
    #[test]
    fn wide_path_promotes_only_long_absolute_paths() {
        let short = wide_path_for_create_file(r"C:\Temp\x");
        assert_eq!(
            String::from_utf16(&short[..short.len() - 1]).unwrap(),
            r"C:\Temp\x"
        );

        let long_drive = format!(r"C:\{}", "a".repeat(260));
        let wide = wide_path_for_create_file(&long_drive);
        let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
        assert!(text.starts_with(r"\\?\C:\"), "{text}");

        let long_unc = format!(r"\\server\share\{}", "a".repeat(260));
        let wide = wide_path_for_create_file(&long_unc);
        let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
        assert!(text.starts_with(r"\\?\UNC\server\share\"), "{text}");

        let relative = format!(r"rel\{}", "a".repeat(260));
        let wide = wide_path_for_create_file(&relative);
        let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
        assert_eq!(text, relative.replace('/', "\\"));
    }

    /// A2 e2e: a destination deeper than MAX_PATH must pin successfully, and
    /// its target must carry the marker.
    #[cfg(windows)]
    #[test]
    fn create_dirs_pinned_supports_paths_beyond_max_path() {
        let root = unique_tmp("long_path");
        let deep = root
            .join("a".repeat(140))
            .join("b".repeat(140))
            .join("target");
        assert!(deep.to_string_lossy().len() > 260);
        let pins = create_dirs_pinned(&deep).unwrap();
        assert!(
            std::fs::read_dir(&deep)
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with(".remova-guard")),
            "the deep target must be reachable and marked"
        );
        drop(pins);
    }

    /// R38-11 / F-16: only the operation's own target carries a guard marker.
    /// A crash mid-operation must never leave `.remova-guard-*` in `C:\` or
    /// `%ProgramData%`, and the ancestor chain must stay marker-free.
    #[cfg(windows)]
    #[test]
    fn create_dirs_pinned_marks_only_the_target() {
        let root = unique_tmp("guard_ancestors");
        let deep = root.join("a").join("b").join("target");
        let pins = create_dirs_pinned(&deep).unwrap();
        let has_guard = |dir: &std::path::Path| {
            std::fs::read_dir(dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with(".remova-guard"))
        };
        assert!(has_guard(&deep), "the target must carry the marker");
        for ancestor in [root.join("a"), root.join("a").join("b")] {
            assert!(
                !has_guard(&ancestor),
                "an ancestor must never be marked: {}",
                ancestor.display()
            );
        }
        drop(pins);
        assert!(!has_guard(&deep), "the marker must disappear on close");
    }

    #[cfg(windows)]
    #[test]
    fn directory_pins_block_reparse_mutation_but_allow_child_io() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        fn make_junction(path: &Path, target: &Path) -> windows::core::Result<()> {
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(0x0200_0000)
                .open(path)
                .unwrap();
            let substitute: Vec<u16> = format!("\\??\\{}", target.display())
                .encode_utf16()
                .collect();
            let print: Vec<u16> = target.to_str().unwrap().encode_utf16().collect();
            let mut buffer = 0xa000_0003u32.to_le_bytes().to_vec();
            buffer.extend_from_slice(
                &((8 + (substitute.len() + print.len() + 2) * 2) as u16).to_le_bytes(),
            );
            buffer.extend_from_slice(&0u16.to_le_bytes());
            for value in [
                0,
                substitute.len() * 2,
                (substitute.len() + 1) * 2,
                print.len() * 2,
            ] {
                buffer.extend_from_slice(&(value as u16).to_le_bytes());
            }
            for unit in substitute
                .into_iter()
                .chain(Some(0))
                .chain(print)
                .chain(Some(0))
            {
                buffer.extend_from_slice(&unit.to_le_bytes());
            }
            let mut written = 0;
            unsafe {
                windows::Win32::System::IO::DeviceIoControl(
                    windows::Win32::Foundation::HANDLE(file.as_raw_handle()),
                    0x0009_00a4,
                    Some(buffer.as_ptr().cast()),
                    buffer.len() as u32,
                    None,
                    0,
                    Some(&mut written),
                    None,
                )
            }
        }
        let root = unique_tmp("directory_write_pin");
        let target = root.join("target");
        let control = root.join("control");
        let guarded = root.join("guarded");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&control).unwrap();
        fs::create_dir(&guarded).unwrap();
        // First establish a real unprivileged in-place junction mutation fixture.
        make_junction(&control, &target).expect("control junction mutation");
        assert!(is_reparse_point(&control));
        let pins = create_dirs_pinned(&guarded).unwrap();
        let mutation = make_junction(&guarded, &target);
        assert!(
            mutation.is_err(),
            "a pinned directory must refuse reparse mutation: {mutation:?}"
        );
        assert!(!is_reparse_point(&guarded));
        fs::create_dir(guarded.join("child")).unwrap();
        fs::write(guarded.join("child/file"), b"child IO is allowed").unwrap();
        fs::rename(guarded.join("child/file"), guarded.join("child/moved")).unwrap();
        fs::remove_file(guarded.join("child/moved")).unwrap();
        fs::remove_dir(guarded.join("child")).unwrap();
        drop(pins);
        assert_eq!(
            fs::read_dir(&guarded).unwrap().count(),
            0,
            "guard must disappear on close"
        );
        fs::write(guarded.join("existing"), b"keep directory nonempty").unwrap();
        let pins = create_dirs_pinned(&guarded).unwrap();
        fs::remove_file(guarded.join("existing")).unwrap();
        assert!(make_junction(&guarded, &target).is_err());
        drop(pins);
        let copied = root.join("copied");
        copy_dir(&guarded, &copied).unwrap();
        assert_eq!(fs::read_dir(&guarded).unwrap().count(), 0);
        assert_eq!(
            fs::read_dir(&copied).unwrap().count(),
            0,
            "guard must never enter a backup"
        );
        fs::remove_dir(control).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn output_directory_pins_refuse_junction_ancestors_before_creating_children() {
        let root = unique_tmp("output_pins");
        let target = root.join("target");
        let link = root.join("link");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"unchanged").unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(create_dirs_pinned(&link.join("child")).is_err());
        assert!(remove_tree_no_reparse(&link.join("keep")).is_err());
        assert!(!crate::sysops::schedule_delete_on_reboot(
            link.join("keep").to_str().unwrap()
        ));
        assert!(!target.join("child").exists());
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"unchanged");
        // Unlink only the fixture junction before recursively removing its sandbox.
        fs::remove_dir(link).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    /// the restore write-back relies on `pin_dir_resolved` to expose
    /// junction redirects — regressions here would silently reintroduce the
    /// planted-junction write path. Junctions need no admin rights.
    #[cfg(windows)]
    #[test]
    fn pin_dir_resolved_resolves_junction_redirect() {
        let base = unique_tmp("pinresolved");
        let real = base.join("real");
        fs::create_dir_all(&real).unwrap();
        // CI's TEMP can contain RUNNER~1 even when our leaf is a long name.
        // Compare to the expanded request, not the environment's short spelling.
        let expected = long_path_form(&real).expect("expand real directory name");

        // A real directory resolves to itself.
        let pin = pin_dir_resolved(&real).expect("pin real dir");
        assert_eq!(
            pin.final_path().replace('/', "\\").to_lowercase(),
            expected.replace('/', "\\").to_lowercase()
        );
        drop(pin);

        let link = base.join("link");
        let st = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .status()
            .expect("mklink /J");
        assert!(st.success(), "mklink /J failed");

        // The junction path resolves to the TARGET, never to itself.
        let pin2 = pin_dir_resolved(&link).expect("pin junction");
        assert_eq!(
            pin2.final_path().to_lowercase(),
            expected.to_lowercase(),
            "final path must resolve through the junction"
        );
        assert_ne!(
            pin2.final_path().to_lowercase(),
            link.to_string_lossy().to_lowercase()
        );
        drop(pin2);
        fs::remove_dir_all(&base).unwrap();
    }
}
