//! Local cleanup history jsonl.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufWriter, Write};
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
const HISTORY_KEEP_LINES: usize = 2000;

fn history_path() -> PathBuf {
    let pd = std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Remova").join("history.jsonl")
}

/// Serializes append / rewrite so concurrent cleanup cannot drop or duplicate rows.
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// a poisoned mutex still holds `()` — recover instead of running
/// subsequent reads/writes with no lock at all.
fn lock_file() -> std::sync::MutexGuard<'static, ()> {
    FILE_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// Content hash of a raw jsonl line (shared by identical rows).
fn line_hash(line: &str) -> String {
    format!("H{:016x}", crate::fsutil::fnv1a64(line))
}

/// Stable row id in file order: content hash + occurrence index among identical
/// lines (computed streaming — no whole-file parse).
fn next_line_id(seen: &mut HashMap<String, usize>, line: &str) -> String {
    let h = line_hash(line);
    let nth = seen.entry(h.clone()).or_insert(0);
    let id = format!("{h}-{nth}");
    *nth += 1;
    id
}

/// Rewrite `p` keeping only the newest `keep` non-empty lines. Streams the file
/// one line at a time through a ring buffer — memory is O(keep), never O(file)
/// (; also removes the old unbounded whole-file read on oversize).
/// Caller must hold [`FILE_LOCK`].
fn compact_keep_newest(p: &Path, keep: usize) -> std::io::Result<()> {
    let file = fs::File::open(p)?;
    let reader = std::io::BufReader::new(file);
    let mut ring: VecDeque<String> = VecDeque::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if ring.len() == keep {
            ring.pop_front();
        }
        ring.push_back(line);
    }
    let (tmp, out) = crate::fsutil::create_atomic_tmp(p)?;
    let result = (|| -> std::io::Result<()> {
        let mut w = BufWriter::new(out);
        for l in &ring {
            writeln!(w, "{l}")?;
        }
        w.flush()?;
        w.into_inner()?.sync_all()?;
        fs::rename(&tmp, p)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
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
    let _g = lock_file();
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Soft cap: compact when the log grows past 2 MiB (keep newest 2000 rows).
    if let Ok(meta) = fs::metadata(&p) {
        if meta.len() > HISTORY_SOFT_CAP {
            // streaming compact — no whole-file read.
            let _ = compact_keep_newest(&p, HISTORY_KEEP_LINES);
        }
    }
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&p) {
        // A failed write is a lost history row — report it, never claim success.
        return writeln!(f, "{line}").is_ok();
    }
    false
}

pub fn load(limit: usize) -> Vec<HistoryEntry> {
    if limit == 0 {
        return vec![];
    }
    let _g = lock_file();
    let p = history_path();
    let Ok(file) = fs::File::open(&p) else {
        return vec![];
    };
    // stream lines, keep only the newest `limit` in a ring, parse
    // after the pass — memory is O(limit), not O(file); rows whose ids must be
    // counted across the whole file still get file-order occurrence ids.
    let reader = std::io::BufReader::new(file);
    let mut ring: VecDeque<(String, String)> = VecDeque::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        let id = next_line_id(&mut seen, &line);
        if ring.len() == limit {
            ring.pop_front();
        }
        ring.push_back((line, id));
    }
    let mut out: Vec<HistoryEntry> = Vec::with_capacity(ring.len());
    for (line, id) in ring.into_iter().rev() {
        if let Ok(mut e) = serde_json::from_str::<HistoryEntry>(&line) {
            e.id = id;
            out.push(e);
        }
    }
    out
}

