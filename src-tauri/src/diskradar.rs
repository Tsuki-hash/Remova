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
    /// The walk was cut short — by the entry budget, the depth cap, the queue
    /// cap, an unreadable level or a cancelled scan — so size is a floor.
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

/// One resumable bounded walk over a subtree. Because it can stop and resume
/// where it left off, the budget can hand more entries to the largest
/// directories later without re-reading a prefix (a re-walk would burn budget
/// on entries already counted, which is how an earlier version starved the
/// largest directory and left most of the budget unused).
struct Walk {
    /// (levels below this directory that may still be read, its entries)
    stack: Vec<(u32, std::fs::ReadDir)>,
    bytes: i64,
    inspected: u64,
    finished: bool,
    capped: bool,
}

enum WalkStart {
    Walk(Walk),
    /// A junction/symlink: a pointer to storage counted where it lives.
    Reparse,
    /// Exists but cannot be enumerated: a floor, not a zero total.
    Unreadable,
}

fn start_walk(root: &std::path::Path, max_depth: u32) -> WalkStart {
    // The parent already classified this entry with its own metadata read; the
    // extra `symlink_metadata` here is a deliberate re-check for the window
    // between that read and opening the directory (a swapped junction would
    // otherwise make the radar charge another drive's bytes to this one).
    if crate::fsutil::is_reparse_point(root) {
        return WalkStart::Reparse;
    }
    match std::fs::read_dir(root) {
        Ok(rd) => WalkStart::Walk(Walk {
            stack: vec![(max_depth, rd)],
            bytes: 0,
            inspected: 0,
            finished: false,
            capped: false,
        }),
        Err(_) => WalkStart::Unreadable,
    }
}

impl Walk {
    /// Consume up to `budget` entries, resuming from where the last call
    /// stopped. `finished` means every directory in the subtree was read;
    /// otherwise `capped` says the number is a floor.
    fn advance(&mut self, budget: &mut u64, progress: &mut WalkProgress) {
        loop {
            let Some(top) = self.stack.len().checked_sub(1) else {
                self.finished = true;
                return;
            };
            if crate::scan_task::cancelled() || *budget == 0 {
                self.capped = true;
                return;
            }
            let depth_left = self.stack[top].0;
            let item = self.stack[top].1.next();
            match item {
                None => {
                    self.stack.pop();
                }
                Some(Err(_)) => {
                    *budget -= 1;
                    self.inspected += 1;
                    progress.tick();
                    self.capped = true;
                }
                Some(Ok(ent)) => {
                    *budget -= 1;
                    self.inspected += 1;
                    progress.tick();
                    let Ok(meta) = ent.metadata() else {
                        self.capped = true;
                        continue;
                    };
                    if crate::fsutil::is_reparse_metadata(&meta) {
                        continue;
                    }
                    if meta.is_dir() {
                        if depth_left == 0 {
                            // Content below the depth cap stays unmeasured.
                            self.capped = true;
                            continue;
                        }
                        match std::fs::read_dir(ent.path()) {
                            Ok(sub) => self.stack.push((depth_left - 1, sub)),
                            Err(_) => self.capped = true,
                        }
                    } else if meta.is_file() {
                        self.bytes += meta.len() as i64;
                    }
                }
            }
        }
    }

    /// True when the measured size is a floor rather than a total.
    fn is_floor(&self) -> bool {
        self.capped || !self.finished
    }
}

/// One-shot walk of a subtree with a fixed budget (test helper: the product
/// path drives the resumable `Walk` directly so the budget can be re-shared).
#[cfg(test)]
fn walk_subtree(root: &std::path::Path, max_depth: u32, budget: &mut u64) -> (i64, bool) {
    let mut progress = WalkProgress::new();
    match start_walk(root, max_depth) {
        WalkStart::Reparse => (0, false),
        WalkStart::Unreadable => (0, true),
        WalkStart::Walk(mut walk) => {
            walk.advance(budget, &mut progress);
            (walk.bytes, walk.is_floor())
        }
    }
}

