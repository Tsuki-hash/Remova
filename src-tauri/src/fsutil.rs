//! Shared filesystem helpers (dedupe backup/restore copy).

use std::path::Path;

/// Windows reparse point (junction / mount / symlink). Never follow when copying (REV-SEC-03).
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

/// Recursively copy a directory tree (files + dirs). Symlinks and other reparse points are skipped.
pub fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    // Per-level pin (same as the delete path): a child swapped for a junction
    // between the reparse check and this open is refused, not followed.
    let _pin = pin_dir_no_reparse(src)?;
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        // junction/mount reparse: `file_type().is_symlink()` is false on Windows — must use attributes.
        if is_reparse_point(&path) {
            continue;
        }
        let ty = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&path, &target)?;
        } else if ty.is_file() {
            // R23-SEC-01: the reparse check above is a path stat — a child
            // swapped for a link before `fs::copy` re-opens the path would be
            // followed. The dedicated primitive opens by non-following handle.
            copy_file_no_reparse(&path, &target)?;
        }
    }
    Ok(())
}

/// Copy a single file; refuse reparse sources so `fs::copy` cannot follow a swapped junction.
pub fn copy_file_no_reparse(src: &Path, dest: &Path) -> std::io::Result<u64> {
    if is_reparse_point(src) {
        return Err(std::io::Error::other("refusing to copy reparse point"));
    }
    // Read through a handle that never follows a link swapped in after the
    // check (streamed — no whole-file buffering).
    #[cfg(windows)]
    {
        use std::io::Write as _;
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(src)?;
        let mut out = std::fs::File::create(dest)?;
        let n = std::io::copy(&mut f, &mut out)?;
        out.flush()?;
        Ok(n)
    }
    #[cfg(not(windows))]
    {
        std::fs::copy(src, dest)
    }
}

