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
    /// Entries actually inspected for this row; with `capped`, the floor was
    /// taken after this many entries.
    #[serde(default)]
    pub scanned: u64,
    /// True for a listed root's own total ("where did the drive go at this
    /// root"), false for a single child directory. Same shape, different scope:
    /// the UI labels the difference.
    #[serde(default)]
    pub aggregate: bool,
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
/// Returns `(bytes, capped)`: `capped` marks walks that hit the entry budget,
/// a depth cap, an unreadable level or a cancelled scan, so callers present a
/// floor instead of a total. A reparse child is a link, not storage — its bytes
/// are counted where they physically live — so skipping it does not make the
/// walk incomplete.
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
            let (bytes_below, below_capped) = shallow_size_bytes(&p, max_depth - 1, budget);
            bytes += bytes_below;
            capped |= below_capped;
        } else if meta.is_file() {
            bytes += meta.len() as i64;
        }
    }
    (bytes, capped)
}

/// Total entries one radar call may inspect across every root. Measured on a
/// real machine: one user profile holds ~320k files and a small budget is spent
/// within a few hundred steps inside a single cache tree. The 60k budget this
/// constant once had left every user directory as a near-useless floor
/// (`C:\Users\Aaron >= 1.78 GB` against a real 44 GB); two million covers a
/// file-heavy profile plus every system root at the depth below.
const RADAR_CALL_BUDGET: u64 = 2_000_000;

/// Limits that keep one radar call predictable.
struct RadarLimits {
    /// Directory levels below a listed row whose contents are read. Eight
    /// levels reach the files inside
    /// `Users\<user>\AppData\Local\Google\Chrome\User Data\Default\Cache`,
    /// which is where a browser keeps most of its bytes; a deeper tree ends as
    /// a marked floor. The slice budget, not this cap, bounds the work.
    max_depth: u32,
    /// Rows retained per listed root; the final list is ranked and truncated
    /// again, so retention only has to be wide enough for the real cutoff.
    row_cap: usize,
    /// Direct child directories queued for one root. A wider level is cut and
    /// the aggregate becomes a floor, so a pathological directory cannot grow
    /// the queue without bound.
    queue_cap: usize,
}

/// Wide levels are cut instead of queued without limit.
const RADAR_QUEUE_CAP: usize = 4096;

/// Top view: each system root keeps twice the final cutoff so the combined
/// ranking still sees every candidate.
const RADAR_VISIT_LIMITS: RadarLimits = RadarLimits {
    max_depth: 8,
    row_cap: DISK_RADAR_TOP_N * 2,
    queue_cap: RADAR_QUEUE_CAP,
};

/// Drill-down: one parent lists at most the final cutoff, like the top view.
const RADAR_DRILL_LIMITS: RadarLimits = RadarLimits {
    max_depth: 8,
    row_cap: DISK_RADAR_TOP_N,
    queue_cap: RADAR_QUEUE_CAP,
};

fn kb(bytes: i64) -> i64 {
    if bytes <= 0 {
        0
    } else {
        (bytes + 1023) / 1024
    }
}

/// Rank by size and keep at most `cap` rows. The path tie-break makes the order
/// deterministic; a capped row ranks by its floor, and its marker says the real
/// size may be larger.
fn rank_rows(rows: &mut Vec<DirSizeRow>, cap: usize) {
    rows.sort_by(|a, b| b.size_kb.cmp(&a.size_kb).then_with(|| a.path.cmp(&b.path)));
    rows.truncate(cap);
}

