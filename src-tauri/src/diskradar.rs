//! Disk usage radar — read-only top-level directory sizes for "what ate the drive".
//! Never deletes; UI only opens paths / links to uninstall.

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

fn system_drive_roots() -> Vec<PathBuf> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let drive = drive.trim_end_matches('\\').to_uppercase();
    let root = PathBuf::from(format!("{drive}\\"));
    vec![
        root.join("Users"),
        root.join("Program Files"),
        root.join("Program Files (x86)"),
        root.join("ProgramData"),
        root.join("Windows"),
    ]
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
    rows.sort_by(|a, b| b.size_kb.cmp(&a.size_kb));
    rows.truncate(DISK_RADAR_TOP_N);
    rows
}

/// Top directories under each well-known system root (one level).
pub fn top_dir_sizes() -> Vec<DirSizeRow> {
    let mut out = Vec::new();
    for root in system_drive_roots() {
        if !root.exists() {
            continue;
        }
        let mut rows = child_rows(&root);
        out.append(&mut rows);
    }
    out.sort_by(|a, b| b.size_kb.cmp(&a.size_kb));
    out.truncate(DISK_RADAR_TOP_N * 2);
    out
}

/// True when `path` is under one of the radar's well-known system roots.
fn under_radar_roots(path: &str) -> bool {
    let raw = path.replace('/', "\\");
    // Traversal segments must never pass a prefix check (even if the path is missing
    // and canonicalize cannot resolve it).
    if raw.split('\\').any(|seg| seg == ".." || seg == ".") {
        return false;
    }
    // Canonicalize first so verbatim / 8.3 / substituted shapes resolve to the real root.
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
    // Non-UTF-8 after canonicalize → refuse (no lossy prefix match).
    let Some(canon_s) = crate::fsutil::path_utf8(&canon) else {
        return false;
    };
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let drive = drive.trim_end_matches('\\').to_uppercase();
    let norm = canon_s.trim_end_matches('\\').to_uppercase();
    // Strip Windows verbatim prefix (`\\?\`).
    let norm = norm
        .strip_prefix(r"\\?\")
        .map(|s| s.to_string())
        .unwrap_or(norm);
    for leaf in [
        "USERS",
        "PROGRAM FILES",
        "PROGRAM FILES (X86)",
        "PROGRAMDATA",
        "WINDOWS",
    ] {
        let root = format!("{drive}\\{leaf}");
        if norm == root || norm.starts_with(&format!("{root}\\")) {
            return true;
        }
    }
    false
}

/// Immediate children of `path` ranked by size (for drill-down).
pub fn list_dir_children(path: &str) -> Result<Vec<DirSizeRow>, String> {
    let p = PathBuf::from(path.trim());
    if path.trim().is_empty() || !p.is_dir() {
        return Err("not a directory".into());
    }
    // Only allow drill-down under well-known system roots (read-only safety).
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
        assert!(!super::under_radar_roots(r"C:\Games\Steam"));
        assert!(!super::under_radar_roots(r"D:\Data"));
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
}
