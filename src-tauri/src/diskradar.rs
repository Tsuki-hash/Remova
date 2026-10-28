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
    /// Entry/depth budget ran out during the walk — size is a floor, not a total.
    #[serde(default)]
    pub capped: bool,
    /// Entries actually inspected for this row; with `capped`, the UI shows
    /// the floor was taken after this many entries.
    #[serde(default)]
    pub scanned: u64,
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
/// Returns (size_kb, capped) — `capped` marks walks that hit the entry
/// budget or depth cap, so callers can present a floor instead of a total
/// (the same honesty the other size walkers report).
fn shallow_size_bytes(root: &std::path::Path, max_depth: u32, budget: &mut u64) -> (i64, bool) {
    if crate::scan_task::cancelled() {
        return (0, true);
    }
    if crate::fsutil::is_reparse_point(root) {
        return (0, false);
    }
    if *budget == 0 || max_depth == 0 {
        return (0, true);
    }
    let mut bytes = 0i64;
    let mut capped = false;
    let Ok(rd) = std::fs::read_dir(root) else {
        return (0, true);
    };
    for ent in rd {
        if crate::scan_task::cancelled() {
            return (bytes, true);
        }
        let Ok(ent) = ent else {
            capped = true;
            continue;
        };
        if *budget == 0 {
            capped = true;
            break;
        }
        *budget -= 1;
        let p = ent.path();
        let Ok(meta) = ent.metadata() else {
            capped = true;
            continue;
        };
        // One metadata read answers both "is this a reparse point?" and the
        // file size; a separate symlink_metadata call doubled every stat.
        if crate::fsutil::is_reparse_metadata(&meta) {
            continue;
        }
        if meta.is_dir() {
            let (sub, sub_capped) = shallow_size_bytes(&p, max_depth - 1, budget);
            bytes += sub;
            capped |= sub_capped;
        } else if meta.is_file() {
            bytes += meta.len() as i64;
        }
    }
    (bytes, capped)
}

/// Total entries one radar call may inspect across every root. Measured on a
/// real machine: one user profile alone holds ~320k files, and a file-heavy
/// tree burns a small budget within a few hundred steps — the 60k budget this
/// constant once had left every user directory as a near-useless floor
/// (`C:\Users\Aaron ≥ 1.78 GB` against a real 44 GB). One million entries
/// covers typical trees at the walk depth and costs seconds, not minutes.
const RADAR_CALL_BUDGET: u64 = 1_000_000;

/// Directory levels walked below each listed row (`Users\Aaron` descends four
/// levels, reaching `AppData\Local\Google\Chrome`). Depth beyond the budget
/// still ends the walk honestly via `capped`.
const RADAR_CHILD_DEPTH: u32 = 4;