/// One bounded walk over `root` that yields an aggregate row for the root
/// itself plus rows for its direct child directories. One traversal serves
/// both: every byte found below a child is attributed to that child's row and
/// summed into the root aggregate; files directly under the root only count
/// toward the aggregate. Every number is a floor (`capped`) when the entry
/// budget, the depth cap, the queue cap or an unreadable entry ends the walk.
fn root_walk(
    root: &std::path::Path,
    limits: &RadarLimits,
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
        aggregate: true,
    };
    if !crate::scan_task::checkpoint("sizes", 0) {
        aggregate.capped = true;
        return (aggregate, Vec::new());
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        // Unreadable root: report the row as an explicit floor, not silence.
        aggregate.capped = true;
        return (aggregate, Vec::new());
    };
    // First pass classifies the level directly under the root: files sum into
    // the aggregate, child directories are queued for the walk below.
    let mut child_dirs: Vec<PathBuf> = Vec::new();
    let mut root_bytes: i64 = 0;
    let mut child_bytes_total: i64 = 0;
    for ent in rd {
        if crate::scan_task::cancelled() || *budget == 0 {
            aggregate.capped = true;
            break;
        }
        let Ok(ent) = ent else {
            // A level we could not enumerate is a floor, not a total.
            aggregate.capped = true;
            continue;
        };
        *budget -= 1;
        aggregate.scanned += 1;
        let Ok(meta) = ent.metadata() else {
            aggregate.capped = true;
            continue;
        };
        // A reparse child is a pointer to storage elsewhere (often another
        // drive); counting it here would double-count. Explorer's own size
        // column skips them too, so a skipped junction is the correct total.
        if crate::fsutil::is_reparse_metadata(&meta) {
            continue;
        }
        if meta.is_dir() {
            if child_dirs.len() >= limits.queue_cap {
                aggregate.capped = true;
                break;
            }
            child_dirs.push(ent.path());
        } else if meta.is_file() {
            root_bytes += meta.len() as i64;
        }
    }
    // Deterministic order: budget shares must not depend on read_dir order.
    child_dirs.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    // Dynamic slices: a child may use up to an even share of what is left, so
    // an early huge subtree cannot starve its siblings — an unused share flows
    // back to the pool for the remaining children.
    let mut children: Vec<(DirSizeRow, i64)> = Vec::new();
    let mut remaining = child_dirs.len();
    for child in child_dirs {
        if crate::scan_task::cancelled() || *budget == 0 {
            aggregate.capped = true;
            break;
        }
        remaining = remaining.saturating_sub(1);
        let mut slice = (*budget / (remaining as u64 + 1)).max(1).min(*budget);
        let before = slice;
        let (bytes, capped) = shallow_size_bytes(&child, limits.max_depth, &mut slice);
        let used = before - slice;
        *budget -= used;
        aggregate.scanned += used;
        child_bytes_total += bytes;
        if capped {
            aggregate.capped = true;
        }
        let row = DirSizeRow {
            path: child.to_string_lossy().into_owned(),
            name: child
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size_kb: kb(bytes),
            parent: root.to_string_lossy().into_owned(),
            capped,
            scanned: used,
            aggregate: false,
        };
        if row.size_kb > 0 {
            children.push((row, bytes));
        }
        // Keep the working set bounded while producing rows.
        if children.len() >= limits.row_cap.saturating_mul(2) {
            children.sort_by_key(|(r, _)| std::cmp::Reverse(r.size_kb));
            children.truncate(limits.row_cap);
        }
    }
    // Leftover budget: refine the largest capped rows instead of letting a
    // floor stand when the pool still has entries to give. The better of the
    // two walks wins, so a refinement can only raise accuracy. Only capped
    // rows share the pool — a completed row has nothing left to measure.
    if *budget > 0 {
        children.sort_by_key(|(r, _)| std::cmp::Reverse(r.size_kb));
        let capped_rows: Vec<usize> = children
            .iter()
            .enumerate()
            .filter(|(_, (row, _))| row.capped)
            .map(|(i, _)| i)
            .collect();
        for (k, &i) in capped_rows.iter().enumerate() {
            if *budget == 0 {
                break;
            }
            let left = (capped_rows.len() - k) as u64;
            let mut slice = (*budget / left.max(1)).max(1).min(*budget);
            let before = slice;
            let path = PathBuf::from(&children[i].0.path);
            let (bytes, capped) = shallow_size_bytes(&path, limits.max_depth, &mut slice);
            let used = before - slice;
            *budget -= used;
            aggregate.scanned += used;
            if bytes > children[i].1 {
                child_bytes_total += bytes - children[i].1;
                children[i].1 = bytes;
                children[i].0.size_kb = kb(bytes);
                children[i].0.scanned = used;
            }
            children[i].0.capped = capped || used == 0;
            if children[i].0.capped {
                aggregate.capped = true;
            }
        }
    }
    children.sort_by_key(|(r, _)| std::cmp::Reverse(r.size_kb));
    children.truncate(limits.row_cap);
    let rows: Vec<DirSizeRow> = children.into_iter().map(|(r, _)| r).collect();
    aggregate.size_kb = kb(root_bytes + child_bytes_total);
    (aggregate, rows)
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
            let (aggregate, rows) = root_walk(&root, &RADAR_VISIT_LIMITS, &mut root_budget);
            budget = budget.saturating_sub(share.saturating_sub(root_budget));
            // A capped zero-size root is an unreadable root: keep the row so
            // the UI can say the total is unknown instead of hiding the root.
            if aggregate.size_kb > 0 || aggregate.capped {
                out.push(aggregate);
            }
            out.extend(rows);
        }
        rank_rows(&mut out, DISK_RADAR_TOP_N * 2);
    } else {
        let root = drive_root(letter);
        if root.is_dir() && is_local_fixed_drive(letter) {
            let (aggregate, rows) = root_walk(&root, &RADAR_VISIT_LIMITS, &mut budget);
            if aggregate.size_kb > 0 || aggregate.capped {
                out.push(aggregate);
            }
            out.extend(rows);
            rank_rows(&mut out, DISK_RADAR_TOP_N);
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

/// Immediate children of `path`, ranked by size and capped like the top view
/// (for drill-down). The same dynamic-slice walk as the top view: a huge first
/// sibling cannot starve the rest of the listing, and the list is always in
/// descending size order rather than filesystem enumeration order.
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
    let (_, mut rows) = root_walk(&p, &RADAR_DRILL_LIMITS, &mut budget);
    rank_rows(&mut rows, DISK_RADAR_TOP_N);
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
        assert!(
            super::root_walk(&link, &super::RADAR_DRILL_LIMITS, &mut 1_000)
                .1
                .is_empty()
        );
        assert_eq!(
            super::root_walk(&root, &super::RADAR_DRILL_LIMITS, &mut 1_000)
                .1
                .len(),
            1
        );
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
        let (aggregate, first) = super::root_walk(
            &tmp.path().join("root-a"),
            &super::RADAR_VISIT_LIMITS,
            &mut budget,
        );
        assert_eq!(first.len(), 1);
        assert!(aggregate.size_kb > 0);
        let (second_agg, second) = super::root_walk(
            &tmp.path().join("root-b"),
            &super::RADAR_VISIT_LIMITS,
            &mut budget,
        );
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
        let (aggregate, rows) = super::root_walk(&root, &super::RADAR_VISIT_LIMITS, &mut budget);
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
        let (aggregate, rows) = super::root_walk(&root, &super::RADAR_VISIT_LIMITS, &mut budget);
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

    /// The depth cap is measured in levels whose contents are read: a file in
    /// the browser cache seven levels below the listed row must be counted, and
    /// the same tree under a shallow cap must say so instead of guessing.
    #[test]
    fn radar_depth_counts_a_browser_cache_file_and_marks_a_shallower_cut() {
        let tmp = crate::test_support::TempDir::new("radar-depth");
        let row = tmp.path().join("Aaron");
        let deep = row
            .join("AppData")
            .join("Local")
            .join("Google")
            .join("Chrome")
            .join("User Data")
            .join("Default")
            .join("Cache");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("data.bin"), vec![0u8; 4096]).unwrap();

        let mut budget = 1_000u64;
        let (bytes, capped) =
            super::shallow_size_bytes(&row, super::RADAR_VISIT_LIMITS.max_depth, &mut budget);
        assert_eq!(bytes, 4096, "the cache file must be counted");
        assert!(!capped);

        let mut shallow_budget = 1_000u64;
        let (shallow_bytes, shallow_capped) =
            super::shallow_size_bytes(&row, 3, &mut shallow_budget);
        assert_eq!(shallow_bytes, 0, "a shallow cap cannot see that file");
        assert!(shallow_capped, "and it must say the number is a floor");
    }

    /// An unreadable root is a floor with a visible row, not silence.
    #[test]
    fn root_walk_reports_an_unreadable_root_as_a_floor() {
        let tmp = crate::test_support::TempDir::new("radar-unreadable");
        let file = tmp.path().join("not-a-directory");
        std::fs::write(&file, b"x").unwrap();
        let mut budget = 100u64;
        let (aggregate, rows) = super::root_walk(&file, &super::RADAR_DRILL_LIMITS, &mut budget);
        assert!(rows.is_empty());
        assert!(aggregate.capped, "an unreadable root is not a zero total");
        assert!(aggregate.aggregate);
        assert_eq!(aggregate.size_kb, 0);
    }

    /// Capped rows rank by their floor, the list is deterministic, and the cap
    /// is enforced — the drill-down contract the top view already had.
    #[test]
    fn rank_rows_orders_by_size_and_truncates() {
        let row = |name: &str, size_kb: i64| DirSizeRow {
            path: format!(r"C:\{name}"),
            name: name.to_string(),
            size_kb,
            parent: r"C:\".to_string(),
            capped: false,
            scanned: 0,
            aggregate: false,
        };
        let mut rows = vec![row("b", 10), row("a", 10), row("big", 900), row("small", 1)];
        super::rank_rows(&mut rows, 3);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["big", "a", "b"], "size desc, then path asc");
    }

    /// A pathologically wide level is cut and the aggregate becomes a floor
    /// instead of queueing without bound.
    #[test]
    fn root_walk_cuts_a_wide_level_and_marks_the_aggregate_capped() {
        let tmp = crate::test_support::TempDir::new("radar-queue-cap");
        let root = tmp.path().join("Wide");
        for i in 0..6 {
            let child = root.join(format!("c{i}"));
            std::fs::create_dir_all(&child).unwrap();
            std::fs::write(child.join("payload.bin"), vec![0u8; 4096]).unwrap();
        }
        let limits = super::RadarLimits {
            max_depth: 2,
            row_cap: 4,
            queue_cap: 3,
        };
        let mut budget = 1_000u64;
        let (aggregate, rows) = super::root_walk(&root, &limits, &mut budget);
        assert!(aggregate.capped, "a cut level is a floor");
        assert_eq!(rows.len(), 3, "only the queued children are walked");
        assert!(rows.iter().all(|r| !r.aggregate));
    }

    /// The leftover pool goes to the capped rows, so a floor can improve after
    /// the first pass instead of standing at the first slice's value.
    #[test]
    fn root_walk_refines_a_capped_row_with_leftover_budget() {
        let tmp = crate::test_support::TempDir::new("radar-refine");
        let root = tmp.path().join("Users");
        let big = root.join("aaa-big");
        std::fs::create_dir_all(&big).unwrap();
        for i in 0..10 {
            std::fs::write(big.join(format!("f{i}.bin")), vec![0u8; 4096]).unwrap();
        }
        // Two tiny siblings make the first slice small while leaving pool
        // entries unused — exactly the case the refinement pass exists for.
        for name in ["mmm-one", "zzz-two"] {
            let child = root.join(name);
            std::fs::create_dir_all(&child).unwrap();
            std::fs::write(child.join("payload.bin"), vec![0u8; 4096]).unwrap();
        }
        let mut budget = 12u64;
        let (_, rows) = super::root_walk(&root, &super::RADAR_VISIT_LIMITS, &mut budget);
        assert_eq!(rows.len(), 3);
        let big_row = rows.iter().find(|r| r.name == "aaa-big").unwrap();
        // First pass measured 3 of its files; the refinement's larger slice
        // measured 4, so the floor moved from 12 KB to 16 KB.
        assert_eq!(big_row.size_kb, 16, "the refined floor is larger");
        assert!(big_row.capped, "ten files still do not fit the budget");
        assert_eq!(budget, 0, "the pool was spent refining");
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