/// REV-SEC-06: a handle that pins a directory (or file) while it is being
/// cleared. Opened with FILE_FLAG_OPEN_REPARSE_POINT — a swapped-in junction
/// is opened as the link itself, never its target — and a share mode WITHOUT
/// FILE_SHARE_DELETE, so the path cannot be renamed away while pinned: the
/// object verified below is exactly the object that will be removed.
pub struct DirPin {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

impl Drop for DirPin {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// Open `p` and verify it is not a reparse point — by handle, not by path.
pub fn pin_dir_no_reparse(p: &Path) -> std::io::Result<DirPin> {
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
        const DELETE_RIGHT: u32 = 0x0001_0000;
        const FILE_READ_ATTR: u32 = 0x0000_0080;
        // R21-SEC-09: lossy conversion would pin a U+FFFD-replaced path — fail closed.
        let Some(path_str) = p.to_str() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains unpaired surrogates",
            ));
        };
        unsafe {
            let wide = to_wide(path_str);
            // Holding DELETE ourselves while not sharing it is what blocks a
            // concurrent rename — a 0-access handle does not.
            let handle = CreateFileW(
                PCWSTR(wide.as_ptr()),
                DELETE_RIGHT | FILE_READ_ATTR,
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
            Ok(DirPin { handle })
        }
    }
    #[cfg(not(windows))]
    {
        if is_reparse_point(p) {
            return Err(std::io::Error::other(
                "refusing to delete through reparse point",
            ));
        }
        Ok(DirPin {})
    }
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
        const DELETE_RIGHT: u32 = 0x0001_0000;
        const FILE_READ_ATTR: u32 = 0x0000_0080;
        let Some(path_str) = p.to_str() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains unpaired surrogates",
            ));
        };
        unsafe {
            let wide = to_wide(path_str);
            // No FILE_FLAG_OPEN_REPARSE_POINT: the handle must land on the
            // chain target so the final path exposes any planted junction.
            let handle = CreateFileW(
                PCWSTR(wide.as_ptr()),
                DELETE_RIGHT | FILE_READ_ATTR,
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

/// R23-SEC-04: long (expanded) form of an existing path — `GetLongPathNameW`
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

/// R23-QA companion: short (8.3) form of an existing path, for tests that
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

/// Delete a tree without following reparse points (REV-BE-05 / REV-SEC-06).
/// Every level is pinned (no-DELETE-share handle + by-handle reparse check)
/// before its children are cleared, so the path cannot be swapped for a
/// junction mid-recursion; child junctions are unlinked as links only.
pub fn remove_tree_no_reparse(p: &Path) -> std::io::Result<()> {
    let is_dir = {
        let _pin = pin_dir_no_reparse(p)?;
        let meta = std::fs::symlink_metadata(p)?;
        if meta.is_dir() {
            for entry in std::fs::read_dir(p)? {
                let entry = entry?;
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
                    remove_tree_no_reparse(&child)?;
                } else {
                    std::fs::remove_file(&child)?;
                }
            }
            true
        } else {
            false
        }
    };
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
    let safe = if matches!(s.chars().next(), Some('=' | '+' | '-' | '@' | '\t')) {
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

/// Shared `to_wide` for Windows APIs.
#[cfg(windows)]
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_tmp(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "remova_fsutil_{tag}_{}_{}",
            std::process::id(),
            nanos
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn copy_file_no_reparse_refuses_missing_is_ok_error() {
        let tmp = unique_tmp("nofile");
        let missing = tmp.join("nope.bin");
        assert!(copy_file_no_reparse(&missing, &tmp.join("out.bin")).is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remove_tree_no_reparse_refuses_root_reparse() {
        let tmp = unique_tmp("rm_reparse");
        let link = tmp.join("link");
        let target = tmp.join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("f.txt"), b"x").unwrap();
        let _ = std::os::windows::fs::symlink_dir(&target, &link);
        if is_reparse_point(&link) {
            assert!(remove_tree_no_reparse(&link).is_err());
            assert!(
                target.join("f.txt").exists(),
                "must not delete through link"
            );
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    /// REV-SEC-06: the pin itself must refuse reparse points and actually pin
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
        let _ = std::os::windows::fs::symlink_dir(&target, &junc);
        if is_reparse_point(&junc) {
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
        let _ = std::os::windows::fs::symlink_dir(&outside, &junc);
        remove_tree_no_reparse(&tree).unwrap();
        assert!(!tree.exists());
        assert!(
            outside.join("keep.txt").exists(),
            "junction target must survive"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn copy_dir_skips_symlink_entries() {
        let tmp = unique_tmp("link");
        let src = tmp.join("src");
        let dest = tmp.join("dest");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("ok.txt"), b"x").unwrap();
        // Best-effort symlink: skip assertion on platforms that cannot create one.
        let _ = std::os::windows::fs::symlink_file(src.join("ok.txt"), src.join("link.txt"));
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
        // OWASP CSV injection: formula prefixes are neutralized (R22-SUP-06).
        assert_eq!(csv_escape("=cmd|' /C calc'!A0"), "'=cmd|' /C calc'!A0");
        assert_eq!(csv_escape("+sum"), "'+sum");
        assert_eq!(csv_escape("-flag"), "'-flag");
        assert_eq!(csv_escape("@import"), "'@import");
        assert_eq!(csv_escape("\t=1"), "'\t=1");
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
    /// R23-QA-02: the restore write-back relies on `pin_dir_resolved` to expose
    /// junction redirects — regressions here would silently reintroduce the
    /// planted-junction write path. Junctions need no admin rights.
    #[cfg(windows)]
    #[test]
    fn pin_dir_resolved_resolves_junction_redirect() {
        let base = unique_tmp("pinresolved");
        let real = base.join("real");
        fs::create_dir_all(&real).unwrap();

        // A real directory resolves to itself.
        let pin = pin_dir_resolved(&real).expect("pin real dir");
        assert_eq!(
            pin.final_path().replace('/', "\\").to_lowercase(),
            real.to_string_lossy().replace('/', "\\").to_lowercase()
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
            real.to_string_lossy().to_lowercase(),
            "final path must resolve through the junction"
        );
        assert_ne!(
            pin2.final_path().to_lowercase(),
            link.to_string_lossy().to_lowercase()
        );
        let _ = fs::remove_dir_all(&base);
    }
}