/// One bounded walk over `root` that yields an aggregate row for the root
/// itself plus rows for its direct child directories. One traversal serves
/// both: every byte found below a child is attributed to that child's row and
/// summed into the root aggregate; files directly under the root only count
/// toward the aggregate. Every number stays a floor when the entry budget or
/// the depth cap ends the walk (`capped`).
fn root_walk(
    root: &std::path::Path,
    max_depth: u32,
    budget: &mut u64,
) -> (DirSizeRow, Vec<DirSizeRow>) {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().trim_end_matches('\\').to_string());
    let mut aggregate = DirSizeRow {
        path: root.to_string_lossy().into_owned(),
        name,
        size_kb: 0,
        parent: root
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        capped: false,
        scanned: 0,
    };
    if !crate::scan_task::checkpoint("sizes", 0) || crate::fsutil::is_reparse_point(root) {
        return (aggregate, Vec::new());
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        // Unreadable root: report the row as an explicit floor, not silence.
        aggregate.capped = true;
        return (aggregate, Vec::new());
    };
    // A cheap first pass classifies the level directly under the root: files
    // sum into the aggregate, child directories are queued for the walk.
    let mut child_dirs: Vec<PathBuf> = Vec::new();
    let mut agg_bytes: i64 = 0;
    for ent in rd.flatten() {
        if crate::scan_task::cancelled() || *budget == 0 {
            aggregate.capped = true;
            break;
        }
        *budget -= 1;
        aggregate.scanned += 1;
        let Ok(meta) = ent.metadata() else {
            continue;
        };
        if crate::fsutil::is_reparse_metadata(&meta) {
            continue;
        }
        if meta.is_dir() {
            child_dirs.push(ent.path());
        } else if meta.is_file() {
            agg_bytes += meta.len() as i64;
        }
    }
    // Dynamic slices: a child may use up to an even share of what is left, so
    // an early, huge subtree cannot starve its siblings — unused share flows
    // back to the pool for the remaining children.
    let mut children = Vec::new();
    let mut remaining = child_dirs.len();
    for child in child_dirs {
        if crate::scan_task::cancelled() {
            aggregate.capped = true;
            break;
        }
        if *budget == 0 {
            aggregate.capped = true;
            break;
        }
        remaining = remaining.saturating_sub(1);
        let mut slice = (*budget / (remaining as u64 + 1)).max(1).min(*budget);
        let before = slice;
        let (bytes, capped) = shallow_size_bytes(&child, max_depth, &mut slice);
        let used = before - slice;
        *budget -= used;
        aggregate.scanned += used;
        agg_bytes += bytes;
        aggregate.capped |= capped;
        let row = DirSizeRow {
            path: child.to_string_lossy().into_owned(),
            name: child
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size_kb: if bytes <= 0 { 0 } else { (bytes + 1023) / 1024 },
            parent: root.to_string_lossy().into_owned(),
            capped,
            scanned: used,
        };
        if row.size_kb > 0 {
            children.push(row);
        }
    }
    aggregate.size_kb = if agg_bytes <= 0 {
        0
    } else {
        (agg_bytes + 1023) / 1024
    };
    (aggregate, children)
}

