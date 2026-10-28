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
fn shallow_size_bytes(
    root: &std::path::Path,
    max_depth: u32,
    budget: &mut u64,
    progress: &mut WalkProgress,
) -> (i64, bool) {
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
        progress.tick();
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
            let (bytes_below, below_capped) =
                shallow_size_bytes(&p, max_depth - 1, budget, progress);
            bytes += bytes_below;
            capped |= below_capped;
        } else if meta.is_file() {
            bytes += meta.len() as i64;
        }
    }
    (bytes, capped)
}

/// Entries one radar call may inspect in total. Measured locally: 300k
/// entries of read_dir + metadata cost a few seconds, which is the most a
/// panel that opens by scanning may ask of the user; a deeper or wider tree
/// simply ends as a marked floor.
const RADAR_CALL_BUDGET: u64 = 300_000;

/// Directory levels below a listed row whose contents are read. Nine levels
/// reach the files inside
/// `Users\<user>\AppData\Local\Google\Chrome\User Data\Default\Cache\Cache_Data`,
/// which is where Chrome and Edge keep their cache bytes (a browser cache
/// nested deeper still ends as a marked floor).
const RADAR_MAX_DEPTH: u32 = 9;

/// Direct child directories queued for one root. A wider level is cut and the
/// aggregate becomes a floor, so a pathological directory cannot grow the
/// queue without bound.
const RADAR_QUEUE_CAP: usize = 4096;

/// How often a walk reports progress. The panel polls "found" every 250ms; a
/// walk that never reports leaves it reading zero for the whole scan.
const RADAR_REPORT_EVERY: u64 = 4096;

/// Limits that keep one radar call predictable.
struct RadarLimits {
    /// Directory levels below a listed row whose contents are read.
    max_depth: u32,
    /// Rows kept per listed root; the final list is ranked again.
    row_cap: usize,
    /// Direct child directories queued for one root.
    queue_cap: usize,
}

/// Top view: each system root keeps twice the final cutoff so the combined
/// ranking still sees every candidate.
const RADAR_VISIT_LIMITS: RadarLimits = RadarLimits {
    max_depth: RADAR_MAX_DEPTH,
    row_cap: DISK_RADAR_TOP_N * 2,
    queue_cap: RADAR_QUEUE_CAP,
};

/// Drill-down: one parent lists at most the final cutoff, like the top view.
const RADAR_DRILL_LIMITS: RadarLimits = RadarLimits {
    max_depth: RADAR_MAX_DEPTH,
    row_cap: DISK_RADAR_TOP_N,
    queue_cap: RADAR_QUEUE_CAP,
};

/// Progress accounting shared by one call: counts inspected entries and ticks
/// the scan task often enough that the panel's "found" line moves during a long
/// walk instead of staying at zero.
struct WalkProgress {
    inspected: u64,
    next_report: u64,
}

impl WalkProgress {
    fn new() -> Self {
        Self {
            inspected: 0,
            next_report: RADAR_REPORT_EVERY,
        }
    }

