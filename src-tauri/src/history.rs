//! Local cleanup history jsonl.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Runtime row id (`H{hash}-{nth}`). Empty on disk; filled by [`load`] for the UI/API.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub app_name: String,
    pub deleted: u32,
    pub failed: u32,
    pub skipped: u32,
    #[serde(default)]
    pub delayed: u32,
    pub aborted: bool,
    pub dry_run: bool,
    pub backup_dir: String,
    pub created_at: String,
}

/// Soft cap: compact when the log grows past 2 MiB (keep newest 2000 rows).
const HISTORY_SOFT_CAP: u64 = 2 * 1024 * 1024;
/// Hard read bound — never load more than this into memory (N-risk).
const HISTORY_HARD_CAP: u64 = 8 * 1024 * 1024;
const HISTORY_KEEP_LINES: usize = 2000;

fn history_path() -> PathBuf {
    let pd = std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Remova").join("history.jsonl")
}

/// Read history text under a hard size bound. Oversized files are compacted first.
fn read_history_bounded(p: &Path) -> Option<String> {
    let meta = fs::metadata(p).ok()?;
    if meta.len() > HISTORY_HARD_CAP {
        // Compact in place (keep newest), then re-read under the bound.
        if let Ok(raw) = fs::read_to_string(p) {
            let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
            let keep: Vec<&str> = lines[lines.len().saturating_sub(HISTORY_KEEP_LINES)..].to_vec();
            let _ = write_history_file(p, &keep.join("\n"));
        } else {
            return None;
        }
    }
    fs::read_to_string(p)
        .ok()
        .filter(|s| s.len() as u64 <= HISTORY_HARD_CAP)
}

/// Serializes append / rewrite so concurrent cleanup cannot drop or duplicate rows.
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Content hash of a raw jsonl line (shared by identical rows).
fn line_hash(line: &str) -> String {
    format!("H{:016x}", crate::fsutil::fnv1a64(line))
}

/// Stable row id: content hash + occurrence index among identical lines.
/// Distinct rows never collide, so delete-by-id removes exactly one of them.
fn line_content_id(line: &str, nth: usize) -> String {
    format!("{}-{nth}", line_hash(line))
}

/// Assign `H{hash}-{nth}` ids in file order (nth counts identical non-empty lines).
fn with_ids<'a>(lines: impl Iterator<Item = &'a str>) -> impl Iterator<Item = (&'a str, String)> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    lines.filter(|l| !l.trim().is_empty()).map(move |line| {
        let h = line_hash(line);
        let nth = seen.entry(h.clone()).or_insert(0);
        let id = line_content_id(line, *nth);
        *nth += 1;
        (line, id)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn append(
    app_name: &str,
    deleted: u32,
    failed: u32,
    skipped: u32,
    delayed: u32,
    aborted: bool,
    dry_run: bool,
    backup_dir: &str,
) -> bool {
    if dry_run {
        return true;
    }
    let entry = HistoryEntry {
        id: String::new(),
        app_name: app_name.into(),
        deleted,
        failed,
        skipped,
        delayed,
        aborted,
        dry_run,
        backup_dir: backup_dir.into(),
        created_at: unix_now_secs(),
    };
    let line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let p = history_path();
    let _g = FILE_LOCK.lock().ok();
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Soft cap: compact when the log grows past 2 MiB (keep newest 2000 rows).
    if let Ok(meta) = fs::metadata(&p) {
        if meta.len() > HISTORY_SOFT_CAP {
            if let Some(raw) = read_history_bounded(&p) {
                let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
                let keep: Vec<&str> =
                    lines[lines.len().saturating_sub(HISTORY_KEEP_LINES)..].to_vec();
                let _ = write_history_file(&p, &keep.join("\n"));
            }
        }
    }
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{line}");
        return true;
    }
    false
}

pub fn load(limit: usize) -> Vec<HistoryEntry> {
    let _g = FILE_LOCK.lock().ok();
    let p = history_path();
    let Some(raw) = read_history_bounded(&p) else {
        return vec![];
    };
    let mut out: Vec<HistoryEntry> = vec![];
    for (line, id) in with_ids(raw.lines()) {
        if let Ok(mut e) = serde_json::from_str::<HistoryEntry>(line) {
            e.id = id;
            out.push(e);
        }
    }
    out.reverse();
    out.truncate(limit);
    out
}