/// Entries one radar call may inspect in total. Measured on this machine: a
/// 300k-entry pass of read_dir + metadata costs roughly ten seconds of wall
/// clock (debug build, warm cache), which is what a panel that opens by
/// scanning can ask for; everything beyond that ends as a marked floor.
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
/// deterministic; a floor row ranks by its measured value, and its marker says
/// the real size may be larger.
fn rank_rows(rows: &mut Vec<DirSizeRow>, cap: usize) {
    rows.sort_by(|a, b| b.size_kb.cmp(&a.size_kb).then_with(|| a.path.cmp(&b.path)));
    rows.truncate(cap);
}

/// One bounded walk over `root` that yields an aggregate row for the root
/// itself plus rows for its direct child directories. One traversal serves
/// both: every byte found below a child is attributed to that child's row and
/// summed into the root aggregate; files directly under the root only count
/// toward the aggregate.
///
/// Budget is handed out by water filling: every unfinished child gets an equal
/// share of what is left each round, so a small child finishes early and its
/// unused share flows to the largest ones. The walk therefore stops because the
/// budget is gone, not because the first child happened to be first.
///
/// `omitted` counts rows the caller will not see: children queued but never
/// walked plus rows dropped by the row cap. A level cut by `queue_cap` (the
/// directory had more direct children than the cap) and unreadable levels are
/// reported through `capped` instead — their exact count is not known.
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
    if crate::fsutil::is_reparse_point(root) {
        return (aggregate, Vec::new(), 0);
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        // Unreadable root: report the row as an explicit floor, not silence.
        aggregate.capped = true;
        return (aggregate, Vec::new(), 0);
    };
    let start = progress.inspected;
    // Classify the level directly under the root: files sum into the aggregate,
    // child directories become one resumable walk each.
    let mut child_dirs: Vec<PathBuf> = Vec::new();
    let mut root_bytes: i64 = 0;
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
    let mut walks: Vec<(PathBuf, Option<Walk>)> = child_dirs
        .iter()
        .map(|child| match start_walk(child, limits.max_depth) {
            WalkStart::Walk(walk) => (child.clone(), Some(walk)),
            WalkStart::Reparse => (child.clone(), None),
            WalkStart::Unreadable => (
                child.clone(),
                Some(Walk {
                    stack: Vec::new(),
                    bytes: 0,
                    inspected: 0,
                    finished: true,
                    capped: true,
                }),
            ),
        })
        .collect();
    // Water filling: equal shares for every unfinished child, repeated until the
    // pool is empty or all children are fully measured.
    loop {
        let active: Vec<usize> = walks
            .iter()
            .enumerate()
            .filter(|(_, (_, walk))| walk.as_ref().is_some_and(|w| !w.finished))
            .map(|(i, _)| i)
            .collect();
        if active.is_empty() || *budget == 0 {
            break;
        }
        let share = (*budget / active.len() as u64).max(1);
        let mut spent_any = false;
        for &i in &active {
            if *budget == 0 {
                break;
            }
            let Some(walk) = walks[i].1.as_mut() else {
                continue;
            };
            let mut slice = share.min(*budget);
            let before = slice;
            walk.advance(&mut slice, progress);
            let spent = before - slice;
            *budget = budget.saturating_sub(spent);
            spent_any |= spent > 0;
        }
        if !spent_any {
            break;
        }
    }
    let mut children: Vec<DirSizeRow> = Vec::new();
    let mut child_bytes_total: i64 = 0;
    for (path, walk) in walks {
        match walk {
            None => {
                // Reparse child: not part of this root's storage, not a row.
            }
            Some(w) => {
                child_bytes_total += w.bytes;
                if w.is_floor() {
                    aggregate.capped = true;
                }
                let row = DirSizeRow {
                    path: path.to_string_lossy().into_owned(),
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    size_kb: kb(w.bytes),
                    parent: root.to_string_lossy().into_owned(),
                    capped: w.is_floor(),
                    scanned: w.inspected,
                    aggregate: false,
                };
                // An unreadable or unmeasured child is a floor worth showing; a
                // proven-empty one is not a row the user can act on.
                if row.size_kb > 0 || row.capped {
                    children.push(row);
                }
            }
        }
    }
    // Rank the measured rows, then keep every unmeasured floor that still fits:
    // a directory we could not read has no bytes to rank by, and dropping it
    // would hide exactly the case the user needs to see.
    let total = children.len() as u64;
    let mut floors: Vec<DirSizeRow> = children
        .iter()
        .filter(|r| r.size_kb == 0)
        .cloned()
        .collect();
    let mut measured: Vec<DirSizeRow> = children.into_iter().filter(|r| r.size_kb > 0).collect();
    rank_rows(&mut measured, limits.row_cap);
    let room = limits.row_cap.saturating_sub(measured.len());
    floors.truncate(room);
    let kept = (measured.len() + floors.len()) as u64;
    measured.extend(floors);
    let omitted = total.saturating_sub(kept);
    aggregate.scanned = progress.inspected - start;
    aggregate.size_kb = kb(root_bytes + child_bytes_total);
    (aggregate, measured, omitted)
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
///
/// `omitted` counts rows the caller will not see: children never walked plus
/// rows dropped by the row cap. A level cut by the queue cap is not counted
/// here (the remaining count is unknown) — it shows up as `capped`.
///
/// `capped` means the listing is not the whole story: the parent scan was cut
/// short, or a child could not be measured in full. Either way the UI must say
/// so instead of presenting the list as complete.
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

    /// Write `count` files of `size` bytes into `dir`.
    fn seed(dir: &std::path::Path, count: usize, size: usize) {
        std::fs::create_dir_all(dir).unwrap();
        for i in 0..count {
            std::fs::write(dir.join(format!("f{i:04}.bin")), vec![0u8; size]).unwrap();
        }
    }

    #[test]
    fn cancelled_radar_walk_keeps_budget_and_rejects_partial_results() {
        let id = crate::scan_task::begin().unwrap();
        let mut budget = 20;
        let result = crate::scan_task::run(id, || {
            crate::scan_task::cancel(id).unwrap();
            super::walk_subtree(
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
        assert_eq!(super::walk_subtree(&root, 3, &mut 20), (0, true));
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
        assert_eq!(super::walk_subtree(&link, 3, &mut 100), (0, false));
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
        assert_eq!(super::walk_subtree(&target, 3, &mut 100), (5, false));
    }

    #[test]
    fn nested_small_files_are_rounded_only_once_and_caps_are_reported() {
        let root = fixture("bytes");
        for name in ["a", "b"] {
            seed(&root.path().join(name), 1, 1);
        }
        assert_eq!(super::walk_subtree(root.path(), 3, &mut 100), (2, false));
        assert!(
            super::walk_subtree(root.path(), 3, &mut 1).1,
            "an exhausted budget is a floor"
        );
    }

    /// The depth cap counts levels whose contents are read. Nine levels reach
    /// the files inside a real browser cache
    /// (`...\Default\Cache\Cache_Data\data.bin`); a shallower cap must say so
    /// instead of guessing.
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
        seed(&cache, 1, 4096);

        let mut budget = 1_000u64;
        assert_eq!(
            super::walk_subtree(&row, RADAR_MAX_DEPTH, &mut budget),
            (4096, false),
            "the cache file must be counted"
        );
        let mut shallow_budget = 1_000u64;
        assert_eq!(super::walk_subtree(&row, 3, &mut shallow_budget).0, 0);
        assert!(super::walk_subtree(&row, 3, &mut shallow_budget).1);
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
        seed(&deep, 1, 8192);

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
        seed(&top.join("alice"), 1, 4096);
        seed(&top.join("bob"), 1, 4096);
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
        // 3 child dirs + 1 root file classified, plus one entry per child walk.
        assert_eq!(aggregate.scanned, 6);
        for row in &rows {
            assert_eq!(row.size_kb, 4);
            assert!(!row.capped);
            assert!(!row.aggregate);
            assert!(row.scanned > 0);
            assert_eq!(row.parent, top.to_string_lossy());
        }
        assert!(
            budget > 0,
            "unused budget is returned when every walk finished"
        );
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

    /// A child whose walk was cut before it found a file is still a row: the
    /// user must be able to see that this directory exists and is unmeasured.
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

    /// Capped rows rank by their measured value, the list is deterministic, and
    /// the cap is enforced.
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

    /// A pathologically wide level is cut, and the aggregate becomes a floor.
    /// The cut children are not in `omitted` (their number is unknown); the
    /// floor marker is what tells the caller the list is partial.
    #[test]
    fn root_walk_cuts_a_wide_level_and_marks_the_aggregate_capped() {
        let root = fixture("queue-cap");
        let top = root.path().join("Wide");
        for i in 0..6 {
            seed(&top.join(format!("c{i}")), 1, 4096);
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
        for i in 1..=10u8 {
            seed(&top.join(format!("c{i:02}")), 1, i as usize * 1024);
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

    /// Water filling: a huge child keeps receiving the pool until the budget is
    /// gone instead of being frozen at its first equal share — the user-visible
    /// failure this allocation exists to prevent.
    #[test]
    fn root_walk_water_fills_the_largest_child() {
        let root = fixture("water-fill");
        let top = root.path().join("Users");
        seed(&top.join("aaa-big"), 100, 4096);
        seed(&top.join("zzz-small"), 1, 4096);
        let mut budget = 40u64;
        let (_, rows, _) =
            super::root_walk(&top, &RADAR_VISIT_LIMITS, &mut budget, &mut progress());
        let big = rows.iter().find(|r| r.name == "aaa-big").unwrap();
        let small = rows.iter().find(|r| r.name == "zzz-small").unwrap();
        // 2 classification entries + 38 of the 40-entry pool: the small child
        // finishes on its first share, and every later round goes to the big one.
        assert_eq!(big.scanned, 37);
        assert!(big.capped, "100 files do not fit a 40-entry budget");
        assert_eq!(small.scanned, 1);
        assert!(!small.capped);
        assert_eq!(budget, 0, "the pool is spent, not left idle");
    }

    /// The panel's "checked N" line comes from `WalkProgress` publishing to the
    /// scan task; without a report the counter stays at zero for the whole scan.
    #[test]
    fn walk_progress_publishes_the_checked_count_during_a_scan() {
        let id = crate::scan_task::begin().unwrap();
        let (before, after) = crate::scan_task::run(id, || {
            let mut p = WalkProgress::new();
            for _ in 0..(RADAR_REPORT_EVERY - 1) {
                p.tick();
            }
            let before = crate::scan_task::progress(id).map_or(0, |pr| pr.found);
            // Crossing the threshold is what pushes a report out.
            p.tick();
            let after = crate::scan_task::progress(id).map_or(0, |pr| pr.found);
            (before, after)
        })
        .unwrap();
        assert_eq!(before, 0, "no report before the first threshold");
        assert_eq!(
            after, RADAR_REPORT_EVERY as usize,
            "the checked count is published once the threshold is crossed"
        );
    }

    /// Sibling fairness: a late sibling still gets a first share before the big
    /// one consumes the pool.
    #[test]
    fn root_walk_gives_a_late_sibling_its_first_share() {
        let root = fixture("fair-first-share");
        let top = root.path().join("Users");
        seed(&top.join("aaa-big"), 10, 4096);
        seed(&top.join("zzz-small"), 2, 4096);
        let mut budget = 8u64;
        let (_, rows, _) =
            super::root_walk(&top, &RADAR_VISIT_LIMITS, &mut budget, &mut progress());
        let small = rows.iter().find(|r| r.name == "zzz-small").unwrap();
        assert!(
            small.size_kb > 0,
            "the late sibling must be measured before the pool runs dry"
        );
        assert!(!small.capped, "two files fit one share");
    }

    /// The budget is shared across roots: a second root cannot reset it.
    #[test]
    fn radar_budget_is_shared_across_roots() {
        let root = fixture("shared-budget");
        for name in ["root-a", "root-b"] {
            seed(&root.path().join(name).join("child"), 1, 4096);
        }
        let mut shared = 3u64;
        let (first_aggregate, first, _) = super::root_walk(
            &root.path().join("root-a"),
            &RADAR_VISIT_LIMITS,
            &mut shared,
            &mut progress(),
        );
        assert_eq!(first.len(), 1);
        assert!(first_aggregate.size_kb > 0);
        let (second_aggregate, second, _) = super::root_walk(
            &root.path().join("root-b"),
            &RADAR_VISIT_LIMITS,
            &mut shared,
            &mut progress(),
        );
        assert!(
            second.iter().all(|r| r.size_kb == 0 && r.capped),
            "a second root must not reset the call-wide budget"
        );
        assert!(second_aggregate.capped, "a starved root reports a floor");
    }

    /// `list_dir_children` refuses blank paths and anything outside the radar
    /// allow-list — the only boundary on the drill-down IPC command.
    #[test]
    fn list_dir_children_rejects_blank_and_outside_paths() {
        assert!(list_dir_children("").is_err());
        assert!(list_dir_children("   ").is_err());
        assert!(list_dir_children(r"C:\Games\Steam").is_err());
    }

    #[test]
    fn under_radar_roots_whitelist() {
        let sys = system_drive_letter();
        let ok = |leaf: &str| super::under_radar_roots(&format!(r"{sys}:\{leaf}"));
        assert!(ok(r"Users\a\Documents"));
        assert!(ok(r"Program Files\App"));
        assert!(ok("Windows\\Temp"));
        // System-drive non-root trees stay restricted.
        assert!(!ok("Games\\Steam"));
        // UNC / device prefixes never allowed.
        assert!(!super::under_radar_roots(r"\\server\share\file"));
        assert!(!super::under_radar_roots(r"\\?\C:\x"));
        // Non-system local fixed drives are browsable (letter-dependent).
        let d_ok = super::is_local_fixed_drive('D');
        assert_eq!(super::under_radar_roots(r"D:\Games\Steam"), d_ok);
    }

    /// `..` must never pass the radar roots check (string prefix or canonicalize).
    #[test]
    fn drilldown_rejects_traversal_outside_roots() {
        let sys = system_drive_letter();
        let trav = |tail: &str| format!(r"{sys}:\Users\a\Documents\{tail}");
        assert!(!super::under_radar_roots(&trav(
            r"..\..\..\Windows\System32"
        )));
        assert!(!super::under_radar_roots(&trav(r"..\..\Temp\evil")));
        assert!(!super::under_radar_roots(&trav(r"..\secret")));
        // Clean paths under roots still pass.
        assert!(super::under_radar_roots(&trav("keep")));
        // Drill-down refuses traversal shapes.
        assert!(list_dir_children(&trav(r"..\..\..\Windows\System32")).is_err());
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
        // Ranked rows plus one diagnostic per unmeasurable root: the length is
        // bounded, but the bound includes the diagnostics (they bypass the
        // ranking cutoff on purpose).
        let roots = super::system_drive_roots().len();
        assert!(
            rows.len() <= DISK_RADAR_TOP_N * 2 + roots,
            "the ranked list is capped, diagnostics included"
        );
    }

    #[test]
    fn top_for_unknown_drive_is_empty() {
        // Phantom drive letter should not panic and should return no rows.
        let rows = super::top_dir_sizes_with_budget(Some('Q'), 20_000);
        assert!(rows.is_empty() || rows.iter().all(|r| r.path.starts_with("Q:")));
    }
}