    fn tick(&mut self) {
        self.inspected += 1;
        if self.inspected >= self.next_report {
            self.next_report = self.inspected.saturating_add(RADAR_REPORT_EVERY);
            crate::scan_task::checkpoint("sizes", self.inspected as usize);
        }
    }
}

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
/// `omitted` counts direct children the caller will not see (never walked, or
/// dropped by the row cap).
fn root_walk(
    root: &std::path::Path,
    limits: &RadarLimits,
    budget: &mut u64,
    progress: &mut WalkProgress,
) -> (DirSizeRow, Vec<DirSizeRow>, u64) {
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
    // A reparse root is a pointer to storage counted where it lives; walking it
    // would attribute another drive's bytes to this one, and the resulting
    // aggregate row could not be drilled into (the allow-list refuses reparse
    // paths). Skip it and keep the row off the list.
    if !crate::scan_task::checkpoint("sizes", progress.inspected as usize)
        || crate::fsutil::is_reparse_point(root)
    {
        return (aggregate, Vec::new(), 0);
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        // Unreadable root: report the row as an explicit floor, not silence.
        aggregate.capped = true;
        return (aggregate, Vec::new(), 0);
    };
    let start = progress.inspected;
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
        progress.tick();
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
    let mut children: Vec<DirSizeRow> = Vec::new();
    let mut walked = 0u64;
    let mut dropped = 0u64;
    let mut remaining = child_dirs.len();
    for child in &child_dirs {
        if crate::scan_task::cancelled() || *budget == 0 {
            aggregate.capped = true;
            break;
        }
        remaining = remaining.saturating_sub(1);
        let mut slice = (*budget / (remaining as u64 + 1)).max(1).min(*budget);
        let before = progress.inspected;
        let slice_before = slice;
        let (bytes, capped) = shallow_size_bytes(child, limits.max_depth, &mut slice, progress);
        let spent = slice_before - slice;
        let used = progress.inspected - before;
        *budget = budget.saturating_sub(spent);
        walked += 1;
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
        // An unreadable child is a floor worth showing; a proven-empty one is
        // not a row the user can act on.
        if row.size_kb > 0 || row.capped {
            children.push(row);
        }
        // Keep the working set bounded while producing rows.
        if children.len() >= limits.row_cap.saturating_mul(2) {
            let before = children.len();
            children.sort_by_key(|r| std::cmp::Reverse(r.size_kb));
            children.truncate(limits.row_cap);
            dropped += (before - children.len()) as u64;
        }
    }
    children.sort_by_key(|r| std::cmp::Reverse(r.size_kb));
    let before_final = children.len();
    children.truncate(limits.row_cap);
    dropped += (before_final - children.len()) as u64;
    // Children never walked (queue/budget cut) plus rows dropped by the row
    // cap: the caller must be able to say the list is partial.
    let omitted = (child_dirs.len() as u64).saturating_sub(walked) + dropped;
    aggregate.scanned = progress.inspected - start;
    aggregate.size_kb = kb(root_bytes + child_bytes_total);
    (aggregate, children, omitted)
}

/// Top directories: system drive well-known roots, or one local drive's `X:\` children.
/// Each well-known root contributes its own aggregate row ("where did the
/// drive go at this root") plus its largest children, ranked together.
pub fn top_dir_sizes_for_drive(letter: Option<char>) -> Vec<DirSizeRow> {
    top_dir_sizes_with_budget(letter, RADAR_CALL_BUDGET)
}

/// The top view with an explicit entry budget; the test suite injects a
/// small one so a real-disk assertion stays fast instead of scanning for
/// minutes through `RADAR_CALL_BUDGET`.
fn top_dir_sizes_with_budget(letter: Option<char>, budget_cap: u64) -> Vec<DirSizeRow> {
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
    // Rows kept outside the size ranking: an unreadable (or unmeasured) root is
    // a diagnostic the user must see even though its 0 KB would sort it out of
    // any cutoff.
    let mut diagnostics: Vec<DirSizeRow> = Vec::new();
    let mut progress = WalkProgress::new();
    // One budget for the whole call, shared by every root.
    let mut budget = budget_cap;
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
            let (aggregate, rows, _) =
                root_walk(&root, &RADAR_VISIT_LIMITS, &mut root_budget, &mut progress);
            budget = budget.saturating_sub(share.saturating_sub(root_budget));
            if aggregate.size_kb > 0 {
                out.push(aggregate);
            } else if aggregate.capped {
                diagnostics.push(aggregate);
            }
            out.extend(rows);
        }
        rank_rows(&mut out, DISK_RADAR_TOP_N * 2);
    } else {
        let root = drive_root(letter);
        if root.is_dir() && is_local_fixed_drive(letter) {
            let (aggregate, rows, _) =
                root_walk(&root, &RADAR_VISIT_LIMITS, &mut budget, &mut progress);
            if aggregate.size_kb > 0 {
                out.push(aggregate);
            } else if aggregate.capped {
                diagnostics.push(aggregate);
            }
            out.extend(rows);
            rank_rows(&mut out, DISK_RADAR_TOP_N);
        }
    }
    // Diagnostics are appended after the cutoff: a root we could not measure is
    // exactly what the user needs to know, and its 0 KB would never rank in.
    out.extend(diagnostics);
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

/// Drill-down result: the ranked children plus what the list does not show.
/// `omitted` counts child directories the walk never reached or the row cap
/// dropped; `capped` says the parent's own scan was cut short (budget, queue
/// cap or an unreadable level). Either one means the list is not the whole
/// story and the UI must say so.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirChildrenResult {
    pub rows: Vec<DirSizeRow>,
    #[serde(default)]
    pub omitted: u64,
    #[serde(default)]
    pub capped: bool,
}