/// Drop lines whose runtime id is in `drop_ids`. Ids are `H{hash}-{nth}`, so
/// identical rows are addressed one-by-one and only the selected occurrences go.
fn filter_raw_lines(raw: &str, drop_ids: &HashSet<String>) -> (String, usize) {
    let mut out = String::new();
    let mut removed = 0usize;
    for (line, id) in with_ids(raw.lines()) {
        if drop_ids.contains(&id) {
            removed += 1;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, removed)
}

fn write_history_file(p: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("jsonl.tmp");
    fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}

/// Delete rows by runtime ids returned from [`load`] (`H{hash}-{nth}`). Returns removed count.
pub fn delete_by_ids(ids: &[String]) -> Result<usize, String> {
    let mut drop_ids: HashSet<String> = HashSet::new();
    for id in ids {
        if id.is_empty() || id.len() > 40 {
            return Err(format!("invalid history id: {id}"));
        }
        drop_ids.insert(id.clone());
    }
    if drop_ids.is_empty() {
        return Ok(0);
    }
    let _g = FILE_LOCK.lock().ok();
    let p = history_path();
    let Some(raw) = read_history_bounded(&p) else {
        return Ok(0);
    };
    let (next, removed) = filter_raw_lines(&raw, &drop_ids);
    if removed == 0 {
        return Ok(0);
    }
    write_history_file(&p, &next)?;
    Ok(removed)
}

/// Clear all cleanup history rows (does not touch backup sessions).
pub fn clear_all() -> Result<(), String> {
    let _g = FILE_LOCK.lock().ok();
    let p = history_path();
    write_history_file(&p, "")
}

fn unix_now_secs() -> String {
    // Epoch seconds as string (kept dependency-free).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    // history writes to PROGRAMDATA — skip disk tests; pure helpers only

    #[test]
    fn entry_serde_skips_id() {
        let e = super::HistoryEntry {
            id: "L0".into(),
            app_name: "A".into(),
            deleted: 1,
            failed: 0,
            skipped: 0,
            delayed: 0,
            aborted: false,
            dry_run: false,
            backup_dir: String::new(),
            created_at: "1".into(),
        };
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("\"id\":\"L0\""));
        let empty = super::HistoryEntry {
            id: String::new(),
            ..e
        };
        let s2 = serde_json::to_string(&empty).unwrap();
        assert!(!s2.contains("\"id\""));
        let back: super::HistoryEntry = serde_json::from_str(&s2).unwrap();
        assert_eq!(back.app_name, "A");
        assert_eq!(back.id, "");
    }

    #[test]
    fn line_content_id_stable() {
        let a = super::line_content_id("{\"app_name\":\"a\"}", 0);
        let b = super::line_content_id("{\"app_name\":\"a\"}", 0);
        let a1 = super::line_content_id("{\"app_name\":\"a\"}", 1);
        let c = super::line_content_id("{\"app_name\":\"b\"}", 0);
        assert_eq!(a, b);
        assert_ne!(a, a1, "identical rows must get distinct ids");
        assert_ne!(a, c);
        assert!(a.starts_with('H'));
        assert!(a.ends_with("-0"));
        assert!(a1.ends_with("-1"));
    }

    #[test]
    fn filter_raw_lines_drops_selected() {
        let raw = "{\"app_name\":\"a\"}\n{\"app_name\":\"b\"}\n{\"app_name\":\"c\"}\n";
        let mut drop: std::collections::HashSet<String> = Default::default();
        drop.insert(super::line_content_id("{\"app_name\":\"b\"}", 0));
        let (next, removed) = super::filter_raw_lines(raw, &drop);
        assert_eq!(removed, 1);
        assert_eq!(next, "{\"app_name\":\"a\"}\n{\"app_name\":\"c\"}\n");
    }

    #[test]
    fn filter_raw_lines_identical_rows_delete_one_by_one() {
        let raw = "{\"app_name\":\"a\"}\n{\"app_name\":\"a\"}\n{\"app_name\":\"a\"}\n";
        // Delete only the middle occurrence.
        let mut drop: std::collections::HashSet<String> = Default::default();
        drop.insert(super::line_content_id("{\"app_name\":\"a\"}", 1));
        let (next, removed) = super::filter_raw_lines(raw, &drop);
        assert_eq!(removed, 1);
        assert_eq!(next, "{\"app_name\":\"a\"}\n{\"app_name\":\"a\"}\n");

        // Delete two named occurrences together.
        let mut drop: std::collections::HashSet<String> = Default::default();
        drop.insert(super::line_content_id("{\"app_name\":\"a\"}", 0));
        drop.insert(super::line_content_id("{\"app_name\":\"a\"}", 2));
        let (next, removed) = super::filter_raw_lines(raw, &drop);
        assert_eq!(removed, 2);
        assert_eq!(next, "{\"app_name\":\"a\"}\n");
    }

    #[test]
    fn filter_raw_lines_ignores_unknown_ids() {
        let raw = "{\"app_name\":\"a\"}\n";
        let mut drop: std::collections::HashSet<String> = Default::default();
        drop.insert("Hffffffffffffffff-0".into());
        let (next, removed) = super::filter_raw_lines(raw, &drop);
        assert_eq!(removed, 0);
        assert_eq!(next, "{\"app_name\":\"a\"}\n");
    }

    /// Disk round-trip under a temp PROGRAMDATA (does not touch the real history file).
    #[test]
    fn disk_append_load_delete_clear_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("remova_hist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Remova")).unwrap();
        // Point history_path at the temp tree via PROGRAMDATA for this test only.
        // SAFETY: tests run multi-threaded; serialize with a process-local lock and restore env.
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = ENV_LOCK.lock().unwrap();
        let prev = std::env::var_os("PROGRAMDATA");
        std::env::set_var("PROGRAMDATA", &tmp);
        let p = super::history_path();
        assert!(p.starts_with(&tmp));

        assert!(super::append("A", 1, 0, 0, 0, false, false, r"C:\b"));
        assert!(super::append("A", 1, 0, 0, 0, false, false, r"C:\b"));
        assert!(super::append("B", 2, 0, 0, 0, false, false, r"C:\b"));
        let rows = super::load(10);
        assert_eq!(rows.len(), 3);
        assert_ne!(rows[0].id, rows[1].id, "identical rows need distinct ids");
        assert_eq!(rows[0].app_name, "B"); // newest first

        let removed = super::delete_by_ids(&[rows[2].id.clone()]).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(super::load(10).len(), 2);

        super::clear_all().unwrap();
        assert!(super::load(10).is_empty());

        match prev {
            Some(v) => std::env::set_var("PROGRAMDATA", v),
            None => std::env::remove_var("PROGRAMDATA"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
