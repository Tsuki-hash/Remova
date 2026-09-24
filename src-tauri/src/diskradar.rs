//! Disk usage radar — read-only top-level directory sizes for "what ate the drive".
//! Never deletes; UI only opens paths / links to uninstall.
//! Supports every local fixed drive; system drive keeps the well-known root view.

use crate::constants::DISK_RADAR_TOP_N;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirSizeRow {
    pub path: String,
    pub name: String,
    pub size_kb: i64,
    pub parent: String,
}

/// One local fixed drive available to the radar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveInfo {
    /// Uppercase letter without colon, e.g. `C`.
    pub letter: String,
    pub free_gb: f64,
    pub total_gb: f64,
    pub is_system: bool,
}

fn system_drive_letter() -> String {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let drive = drive.trim_end_matches('\\').to_uppercase();
    drive.chars().next().unwrap_or('C').to_string()
}

fn system_drive_roots() -> Vec<PathBuf> {
    let letter = system_drive_letter();
    let root = PathBuf::from(format!("{letter}:\\"));
    vec![
        root.join("Users"),
        root.join("Program Files"),
        root.join("Program Files (x86)"),
        root.join("ProgramData"),
        root.join("Windows"),
    ]
}

fn drive_root(letter: char) -> PathBuf {
    PathBuf::from(format!("{}:\\", letter.to_ascii_uppercase()))
}

fn disk_space_of(root: &str) -> Option<(f64, f64)> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let root_nul = format!("{root}\0");
        let wide: Vec<u16> = root_nul.encode_utf16().collect();
        let mut free = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(wide.as_ptr()),
                Some(&mut free),
                Some(&mut total),
                Some(&mut total_free),
            )
        };
        if ok.is_err() {
            return None;
        }
        Some((total as f64 / 1e9, free as f64 / 1e9))
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        None
    }
}

pub fn is_local_fixed_drive(letter: char) -> bool {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetDriveTypeW;
        // DRIVE_FIXED == 3 (winbase.h). windows-rs 0.58 path for the const varies.
        const DRIVE_FIXED: u32 = 3;
        let root = format!("{}:\\\0", letter.to_ascii_uppercase());
        let wide: Vec<u16> = root.encode_utf16().collect();
        let ty = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
        ty == DRIVE_FIXED
    }
    #[cfg(not(windows))]
    {
        let _ = letter;
        false
    }
}

/// Local fixed drives (skip CD / network / removable shapes via GetDriveType).
pub fn list_local_drives() -> Vec<DriveInfo> {
    let sys = system_drive_letter();
    let mut out = Vec::new();
    for c in b'A'..=b'Z' {
        let letter = c as char;
        if !is_local_fixed_drive(letter) {
            continue;
        }
        let root = format!("{letter}:\\");
        if !PathBuf::from(&root).is_dir() {
            continue;
        }
        let (total_gb, free_gb) = disk_space_of(&root).unwrap_or((0.0, 0.0));
        out.push(DriveInfo {
            letter: letter.to_string(),
            free_gb,
            total_gb,
            is_system: letter.to_ascii_uppercase().to_string() == sys,
        });
    }
    out
}

/// Bounded size walk for radar rows — deep trees (Users/Windows) must not stall the UI.
fn shallow_size_kb(root: &std::path::Path, max_depth: u32, budget: &mut u64) -> i64 {
    if *budget == 0 || max_depth == 0 {
        return 0;
    }
    let mut bytes = 0i64;
    let Ok(rd) = std::fs::read_dir(root) else {
        return 0;
    };
    for ent in rd.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let p = ent.path();
        if p.is_dir() {
            bytes += shallow_size_kb(&p, max_depth - 1, budget) * 1024;
        } else if let Ok(meta) = ent.metadata() {
            bytes += meta.len() as i64;
        }
    }
    if bytes <= 0 {
        0
    } else {
        (bytes + 1023) / 1024
    }
}

fn child_rows(parent: &PathBuf) -> Vec<DirSizeRow> {
    let mut rows = Vec::new();
    let Ok(rd) = std::fs::read_dir(parent) else {
        return rows;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if !p.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        let mut budget = 2_500u64;
        let size_kb = shallow_size_kb(&p, 3, &mut budget);
        if size_kb <= 0 {
            continue;
        }
        rows.push(DirSizeRow {
            path: p.to_string_lossy().to_string(),
            name,
            size_kb,
            parent: parent.to_string_lossy().to_string(),
        });
    }
    rows.sort_by_key(|a| std::cmp::Reverse(a.size_kb));
    rows.truncate(DISK_RADAR_TOP_N);
    rows
}

/// Top directories: system drive well-known roots, or one local drive's `X:\` children.
pub fn top_dir_sizes_for_drive(letter: Option<char>) -> Vec<DirSizeRow> {
    let sys = system_drive_letter()
        .chars()
        .next()
        .unwrap_or('C')
        .to_ascii_uppercase();
    let letter = letter
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or(sys);
    let mut out = Vec::new();
    if letter == sys {
        for root in system_drive_roots() {
            if root.exists() {
                let mut rows = child_rows(&root);
                out.append(&mut rows);
            }
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.size_kb));
        out.truncate(DISK_RADAR_TOP_N * 2);
    } else {
        let root = drive_root(letter);
        if root.is_dir() && is_local_fixed_drive(letter) {
            out = child_rows(&root);
        }
    }
    out
}

/// Back-compat default (system drive).
pub fn top_dir_sizes() -> Vec<DirSizeRow> {
    top_dir_sizes_for_drive(None)
}