/// Immediate children of `path`, ranked by size and capped like the top view
/// (for drill-down). The same dynamic-slice walk as the top view: a huge first
/// sibling cannot starve the rest of the listing, and the list is always in
/// descending size order rather than filesystem enumeration order.
pub fn list_dir_children(path: &str) -> Result<DirChildrenResult, String> {
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
    let mut progress = WalkProgress::new();
    let (aggregate, mut rows, omitted) =
        root_walk(&p, &RADAR_DRILL_LIMITS, &mut budget, &mut progress);
    rank_rows(&mut rows, DISK_RADAR_TOP_N);
    Ok(DirChildrenResult {
        rows,
        omitted,
        capped: aggregate.capped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> crate::test_support::TempDir {
        crate::test_support::TempDir::new(&format!("radar-{tag}"))
    }

    fn progress() -> WalkProgress {
        WalkProgress::new()
    }

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
                &mut progress(),
            )
        });
        assert_eq!(budget, 20);
        assert_eq!(result, Err("scan:cancelled".into()));
    }

    #[test]
    fn failed_directory_read_marks_radar_size_as_a_lower_bound() {
        let root = std::env::temp_dir().join(format!("remova-radar-io-{}", std::process::id()));
        std::fs::write(&root, b"not a directory").unwrap();
        assert_eq!(
            super::shallow_size_bytes(&root, 3, &mut 20, &mut progress()),
            (0, true)
        );
        std::fs::remove_file(root).unwrap();
    }

    /// A junction is a pointer, not storage: neither a reparse root nor a
    /// reparse child contributes bytes, and a reparse root must not produce an
    /// aggregate row at all (its target belongs to another drive).
    #[test]
    #[cfg(windows)]
    fn radar_skips_junction_roots_and_child_junction_rows() {
        let root = fixture("junction");
        let target = root.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("data"), b"kept").unwrap();
        std::fs::create_dir(target.join("sub")).unwrap();
        std::fs::write(target.join("sub").join("more"), b"x").unwrap();
        let link = root.path().join("link");
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            super::shallow_size_bytes(&link, 3, &mut 100, &mut progress()),
            (0, false)
        );
        // The junction root is skipped outright: the target lives elsewhere, so
        // counting it here would charge another drive's bytes to this one.
        let (junction_aggregate, junction_rows, _) =
            super::root_walk(&link, &RADAR_DRILL_LIMITS, &mut 1_000, &mut progress());
        assert!(junction_rows.is_empty());
        assert_eq!(junction_aggregate.size_kb, 0);
        assert!(!junction_aggregate.capped);
        // A real root lists the real child but never the junction.
        let (_, rows, _) = super::root_walk(
            root.path(),
            &RADAR_DRILL_LIMITS,
            &mut 1_000,
            &mut progress(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(
            super::shallow_size_bytes(&target, 3, &mut 100, &mut progress()),
            (5, false)
        );
    }

    #[test]
    fn nested_small_files_are_rounded_only_once_and_caps_are_reported() {
        let root = fixture("bytes");
        for name in ["a", "b"] {
            std::fs::create_dir_all(root.path().join(name)).unwrap();
            std::fs::write(root.path().join(name).join("file"), b"x").unwrap();
        }
        assert_eq!(
            super::shallow_size_bytes(root.path(), 3, &mut 100, &mut progress()),
            (2, false)
        );
        assert!(
            super::shallow_size_bytes(root.path(), 3, &mut 1, &mut progress()).1,
            "an exhausted budget is a floor"
        );
    }

    /// The depth cap counts levels whose contents are read. Nine levels reach
    /// the files inside a real browser cache
    /// (`...\Default\Cache\Cache_Data\data.bin`); a shallower cap must say so
    /// instead of guessing, and a deeper tree ends as a floor.
    #[test]
    fn radar_depth_counts_a_browser_cache_file() {
        let root = fixture("depth");
        let row = root.path().join("Aaron");
        let cache = row
            .join("AppData")
            .join("Local")
            .join("Google")
            .join("Chrome")
            .join("User Data")
            .join("Default")
            .join("Cache")
            .join("Cache_Data");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("data.bin"), vec![0u8; 4096]).unwrap();

        let mut budget = 1_000u64;
        let (bytes, capped) =
            super::shallow_size_bytes(&row, RADAR_MAX_DEPTH, &mut budget, &mut progress());
        assert_eq!(bytes, 4096, "the cache file must be counted");
        assert!(!capped);

        let mut shallow_budget = 1_000u64;
        let (shallow_bytes, shallow_capped) =
            super::shallow_size_bytes(&row, 3, &mut shallow_budget, &mut progress());
        assert_eq!(shallow_bytes, 0, "a shallow cap cannot see that file");
        assert!(shallow_capped, "and it must say the number is a floor");
    }

    /// The walk wiring, not just the helper: a deep file below a listed child
    /// is counted through root_walk, and a shallow limit marks that child.
    #[test]
    fn root_walk_wires_the_depth_limit_into_child_walks() {
        let root = fixture("wiring");
        let child = root.path().join("child");
        let mut deep = child.clone();
        for i in 1..=7 {
            deep = deep.join(format!("l{i}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("deep.bin"), vec![0u8; 8192]).unwrap();

        let mut budget = 1_000u64;
        let (_, rows, _) = super::root_walk(
            root.path(),
            &RADAR_VISIT_LIMITS,
            &mut budget,
            &mut progress(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].size_kb, 8, "the deep file is counted");
        assert!(!rows[0].capped);

        let shallow = RadarLimits {
            max_depth: 3,
            row_cap: 8,
            queue_cap: 32,
        };
        let mut shallow_budget = 1_000u64;
        let (_, shallow_rows, _) =
            super::root_walk(root.path(), &shallow, &mut shallow_budget, &mut progress());
        assert_eq!(shallow_rows.len(), 1);
        assert!(
            shallow_rows[0].capped,
            "the shallow walk says it is a floor"
        );
    }

    #[test]
    fn root_walk_reports_the_aggregate_and_children_together() {
        let root = fixture("aggregate");
        let top = root.path().join("Users");
        for name in ["alice", "bob"] {
            let child = top.join(name);
            std::fs::create_dir_all(&child).unwrap();
            std::fs::write(child.join("payload.bin"), vec![0u8; 4096]).unwrap();
        }
        std::fs::create_dir_all(top.join("empty")).unwrap();
        std::fs::write(top.join("root-file.cfg"), vec![0u8; 100]).unwrap();
        let mut budget = 1_000u64;
        let (aggregate, rows, omitted) =
            super::root_walk(&top, &RADAR_VISIT_LIMITS, &mut budget, &mut progress());
        // 4096 + 4096 + 100 bytes = 8292 -> 9 KB, rounded once for the whole
        // aggregate; the empty child is not listed, files at the root level
        // count toward the aggregate only.
        assert_eq!(aggregate.size_kb, 9);
        assert!(aggregate.aggregate, "the root row is a total");
        assert_eq!(rows.len(), 2, "zero-size children are not listed");
        assert_eq!(omitted, 0);
        // 3 child dirs + 1 root file + the two child walks' single entry each.
        assert_eq!(aggregate.scanned, 6);
        for row in &rows {
            assert_eq!(row.size_kb, 4);
            assert!(!row.capped);
            assert!(!row.aggregate);
            assert!(row.scanned > 0);
            assert_eq!(row.parent, top.to_string_lossy());
        }
        assert!(budget > 0, "unused slice budget returns to the pool");
    }

    /// An unreadable root is a floor with a visible row, not silence.
    #[test]
    fn root_walk_reports_an_unreadable_root_as_a_floor() {
        let root = fixture("unreadable");
        let file = root.path().join("not-a-directory");
        std::fs::write(&file, b"x").unwrap();
        let mut budget = 100u64;
        let (aggregate, rows, omitted) =
            super::root_walk(&file, &RADAR_DRILL_LIMITS, &mut budget, &mut progress());
        assert!(rows.is_empty());
        assert_eq!(omitted, 0);
        assert!(aggregate.capped, "an unreadable root is not a zero total");
        assert!(aggregate.aggregate);
        assert_eq!(aggregate.size_kb, 0);
    }

    /// A child whose own walk was cut before it found a file is still a row:
    /// the user must be able to see that this directory exists and is
    /// unmeasured, instead of it vanishing into a "nothing here" list.
    #[test]
    fn root_walk_keeps_a_zero_byte_capped_child() {
        let root = fixture("zero-capped");
        let top = root.path().join("Users");
        let child = top.join("big");
        for i in 0..10 {
            std::fs::create_dir_all(child.join(format!("d{i}"))).unwrap();
        }
        let mut budget = 2u64;
        let (_, rows, _) =
            super::root_walk(&top, &RADAR_VISIT_LIMITS, &mut budget, &mut progress());
        assert_eq!(rows.len(), 1, "the unmeasured child is listed");
        assert_eq!(rows[0].size_kb, 0);
        assert!(rows[0].capped);
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

    /// A pathologically wide level is cut, the aggregate becomes a floor, and
    /// the cut children are reported as omitted instead of disappearing.
    #[test]
    fn root_walk_cuts_a_wide_level_and_reports_the_omitted_children() {
        let root = fixture("queue-cap");
        let top = root.path().join("Wide");
        for i in 0..6 {
            let child = top.join(format!("c{i}"));
            std::fs::create_dir_all(&child).unwrap();
            std::fs::write(child.join("payload.bin"), vec![0u8; 4096]).unwrap();
        }
        let limits = RadarLimits {
            max_depth: 2,
            row_cap: 4,
            queue_cap: 3,
        };
        let mut budget = 1_000u64;
        let (aggregate, rows, omitted) =
            super::root_walk(&top, &limits, &mut budget, &mut progress());
        assert!(aggregate.capped, "a cut level is a floor");
        assert_eq!(rows.len(), 3, "only the queued children are walked");
        assert!(rows.iter().all(|r| !r.aggregate));
        assert_eq!(omitted, 0, "the cut happened at the queue, not at the rows");
    }

    /// More listed children than the row cap: the returned list is the largest
    /// rows, the omitted count accounts for the rest, and the aggregate still
    /// sums every walked child (not just the returned ones).
    #[test]
    fn root_walk_keeps_the_largest_rows_and_counts_the_rest_as_omitted() {
        let root = fixture("row-cap");
        let top = root.path().join("Users");
        // 10 children of 1..=10 KB; a cap of 4 must keep the four largest.
        for i in 1..=10u8 {
            let child = top.join(format!("c{i:02}"));
            std::fs::create_dir_all(&child).unwrap();
            std::fs::write(child.join("payload.bin"), vec![0u8; i as usize * 1024]).unwrap();
        }
        let limits = RadarLimits {
            max_depth: 2,
            row_cap: 4,
            queue_cap: 32,
        };
        let mut budget = 10_000u64;
        let (aggregate, rows, omitted) =
            super::root_walk(&top, &limits, &mut budget, &mut progress());
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter().map(|r| r.size_kb).collect::<Vec<_>>(),
            vec![10, 9, 8, 7]
        );
        assert_eq!(omitted, 6, "six walked children did not fit the cap");
        // The aggregate sums every child (1+..+10 KB), not only the returned
        // four: the root total must not shrink because the list was capped.
        assert_eq!(aggregate.size_kb, 55);
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

    /// A real scan with a small budget must still produce rows: an empty result
    /// would mean "this machine is clean", which is exactly the user-visible
    /// failure this walk exists to avoid.
    #[test]
    fn top_dir_sizes_produces_rows_on_a_real_drive() {
        let rows = super::top_dir_sizes_with_budget(None, 20_000);
        assert!(!rows.is_empty(), "a real system drive must list something");
        assert!(
            rows.len() <= DISK_RADAR_TOP_N * 2,
            "the ranked list is capped"
        );
        assert!(
            rows.iter().any(|r| r.aggregate),
            "the root totals are part of the list"
        );
        assert!(
            rows.iter().any(|r| r.size_kb > 0),
            "at least one row measured real bytes"
        );
        for r in &rows {
            assert!(!r.path.is_empty());
            assert!(r.size_kb >= 0);
        }
    }

    #[test]
    fn top_for_unknown_drive_is_empty() {
        // Phantom drive letter should not panic and should return no rows.
        let rows = super::top_dir_sizes_with_budget(Some('Q'), 20_000);
        assert!(rows.is_empty() || rows.iter().all(|r| r.path.starts_with("Q:")));
    }
}