/// Top directories: system drive well-known roots, or one local drive's `X:\` children.
/// Each well-known root contributes its own aggregate row ("where did the
/// drive go at this root") plus its largest children, ranked together.
pub fn top_dir_sizes_for_drive(letter: Option<char>) -> Vec<DirSizeRow> {
    if crate::scan_task::cancelled() {
        return Vec::new();
    }
    let sys = system_drive_letter()
        .chars()
        .next()
        .unwrap_or('C')
        .to_ascii_uppercase();
    let letter = letter.map(|c| c.to_ascii_uppercase()).unwrap_or(sys);
    let mut out: Vec<DirSizeRow> = Vec::new();
    // One budget for the whole call, shared by every root.
    let mut budget = RADAR_CALL_BUDGET;
    if letter == sys {
        let roots: Vec<PathBuf> = system_drive_roots()
            .into_iter()
            .filter(|root| root.exists())
            .collect();
        let mut remaining = roots.len();
        for root in roots {
            if crate::scan_task::cancelled() {
                return out;
            }
            // Give every remaining root a fair share; an unused share returns
            // to the pool instead of letting the first root consume it all.
            let share = budget / (remaining.max(1) as u64);
            remaining = remaining.saturating_sub(1);
            let mut root_budget = share;
            let (aggregate, rows) = root_walk(&root, RADAR_CHILD_DEPTH, &mut root_budget);
            budget = budget.saturating_sub(share.saturating_sub(root_budget));
            if aggregate.size_kb > 0 {
                out.push(aggregate);
            }
            out.extend(rows);
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.size_kb));
        out.truncate(DISK_RADAR_TOP_N * 2);
    } else {
        let root = drive_root(letter);
        if root.is_dir() && is_local_fixed_drive(letter) {
            let (aggregate, rows) = root_walk(&root, RADAR_CHILD_DEPTH, &mut budget);
            if aggregate.size_kb > 0 {
                out.push(aggregate);
            }
            out.extend(rows);
            out.sort_by_key(|a| std::cmp::Reverse(a.size_kb));
            out.truncate(DISK_RADAR_TOP_N);
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
    // Refuse remote/reparse shapes before canonicalize can follow them.
    if !crate::fsutil::is_local_drive_path(path) {
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

/// Immediate children of `path` ranked by size (for drill-down). The same
/// dynamic-slice walk as the top view: a huge first sibling cannot starve the
/// rest of the listing.
pub fn list_dir_children(path: &str) -> Result<Vec<DirSizeRow>, String> {
    // Validate the path before any metadata probe: `is_dir()` on a UNC/device
    // path would contact the remote side before the allow-list rejects it.
    if path.trim().is_empty() {
        return Err("not a directory".into());
    }
    if !under_radar_roots(path.trim()) {
        return Err("path outside radar roots".into());
    }
    let p = PathBuf::from(path.trim());
    if !p.is_dir() {
        return Err("not a directory".into());
    }
    let mut budget = RADAR_CALL_BUDGET;
    let (_, rows) = root_walk(&p, RADAR_CHILD_DEPTH, &mut budget);
    Ok(rows)
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancelled_radar_walk_keeps_budget_and_rejects_partial_results() {
        let id = crate::scan_task::begin().unwrap();
        let mut budget = 20;
        let result = crate::scan_task::run(id, || {
            crate::scan_task::cancel(id).unwrap();
            super::shallow_size_bytes(
                std::path::Path::new("cancelled-radar-unread"),
                3,
                &mut budget,
            )
        });
        assert_eq!(budget, 20);
        assert_eq!(result, Err("scan:cancelled".into()));
    }
    #[test]
    fn failed_directory_read_marks_radar_size_as_a_lower_bound() {
        let root = std::env::temp_dir().join(format!("remova-radar-io-{}", std::process::id()));
        std::fs::write(&root, b"not a directory").unwrap();
        assert_eq!(super::shallow_size_bytes(&root, 3, &mut 20), (0, true));
        std::fs::remove_file(root).unwrap();
    }
    #[test]
    #[cfg(windows)]
    fn radar_skips_root_junctions_and_child_junction_rows() {
        let root = std::env::temp_dir().join(format!(
            "remova-radar-link-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let target = root.join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("data"), b"kept").unwrap();
        let link = root.join("link");
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(super::shallow_size_bytes(&link, 3, &mut 100), (0, false));
        // A junction root yields nothing; a real root lists the real child but
        // never the junction.
        assert!(super::root_walk(&link, 4, &mut 1_000).1.is_empty());
        assert_eq!(super::root_walk(&root, 4, &mut 1_000).1.len(), 1);
        assert_eq!(super::shallow_size_bytes(&target, 3, &mut 100), (4, false));
        std::fs::remove_dir(&link).unwrap();
        crate::fsutil::remove_tree_no_reparse(&root).unwrap();
    }
    #[test]
    fn nested_small_files_are_rounded_only_once_and_caps_are_reported() {
        let root = std::env::temp_dir().join(format!(
            "remova_radar_bytes_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for name in ["a", "b"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
            std::fs::write(root.join(name).join("file"), b"x").unwrap();
        }
        assert_eq!(super::shallow_size_bytes(&root, 3, &mut 100), (2, false));
        assert!(super::shallow_size_bytes(&root, 3, &mut 1).1);
        std::fs::remove_dir_all(root).unwrap();
    }
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

    /// .8: `..` must never pass the radar roots check (string prefix or canonicalize).
    #[test]
    fn drilldown_rejects_traversal_outside_roots() {
        // Traversal segments are refused outright — before any prefix match.
        assert!(!super::under_radar_roots(
            r"C:\Users\a\Documents\..\..\..\Windows\System32"
        ));
        assert!(!super::under_radar_roots(
            r"C:\Users\a\Documents\..\..\Temp\evil"
        ));
        assert!(!super::under_radar_roots(
            r"C:\Windows\Temp\..\..\Games\Steam"
        ));
        assert!(!super::under_radar_roots(r"C:\Users\a\Documents\..\secret"));
        // Clean paths under roots still pass.
        assert!(super::under_radar_roots(r"C:\Users\a\Documents\keep"));
        assert!(super::under_radar_roots(r"C:\Windows\Temp\ok"));
        // Drill-down refuses traversal shapes.
        assert!(list_dir_children(r"C:\Users\a\Documents\..\..\..\Windows\System32").is_err());
    }

    #[test]
    fn radar_budget_is_shared_across_roots() {
        let tmp = crate::test_support::TempDir::new("radar-shared-budget");
        for root_name in ["root-a", "root-b"] {
            let root = tmp.path().join(root_name);
            let child = root.join("child");
            std::fs::create_dir_all(&child).unwrap();
            std::fs::write(child.join("payload.bin"), vec![0u8; 4096]).unwrap();
        }
        let mut budget = 3u64;
        let (aggregate, first) = super::root_walk(&tmp.path().join("root-a"), 4, &mut budget);
        assert_eq!(first.len(), 1);
        assert!(aggregate.size_kb > 0);
        let (second_agg, second) = super::root_walk(&tmp.path().join("root-b"), 4, &mut budget);
        assert!(
            second.is_empty(),
            "a second root must not reset the call-wide budget"
        );
        assert!(second_agg.capped, "a starved root reports a floor");
    }

    #[test]
    fn root_walk_reports_the_aggregate_and_children_together() {
        let tmp = crate::test_support::TempDir::new("radar-aggregate");
        let root = tmp.path().join("Users");
        for name in ["alice", "bob"] {
            let child = root.join(name);
            std::fs::create_dir_all(&child).unwrap();
            std::fs::write(child.join("payload.bin"), vec![0u8; 4096]).unwrap();
        }
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::write(root.join("root-file.cfg"), vec![0u8; 100]).unwrap();
        let mut budget = 1_000u64;
        let (aggregate, rows) = super::root_walk(&root, 4, &mut budget);
        // 4096 + 4096 + 100 bytes = 8292 -> 9 KB, rounded once for the whole
        // aggregate; the empty child is not listed, files at the root level
        // count toward the aggregate only.
        assert_eq!(aggregate.size_kb, 9);
        assert_eq!(rows.len(), 2, "zero-size children are not listed");
        assert_eq!(
            aggregate.scanned, 6,
            "3 child dirs + 1 root file classified, 2 child walks"
        );
        for row in &rows {
            assert_eq!(row.size_kb, 4);
            assert!(!row.capped);
            assert!(row.scanned > 0);
            assert_eq!(row.parent, root.to_string_lossy());
        }
        assert!(budget > 0, "unused slice budget returns to the pool");
    }

    #[test]
    fn root_walk_dynamic_slices_keep_late_siblings_visible() {
        let tmp = crate::test_support::TempDir::new("radar-fair-slices");
        let root = tmp.path().join("Users");
        // A huge first sibling (alphabetically first) and a tiny late one, on
        // a budget that cannot cover both: the late sibling must still get a
        // slice of its own instead of being starved to zero.
        let big = root.join("aaa-big");
        std::fs::create_dir_all(&big).unwrap();
        for i in 0..30 {
            std::fs::write(big.join(format!("f{i:02}.bin")), vec![0u8; 4096]).unwrap();
        }
        let small = root.join("zzz-small");
        std::fs::create_dir_all(&small).unwrap();
        std::fs::write(small.join("payload.bin"), vec![0u8; 4096]).unwrap();
        let mut budget = 12u64;
        let (aggregate, rows) = super::root_walk(&root, 4, &mut budget);
        assert_eq!(rows.len(), 2, "the late sibling must not be starved away");
        let big_row = rows.iter().find(|r| r.name == "aaa-big").unwrap();
        let small_row = rows.iter().find(|r| r.name == "zzz-small").unwrap();
        assert!(big_row.capped, "the big sibling's slice ends its walk");
        assert!(!small_row.capped, "the late sibling completes its own walk");
        assert!(aggregate.capped, "any capped child caps the aggregate");
        // The big child scanned exactly its slice (5 entries: 5 of its 30
        // files): the floor is honest about how far it got. The small child
        // completed its single entry.
        assert_eq!(big_row.scanned, 5);
        assert_eq!(small_row.scanned, 1);
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