fn is_drive_root_shape(norm: &str) -> bool {
    // `C:` or `C:\`
    let t = norm.trim_end_matches('\\');
    let b = t.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// True when `path` is browsable by the radar for its drive (read-only).
fn under_radar_roots(path: &str) -> bool {
    let raw = path.replace('/', "\\");
    // Traversal segments must never pass a prefix check (even if the path is missing
    // and canonicalize cannot resolve it).
    if raw.split('\\').any(|seg| seg == ".." || seg == ".") {
        return false;
    }
    // UNC / device / non-drive shapes are out of scope.
    if raw.starts_with("\\\\") || raw.contains("\\\\?\\") || raw.contains("\\\\.\\") {
        return false;
    }
    // Canonicalize first so verbatim / 8.3 / substituted shapes resolve to the real root.
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
    // Non-UTF-8 after canonicalize → refuse (no lossy prefix match).
    let Some(canon_s) = crate::fsutil::path_utf8(&canon) else {
        return false;
    };
    let mut norm = canon_s.trim_end_matches('\\').to_uppercase();
    // Strip Windows verbatim prefix (`\\?\`).
    if let Some(rest) = norm.strip_prefix(r"\\?\") {
        norm = rest.to_string();
    }
    // Must be a drive-rooted path: `X:\...`
    let b = norm.as_bytes();
    if !(b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':') {
        return false;
    }
    let letter = b[0] as char;
    let letter_up = letter.to_ascii_uppercase().to_string();
    let sys = system_drive_letter();
    if is_drive_root_shape(&norm) {
        // Drive root: system drive always; others only when local fixed.
        return letter_up == sys || is_local_fixed_drive(letter);
    }
    if letter_up == sys {
        for leaf in [
            "USERS",
            "PROGRAM FILES",
            "PROGRAM FILES (X86)",
            "PROGRAMDATA",
            "WINDOWS",
        ] {
            let root = format!("{sys}:\\{leaf}");
            if norm == root || norm.starts_with(&format!("{root}\\")) {
                return true;
            }
        }
        return false;
    }
    // Non-system local fixed drive: any folder under `X:\` (still read-only).
    is_local_fixed_drive(letter) && norm.starts_with(&format!("{letter_up}:\\"))
}

/// Immediate children of `path` ranked by size (for drill-down).
pub fn list_dir_children(path: &str) -> Result<Vec<DirSizeRow>, String> {
    let p = PathBuf::from(path.trim());
    if path.trim().is_empty() || !p.is_dir() {
        return Err("not a directory".into());
    }
    // Only allow drill-down under radar roots (read-only safety).
    if !under_radar_roots(&p.to_string_lossy()) {
        return Err("path outside radar roots".into());
    }
    Ok(child_rows(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_dir_sizes_runs() {
        let rows = top_dir_sizes();
        // On any machine this must return without panic.
        assert!(rows.len() <= DISK_RADAR_TOP_N * 2);
        for r in rows.iter().take(5) {
            assert!(r.size_kb >= 0);
            assert!(!r.path.is_empty());
        }
    }

    #[test]
    fn list_dir_children_rejects_blank() {
        assert!(list_dir_children("").is_err());
        assert!(list_dir_children("   ").is_err());
    }

    #[test]
    fn under_radar_roots_whitelist() {
        assert!(super::under_radar_roots(r"C:\Users\a\Documents"));
        assert!(super::under_radar_roots(r"C:\Program Files\App"));
        assert!(super::under_radar_roots(r"C:\Windows\Temp"));
        // System-drive non-root trees stay restricted.
        assert!(!super::under_radar_roots(r"C:\Games\Steam"));
        // UNC / device prefixes never allowed.
        assert!(!super::under_radar_roots(r"\\server\share\file"));
        assert!(!super::under_radar_roots(r"\\?\C:\x"));
        // Non-system local fixed drives are browsable (letter-dependent).
        let d_ok = super::is_local_fixed_drive('D');
        assert_eq!(super::under_radar_roots(r"D:\Games\Steam"), d_ok);
    }

    /// P1.8: `..` must never pass the radar roots check (string prefix or canonicalize).
    #[test]
    fn drilldown_rejects_traversal_outside_roots() {
        // Traversal segments are refused outright — before any prefix match.
        assert!(!super::under_radar_roots(r"C:\Users\a\Documents\..\..\..\Windows\System32"));
        assert!(!super::under_radar_roots(r"C:\Users\a\Documents\..\..\Temp\evil"));
        assert!(!super::under_radar_roots(r"C:\Windows\Temp\..\..\Games\Steam"));
        assert!(!super::under_radar_roots(r"C:\Users\a\Documents\..\secret"));
        // Clean paths under roots still pass.
        assert!(super::under_radar_roots(r"C:\Users\a\Documents\keep"));
        assert!(super::under_radar_roots(r"C:\Windows\Temp\ok"));
        // Drill-down refuses traversal shapes.
        assert!(list_dir_children(r"C:\Users\a\Documents\..\..\..\Windows\System32").is_err());
    }

    #[test]
    fn list_local_drives_runs() {
        let drives = super::list_local_drives();
        assert!(!drives.is_empty());
        assert!(drives.iter().any(|d| d.is_system));
        for d in &drives {
            assert_eq!(d.letter.len(), 1);
            assert!(d.total_gb >= 0.0);
        }
    }

    #[test]
    fn top_for_unknown_drive_is_empty() {
        // Phantom drive letter should not panic and should return no rows.
        let rows = super::top_dir_sizes_for_drive(Some('Q'));
        assert!(rows.is_empty() || rows.iter().all(|r| r.path.starts_with("Q:")));
    }
}
