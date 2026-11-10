//! Junction ledger for the disk-space overview (bounded, read-only).
//!
//! Directory reparse points (junctions, directory symlinks) make naive size
//! walks double-count everything that was moved off `C:` — the ledger
//! enumerates them under a fixed set of roots without ever following one, and
//! records whether each target is still reachable so dead links can be
//! surfaced. Scanning is read-only; no target is ever touched.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Hard ceiling on ledger rows per run; beyond it the walk stops and the
/// result reports the truncation in `omitted`.
const RESULT_CAP: usize = 20_000;
/// Plain directories this many levels below a root are still enumerated.
const MAX_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JunctionEntry {
    /// The reparse-point directory itself (the link, never its target).
    pub path: String,
    /// `read_link` target when the reparse data resolves to a path.
    pub target: Option<String>,
    /// False only when the target provably does not exist. Unreadable targets
    /// (ACL, offline) and unresolvable tags stay true: the ledger never calls
    /// a link dead on doubt.
    pub reachable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JunctionScanResult {
    pub entries: Vec<JunctionEntry>,
    #[serde(default)]
    pub omitted: u32,
    /// Roots that could not be read at all — reported, never silently empty.
    #[serde(default)]
    pub degraded_roots: Vec<String>,
}

/// One child directory of a scanned directory, as classified by the IO layer.
#[derive(Debug, Clone)]
struct ChildDir {
    path: PathBuf,
    is_reparse: bool,
}

/// Pure walk step: record reparse children via `classify`, queue plain
/// directories while depth allows, and stop at `cap`. Structured so the cap
/// and the never-follow rule are testable without touching the disk.
/// Returns true when the cap was reached (results are truncated).
fn visit_step<F: Fn(&Path) -> JunctionEntry>(
    children: Vec<ChildDir>,
    depth: usize,
    cap: usize,
    classify: F,
    out: &mut Vec<JunctionEntry>,
    queue: &mut Vec<(PathBuf, usize)>,
) -> bool {
    for child in children {
        if out.len() >= cap {
            return true;
        }
        if child.is_reparse {
            out.push(classify(&child.path));
        } else if depth < MAX_DEPTH {
            queue.push((child.path, depth + 1));
        }
    }
    false
}

/// IO classification of one reparse-point directory.
fn classify(path: &Path) -> JunctionEntry {
    let parent_pins = path
        .parent()
        .and_then(|p| crate::fsutil::pin_local_scan_path(p).ok());
    if parent_pins.is_none() {
        return JunctionEntry {
            path: path.to_string_lossy().into_owned(),
            target: None,
            reachable: true,
        };
    }
    let Some(target) = crate::fsutil::read_local_scan_link(path)
        .ok()
        .map(|t| t.to_string_lossy().into_owned())
    else {
        return JunctionEntry {
            path: path.to_string_lossy().into_owned(),
            target: None,
            reachable: true,
        };
    };
    // Compatibility contract: true means present OR unknown/not probed. Only
    // conclusive local absence may set false; remote/relative/reparse targets
    // must not be contacted and must never inflate the UI dead-link count.
    let reachable = target_reachable(Path::new(&target));
    JunctionEntry {
        path: path.to_string_lossy().into_owned(),
        target: Some(target),
        reachable,
    }
}

fn target_reachable(target: &Path) -> bool {
    let Some(parent) = target.parent() else {
        return true;
    };
    let Ok(_parents) = crate::fsutil::pin_local_scan_path(parent) else {
        return true;
    };
    // Inspect final entry without following a target that is itself a link.
    !matches!(crate::fsutil::local_scan_exists(target), Ok(false))
}

/// Bounded walk of one root. Plain directories are descended up to
/// `MAX_DEPTH`; reparse directories are recorded and never entered.
fn scan_root<F: Fn(&Path) -> JunctionEntry>(
    root: &Path,
    cap: usize,
    classify: F,
) -> (Vec<JunctionEntry>, bool, bool) {
    let mut out: Vec<JunctionEntry> = Vec::new();
    let mut queue: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut truncated = false;
    let mut degraded = false;
    while let Some((dir, depth)) = queue.pop() {
        let Ok(_pins) = crate::fsutil::pin_local_scan_path(&dir) else {
            degraded = true;
            continue;
        };
        let Ok(read) = crate::fsutil::read_local_scan_dir(&dir) else {
            degraded = true;
            continue;
        };
        let children: Vec<ChildDir> = read
            .filter_map(|entry| {
                let e = match entry {
                    Ok(e) => e,
                    Err(_) => {
                        degraded = true;
                        return None;
                    }
                };
                // DirEntry::metadata does not traverse the link. std's
                // file_type() reports junctions as is_dir() == false on
                // Windows, so directory-ness comes from the attribute bit.
                let md = match e.metadata() {
                    Ok(md) => md,
                    Err(_) => {
                        degraded = true;
                        return None;
                    }
                };
                #[cfg(windows)]
                let dir_like = {
                    use std::os::windows::fs::MetadataExt;
                    // FILE_ATTRIBUTE_DIRECTORY = 0x10.
                    md.file_attributes() & 0x10 != 0
                };
                #[cfg(not(windows))]
                let dir_like = md.file_type().is_dir();
                if !dir_like {
                    return None;
                }
                Some(ChildDir {
                    path: e.path(),
                    is_reparse: crate::fsutil::is_reparse_metadata(&md),
                })
            })
            .collect();
        if visit_step(children, depth, cap, &classify, &mut out, &mut queue) {
            truncated = true;
            break;
        }
    }
    (out, truncated, degraded)
}

/// Known roots, in scan order. `USERPROFILE` nests the two AppData roots, so
/// the ledger dedups paths found through more than one root.
fn scan_roots() -> Vec<PathBuf> {
    [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
    ]
    .iter()
    .filter_map(std::env::var_os)
    .map(PathBuf::from)
    .collect()
}

/// Deterministic order, case-insensitive dedup across nested roots, and the
/// global cap. `omitted_roots` counts roots whose walk hit the cap, so it is
/// a "results incomplete" marker, not an exact remainder.
fn finalize(
    mut entries: Vec<JunctionEntry>,
    omitted_roots: u32,
    degraded: Vec<String>,
) -> JunctionScanResult {
    entries.sort_by_key(|e| e.path.to_lowercase());
    entries.dedup_by(|a, b| a.path.eq_ignore_ascii_case(&b.path));
    let omitted = omitted_roots + entries.len().saturating_sub(RESULT_CAP) as u32;
    entries.truncate(RESULT_CAP);
    JunctionScanResult {
        entries,
        omitted,
        degraded_roots: degraded,
    }
}

/// Bounded, read-only enumeration of directory reparse points under the
/// known roots. Unreadable roots land in `degraded_roots`.
pub fn scan_junction_ledger() -> JunctionScanResult {
    scan_ledger_with(scan_roots())
}

fn scan_ledger_with(roots: Vec<PathBuf>) -> JunctionScanResult {
    let mut entries = Vec::new();
    let mut degraded = Vec::new();
    let mut omitted_roots: u32 = 0;
    for root in roots {
        let Ok(_pins) = crate::fsutil::pin_local_scan_path(&root) else {
            degraded.push(root.to_string_lossy().into_owned());
            continue;
        };
        if crate::fsutil::read_local_scan_dir(&root).is_err() {
            degraded.push(root.to_string_lossy().into_owned());
            continue;
        }
        let remaining = RESULT_CAP.saturating_sub(entries.len());
        if remaining == 0 {
            omitted_roots += 1;
            continue;
        }
        let (found, truncated, incomplete) = scan_root(&root, remaining, classify);
        if incomplete {
            degraded.push(root.to_string_lossy().into_owned());
        }
        entries.extend(found);
        if truncated {
            omitted_roots += 1;
        }
    }
    finalize(entries, omitted_roots, degraded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(path: &str, is_reparse: bool) -> ChildDir {
        ChildDir {
            path: PathBuf::from(path),
            is_reparse,
        }
    }

    fn fake(path: &Path) -> JunctionEntry {
        JunctionEntry {
            path: path.to_string_lossy().into_owned(),
            target: Some("Z:\\target".into()),
            reachable: true,
        }
    }

    #[cfg(windows)]
    #[test]
    fn local_scan_unknown_targets_never_count_as_dead() {
        for raw in [
            r"\\invalid.example\share\x",
            r"relative\target",
            r"C:relative",
            r"\\?\UNC\invalid.example\share\x",
        ] {
            assert!(
                target_reachable(Path::new(raw)),
                "unknown must stay reachable-compatible: {raw}"
            );
        }
        let tmp = crate::test_support::TempDir::new("junction-unknown");
        let target = tmp.subdir("target");
        let link = tmp.path().join("link");
        make_junction(&link, &target);
        assert!(target_reachable(&link));
        assert!(!target_reachable(&tmp.path().join("missing")));
    }

    #[test]
    fn reparse_children_are_recorded_but_never_queued() {
        let mut out = Vec::new();
        let mut queue = Vec::new();
        let full = visit_step(
            vec![child(r"C:\R\link", true), child(r"C:\R\plain", false)],
            1,
            100,
            fake,
            &mut out,
            &mut queue,
        );
        assert!(!full);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, r"C:\R\link");
        // Only the plain directory is queued for descent.
        assert_eq!(queue, vec![(PathBuf::from(r"C:\R\plain"), 2)]);
    }

    #[test]
    fn depth_four_directories_are_enumerated_but_not_descended() {
        let mut out = Vec::new();
        let mut queue = Vec::new();
        visit_step(
            vec![
                child(r"C:\R\a\b\link", true),
                child(r"C:\R\a\b\plain", false),
            ],
            MAX_DEPTH,
            100,
            fake,
            &mut out,
            &mut queue,
        );
        // A reparse at the deepest enumerated level is still recorded…
        assert_eq!(out.len(), 1);
        // …but plain children of it are no longer queued.
        assert!(queue.is_empty());
    }

    #[test]
    fn the_walk_stops_cleanly_at_the_cap() {
        let mut out = Vec::new();
        let mut queue = Vec::new();
        let full = visit_step(
            vec![
                child(r"C:\R\link1", true),
                child(r"C:\R\link2", true),
                child(r"C:\R\link3", true),
            ],
            0,
            2,
            fake,
            &mut out,
            &mut queue,
        );
        assert!(full, "the cap must be reported to the caller");
        assert_eq!(out.len(), 2);
        assert!(queue.is_empty(), "nothing is queued once the cap hit");
    }

    #[test]
    fn finalize_dedups_case_insensitive_duplicates_and_sorts() {
        let entry = |p: &str| JunctionEntry {
            path: p.into(),
            target: None,
            reachable: true,
        };
        let result = finalize(
            vec![entry(r"C:\B\z"), entry(r"c:\a\x"), entry(r"C:\B\Z")],
            0,
            vec![],
        );
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.entries[0].path, r"c:\a\x");
        assert_eq!(result.entries[1].path, r"C:\B\z");
        assert_eq!(result.omitted, 0);
    }

    #[test]
    fn finalize_reports_truncated_roots_and_overflow() {
        let entry = |p: &str| JunctionEntry {
            path: p.into(),
            target: None,
            reachable: true,
        };
        let many: Vec<JunctionEntry> = (0..RESULT_CAP + 3)
            .map(|i| entry(&format!(r"C:\R\{i:05}")))
            .collect();
        let result = finalize(many, 2, vec![r"C:\Gone".into()]);
        assert_eq!(result.entries.len(), RESULT_CAP);
        assert_eq!(result.omitted, 5);
        assert_eq!(result.degraded_roots, vec![r"C:\Gone"]);
    }

    #[cfg(windows)]
    fn make_junction(link: &Path, target: &Path) {
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .unwrap();
        assert!(status.success(), "junction fixture creation failed");
    }

    #[cfg(windows)]
    #[test]
    fn unreadable_link_child_keeps_readable_links_and_reports_degraded_root() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = crate::test_support::TempDir::new("junction-unreadable-child");
        let root = tmp.subdir("root");
        let target = tmp.subdir("target");
        let good = root.join("good");
        let blocked = root.join("blocked");
        make_junction(&good, &target);
        make_junction(&blocked, &target);
        let lock = std::fs::OpenOptions::new()
            .access_mode(2)
            .share_mode(7)
            .custom_flags(0x0220_0000)
            .open(&blocked)
            .unwrap();
        let report = scan_ledger_with(vec![root.clone()]);
        assert!(report.entries.iter().any(|e| Path::new(&e.path) == good));
        assert!(!report.entries.iter().any(|e| Path::new(&e.path) == blocked));
        assert_eq!(report.degraded_roots, [root.to_string_lossy().into_owned()]);
        drop(lock);
        let complete = scan_ledger_with(vec![root]);
        assert!(complete.degraded_roots.is_empty());
        assert!(complete
            .entries
            .iter()
            .any(|e| Path::new(&e.path) == blocked));
    }

    #[cfg(windows)]
    #[test]
    fn scan_root_classifies_live_and_dead_junctions_without_following() {
        let tmp = crate::test_support::TempDir::new("junction-ledger");
        let root = tmp.path().join("root");
        // A junction two plain levels below the root.
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let live_target = tmp.path().join("live-target");
        std::fs::create_dir_all(&live_target).unwrap();
        make_junction(&nested.join("live"), &live_target);
        // A junction whose target is then removed → provably dead.
        let doomed_target = tmp.path().join("doomed-target");
        std::fs::create_dir_all(&doomed_target).unwrap();
        make_junction(&root.join("dead"), &doomed_target);
        std::fs::remove_dir_all(&doomed_target).unwrap();
        // A junction whose *target tree* contains another junction: the walk
        // must never see the inner one through the link path.
        let outer_target = tmp.path().join("outer-target");
        let inner_link = outer_target.join("inner");
        let inner_target = tmp.path().join("inner-target");
        std::fs::create_dir_all(&inner_target).unwrap();
        std::fs::create_dir_all(&outer_target).unwrap();
        make_junction(&inner_link, &inner_target);
        make_junction(&root.join("outer"), &outer_target);

        let (entries, truncated, degraded) = scan_root(&root, RESULT_CAP, classify);
        assert!(!truncated);
        assert!(!degraded);
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths.len(), 3, "three junctions: {paths:?}");
        assert!(
            paths.iter().all(|p| !p.contains("outer.inner")),
            "the walk must not follow a junction into its target: {paths:?}"
        );
        let live = entries.iter().find(|e| e.path.ends_with("live")).unwrap();
        assert!(live.reachable);
        assert!(live.target.as_deref().unwrap().ends_with("live-target"));
        let dead = entries.iter().find(|e| e.path.ends_with("dead")).unwrap();
        assert!(!dead.reachable, "a removed target is provably dead");
    }
}
