//! Local cleanup history jsonl.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub app_name: String,
    pub deleted: u32,
    pub failed: u32,
    pub skipped: u32,
    pub aborted: bool,
    pub dry_run: bool,
    pub backup_dir: String,
    pub created_at: String,
}

fn history_path() -> PathBuf {
    let pd = std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Remova").join("history.jsonl")
}

pub fn append(
    app_name: &str,
    deleted: u32,
    failed: u32,
    skipped: u32,
    aborted: bool,
    dry_run: bool,
    backup_dir: &str,
) -> bool {
    if dry_run {
        return true;
    }
    let entry = HistoryEntry {
        app_name: app_name.into(),
        deleted,
        failed,
        skipped,
        aborted,
        dry_run,
        backup_dir: backup_dir.into(),
        created_at: chrono_like_now(),
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
    for line in raw.lines().rev().take(limit) {
        if let Ok(e) = serde_json::from_str::<HistoryEntry>(line) {
            out.push(e);
        }
    }
    out
}

fn chrono_like_now() -> String {
    // ISO-ish without chrono dep
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    // history writes to PROGRAMDATA — skip disk test; pure types only
    #[test]
    fn entry_serde() {
        let e = super::HistoryEntry {
            app_name: "A".into(),
            deleted: 1,
            failed: 0,
            skipped: 0,
            aborted: false,
            dry_run: false,
            backup_dir: String::new(),
            created_at: "1".into(),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: super::HistoryEntry = serde_json::from_str(&s).unwrap();
        assert_eq!(back.app_name, "A");
    }
}
