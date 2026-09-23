//! Local cleanup history jsonl.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Runtime row id (`L{line_no}`). Empty on disk; filled by [`load`] for the UI/API.
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

fn history_path() -> PathBuf {
    let pd = std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Remova").join("history.jsonl")
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
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{line}");
        return true;
    }
    false
}

pub fn load(limit: usize) -> Vec<HistoryEntry> {
    let p = history_path();
    let Ok(raw) = fs::read_to_string(&p) else {
        return vec![];
    };
    let mut out: Vec<HistoryEntry> = vec![];
    for (i, line) in raw.lines().enumerate() {
        if let Ok(mut e) = serde_json::from_str::<HistoryEntry>(line) {
            e.id = line_id(i);
            out.push(e);
        }
    }
    out.reverse();
    out.truncate(limit);
    out
}

/// Stable row id for a 0-based line index in `history.jsonl`.
fn line_id(line_no: usize) -> String {
    format!("L{line_no}")
}

fn parse_line_id(id: &str) -> Option<usize> {
    id.strip_prefix('L')?.parse::<usize>().ok()
}

/// Drop lines whose 0-based index is in `drop_nos`; keep the rest.
fn filter_raw_lines(raw: &str, drop_nos: &HashSet<usize>) -> (String, usize) {
    let mut out = String::new();
    let mut removed = 0usize;
    for (i, line) in raw.lines().enumerate() {
        if drop_nos.contains(&i) {
            removed += 1;
            continue;
        }
        if line.trim().is_empty() {
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

/// Delete rows by runtime ids returned from [`load`]. Returns removed count.
pub fn delete_by_ids(ids: &[String]) -> Result<usize, String> {
    let mut drop_nos: HashSet<usize> = HashSet::new();
    for id in ids {
        match parse_line_id(id) {
            Some(n) => {
                drop_nos.insert(n);
            }
            None => return Err(format!("invalid history id: {id}")),
        }
    }
    if drop_nos.is_empty() {
        return Ok(0);
    }
    let p = history_path();
    let Ok(raw) = fs::read_to_string(&p) else {
        return Ok(0);
    };
    let (next, removed) = filter_raw_lines(&raw, &drop_nos);
    if removed == 0 {
        return Ok(0);
    }
    write_history_file(&p, &next)?;
    Ok(removed)
}

/// Clear all cleanup history rows (does not touch backup sessions).
pub fn clear_all() -> Result<(), String> {
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
    fn line_id_roundtrip() {
        assert_eq!(super::line_id(0), "L0");
        assert_eq!(super::parse_line_id("L12"), Some(12));
        assert_eq!(super::parse_line_id("x"), None);
        assert_eq!(super::parse_line_id("L"), None);
        assert_eq!(super::parse_line_id("../etc"), None);
    }

    #[test]
    fn filter_raw_lines_drops_selected() {
        let raw = "{\"app_name\":\"a\"}\n{\"app_name\":\"b\"}\n{\"app_name\":\"c\"}\n";
        let drop: std::collections::HashSet<usize> = [1].into_iter().collect();
        let (next, removed) = super::filter_raw_lines(raw, &drop);
        assert_eq!(removed, 1);
        assert_eq!(next, "{\"app_name\":\"a\"}\n{\"app_name\":\"c\"}\n");
    }

    #[test]
    fn filter_raw_lines_ignores_unknown_ids() {
        let raw = "{\"app_name\":\"a\"}\n";
        let drop: std::collections::HashSet<usize> = [9].into_iter().collect();
        let (next, removed) = super::filter_raw_lines(raw, &drop);
        assert_eq!(removed, 0);
        assert_eq!(next, "{\"app_name\":\"a\"}\n");
    }
}