/// Delete rows by runtime ids returned from [`load`] (`H{hash}-{nth}`). Ids are
/// `H{hash}-{nth}`, so identical rows are addressed one-by-one and only the
/// selected occurrences go. Streams a full rewrite — one line in memory at a time.
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
    let _g = lock_file();
    let p = history_path();
    let Ok(file) = fs::File::open(&p) else {
        return Ok(0);
    };
    let (tmp, out) = crate::fsutil::create_atomic_tmp(&p).map_err(|e| e.to_string())?;
    let mut w = BufWriter::new(out);
    let mut reader = std::io::BufReader::new(file);
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut removed = 0usize;
    // stream raw lines — a non-UTF-8 row (torn append) must pass
    // through untouched, not vanish along with unrelated rows.
    let mut raw: Vec<u8> = Vec::new();
    loop {
        raw.clear();
        let n = match reader.read_until(b'\n', &mut raw) {
            Ok(n) => n,
            Err(error) => {
                drop(w);
                let _ = fs::remove_file(&tmp);
                return Err(error.to_string());
            }
        };
        if n == 0 {
            break;
        }
        while matches!(raw.last(), Some(b'\n') | Some(b'\r')) {
            raw.pop();
        }
        if raw.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        match std::str::from_utf8(&raw) {
            Ok(line) => {
                let id = next_line_id(&mut seen, line);
                if drop_ids.contains(&id) {
                    removed += 1;
                    continue;
                }
                if writeln!(w, "{line}").is_err() {
                    let _ = fs::remove_file(&tmp);
                    return Err("history rewrite failed".into());
                }
            }
            Err(_) => {
                // Opaque bytes — never selected via UI ids, so always keep.
                if w.write_all(&raw).is_err() || w.write_all(b"\n").is_err() {
                    let _ = fs::remove_file(&tmp);
                    return Err("history rewrite failed".into());
                }
            }
        }
    }
    if w.flush().and_then(|_| w.get_ref().sync_all()).is_err() {
        let _ = fs::remove_file(&tmp);
        return Err("history rewrite failed".into());
    }
    drop(w);
    if removed == 0 {
        let _ = fs::remove_file(&tmp);
        return Ok(0);
    }
    // rename failure must not leave the tmp file behind.
    fs::rename(&tmp, &p)
        .inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
        .map_err(|e| e.to_string())?;
    Ok(removed)
}

fn write_history_file(p: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::fsutil::write_bytes_atomic(p, contents.as_bytes()).map_err(|e| e.to_string())
}

