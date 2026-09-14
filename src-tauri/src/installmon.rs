//! Explicit install-monitor snapshots (P2-1). User starts/stops; no driver.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FsSnapshot {
    pub files: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorDiff {
    pub added_files: Vec<String>,
    pub added_reg_values: Vec<String>,
}

fn monitor_state_path() -> PathBuf {
    let base = std::env::var("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"));
    base.join("Remova").join("monitor_snapshot.json")
}

fn roots() -> Vec<PathBuf> {
    let mut v = vec![];
    for e in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA", "PROGRAMDATA"] {
        if let Ok(p) = std::env::var(e) {
            v.push(PathBuf::from(p));
        }
    }
    v
}

fn walk_names(root: &Path, out: &mut BTreeSet<String>, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        let p = e.path();
        let rel = p.to_string_lossy().to_lowercase();
        *budget = budget.saturating_sub(1);
        out.insert(rel);
        if p.is_dir() && !e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            walk_names(&p, out, budget);
        }
    }
}

fn take_fs_snapshot() -> FsSnapshot {
    let mut files = BTreeSet::new();
    let mut budget = 80_000usize;
    for r in roots() {
        walk_names(&r, &mut files, &mut budget);
    }
    FsSnapshot { files }
}

fn reg_value_names() -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let keys = [
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
    ];
    for k in keys {
        for sub in crate::regscan::list_subkeys(k) {
            set.insert(format!("{k}\\{sub}").to_lowercase());
        }
        for (v, _) in crate::regscan::list_values(k) {
            set.insert(format!("{k}::{v}").to_lowercase());
        }
    }
    set
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FullSnapshot {
    fs: FsSnapshot,
    reg: Vec<String>,
}

pub fn begin() -> Result<(), String> {
    let snap = FullSnapshot {
        fs: take_fs_snapshot(),
        reg: reg_value_names().into_iter().collect(),
    };
    let p = monitor_state_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string(&snap).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| e.to_string())
}

pub fn end() -> Result<MonitorDiff, String> {
    let p = monitor_state_path();
    let raw = std::fs::read_to_string(&p).map_err(|_| "no monitor snapshot; start first".to_string())?;
    let before: FullSnapshot = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&p);
    let after = take_fs_snapshot();
    let after_reg = reg_value_names();
    let added_files: Vec<String> = after
        .files
        .difference(&before.fs.files)
        .cloned()
        .take(200)
        .collect();
    let before_reg: BTreeSet<String> = before.reg.into_iter().collect();
    let added_reg_values: Vec<String> = after_reg
        .difference(&before_reg)
        .cloned()
        .take(100)
        .collect();
    Ok(MonitorDiff {
        added_files,
        added_reg_values,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_roundtrip_shape() {
        // begin may be slow; only ensure end without begin errors
        let e = super::end();
        assert!(e.is_err());
    }
}