/// Clear all cleanup history rows (does not touch backup sessions).
pub fn clear_all() -> Result<(), String> {
    let _g = lock_file();
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
    use std::collections::HashMap;

    /// Serializes tests that mutate process-wide PROGRAMDATA.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(windows)]
    #[test]
    fn history_write_preserves_preexisting_temporary_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir =
            std::env::temp_dir().join(format!("remova-r23-history-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.jsonl");
        let tmp = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
        std::fs::write(&tmp, "old partial").unwrap();
        // A pre-existing file is not owned by this write, even if it is locked.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x4)
            .open(&tmp)
            .unwrap();
        super::write_history_file(&path, "new data").unwrap();
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "old partial");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new data");
        drop(held);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Compaction cannot take ownership of an old staging file.
    #[cfg(windows)]
    #[test]
    fn compact_keep_newest_preserves_preexisting_temporary_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir =
            std::env::temp_dir().join(format!("remova-r23-history-compact-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.jsonl");
        std::fs::write(&path, "{\"id\":\"L1\"}\n{\"id\":\"L2\"}\n").unwrap();
        let tmp = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
        std::fs::write(&tmp, "old partial").unwrap();
        // Deny writes to the pre-existing file.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x4)
            .open(&tmp)
            .unwrap();
        super::compact_keep_newest(&path, 1).unwrap();
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "old partial");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"id\":\"L2\"}\n");
        drop(held);
        std::fs::remove_dir_all(dir).unwrap();
    }

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
    fn next_line_id_counts_occurrences_in_file_order() {
        let mut seen = HashMap::new();
        let a0 = super::next_line_id(&mut seen, "{\"app_name\":\"a\"}");
        let a1 = super::next_line_id(&mut seen, "{\"app_name\":\"a\"}");
        let a2 = super::next_line_id(&mut seen, "{\"app_name\":\"a\"}");
        let b0 = super::next_line_id(&mut seen, "{\"app_name\":\"b\"}");
        assert_ne!(a0, a1, "identical rows must get distinct ids");
        assert_ne!(a0, a2);
        assert_ne!(a0, b0, "distinct rows never collide on the same nth");
        assert!(a0.starts_with('H') && a0.ends_with("-0"));
        assert!(a1.ends_with("-1"));
        assert!(a2.ends_with("-2"));
        // A fresh map restarts the occurrence counter (delete pass == load pass).
        let mut fresh = HashMap::new();
        assert_eq!(super::next_line_id(&mut fresh, "{\"app_name\":\"a\"}"), a0);
    }

    /// streaming compact keeps only the newest N non-empty lines.
    #[test]
    fn compact_keep_newest_keeps_tail() {
        let tmp = std::env::temp_dir().join(format!("remova_hist_c_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let p = tmp.join("history.jsonl");
        std::fs::write(&p, "l1\nl2\nl3\nl4\n\nl5\n").unwrap();
        super::compact_keep_newest(&p, 3).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert_eq!(s, "l3\nl4\nl5\n");
        // Keeping more than present is a no-op.
        super::compact_keep_newest(&p, 10).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "l3\nl4\nl5\n");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Disk round-trip under a temp PROGRAMDATA (does not touch the real history file).
    #[test]
    fn disk_append_load_delete_clear_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("remova_hist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Remova")).unwrap();
        // Point history_path at the temp tree via PROGRAMDATA for this test only.
        // SAFETY: tests run multi-threaded; serialize with a process-local lock and restore env.
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

    /// a torn non-UTF-8 row must survive delete rewrites untouched.
    #[test]
    fn delete_preserves_non_utf8_lines() {
        let tmp = std::env::temp_dir().join(format!("remova_hist_nu_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Remova")).unwrap();
        let _g = ENV_LOCK.lock().unwrap();
        let prev = std::env::var_os("PROGRAMDATA");
        std::env::set_var("PROGRAMDATA", &tmp);
        let p = super::history_path();

        assert!(super::append("A", 1, 0, 0, 0, false, false, r"C:\b"));
        assert!(super::append("B", 2, 0, 0, 0, false, false, r"C:\b"));
        // Inject a torn non-UTF-8 line between the two good rows.
        let lines = std::fs::read(&p).unwrap();
        let split = lines.iter().position(|&b| b == b'\n').unwrap();
        let mut rebuilt = lines[..=split].to_vec();
        rebuilt.extend_from_slice(b"{\"app_name\":\"");
        rebuilt.extend_from_slice(&[0xFF, 0xFE, 0x80]);
        rebuilt.extend_from_slice(b"\"}\n");
        rebuilt.extend_from_slice(&lines[split + 1..]);
        std::fs::write(&p, &rebuilt).unwrap();

        // Load sees only the two UTF-8 rows (bad line is invisible to the UI).
        let rows = super::load(10);
        assert_eq!(rows.len(), 2, "load rows");
        // Delete the oldest good row (newest-first → index 1) — opaque line remains.
        let removed = super::delete_by_ids(&[rows[1].id.clone()]).unwrap();
        assert_eq!(removed, 1);
        let out = std::fs::read(&p).unwrap();
        assert!(
            out.windows(3).any(|w| w == [0xFF, 0xFE, 0x80]),
            "non-UTF-8 line must pass through delete rewrite"
        );
        assert!(
            out.windows(2).any(|w| w == b"B\n") || out.windows(3).any(|w| w == b"\"B\""),
            "unrelated UTF-8 row must remain"
        );
        assert!(
            !out.windows(3).any(|w| w == b"\"A\""),
            "selected row must be gone"
        );

        match prev {
            Some(v) => std::env::set_var("PROGRAMDATA", v),
            None => std::env::remove_var("PROGRAMDATA"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
