//! Regenerable-layout cleanup (M6.2) — the one class allowed to delete
//! without a Safety Vault backup (safety invariant 7).
//!
//! Admission is strict: a path is deletable only when it byte-matches one of
//! the fixed regenerable layouts (never a keyword, never a child the scan did
//! not list). The layout directory itself is kept — only its contents go — so
//! apps that expect the directory to exist keep working. Locked files are
//! skipped and reported, never counted as reclaimed. `CbsTemp` additionally
//! waits for servicing to be idle.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const RESULT_CAP: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegenerableItem {
    pub path: String,
    /// Stable reason code shared with the triage view.
    pub reason: String,
    #[serde(default)]
    pub size_kb: i64,
    #[serde(default)]
    pub size_capped: bool,
    /// False when a live gate (e.g. servicing busy) blocks this layout now.
    #[serde(default)]
    pub deletable: bool,
    /// Why `deletable` is false, as a stable code the UI localises.
    #[serde(default)]
    pub block_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegenerableScanResult {
    pub items: Vec<RegenerableItem>,
    #[serde(default)]
    pub omitted: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegenerableSkipped {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegenerableCleanupReport {
    /// Layout roots whose contents were removed (the directory itself is kept).
    pub cleaned: Vec<String>,
    pub skipped: Vec<RegenerableSkipped>,
    /// before − after across cleaned roots; underestimates when files locked.
    pub reclaimed_kb: i64,
}

/// The exact regenerable layouts. Nothing outside this table is ever
/// deletable through this module.
pub fn fixed_layouts(roots: &crate::triage::TriageRoots) -> Vec<(PathBuf, &'static str)> {
    let win = &roots.windows;
    vec![
        (
            win.join(r"SoftwareDistribution\Download"),
            "win_update_download",
        ),
        (win.join("CbsTemp"), "cbs_temp"),
        (win.join("Temp"), "win_temp"),
        (win.join("Minidump"), "crash_dumps"),
        (
            roots.program_data.join(r"Microsoft\Windows\WER"),
            "wer_reports",
        ),
        (roots.local_app_data.join("Temp"), "user_temp"),
        (roots.local_app_data.join("CrashDumps"), "crash_dumps"),
        (roots.local_app_data.join("D3DSCache"), "shader_cache"),
    ]
}

/// Servicing must be idle before CbsTemp is touched. TiWorker/DISM running
/// means a session is in flight; the CBS tail check stays out of v1 — the
/// process gate plus honest reporting is the bounded v1 contract.
fn servicing_busy() -> bool {
    for image in ["TiWorker.exe", "dism.exe"] {
        let out = std::process::Command::new(crate::regops::sys_tool("tasklist.exe"))
            .args(["/FI", &format!("IMAGENAME eq {image}"), "/FO", "CSV", "/NH"])
            .output();
        let Ok(out) = out else { return true };
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        // tasklist prints "INFO: no tasks are running..." when the filter misses.
        if !text.contains("no tasks are running") && text.contains(&image.to_lowercase()) {
            return true;
        }
    }
    false
}

/// Short-lived cache for the *busy* verdict only, used by the read-only
/// panel scan: a batch of display refreshes should not spawn tasklist each
/// time. Fail-closed by construction — caching a "busy" verdict can only
/// keep CbsTemp blocked longer (safe), never release it early; an "idle"
/// verdict is never cached, so the delete path always re-checks live.
fn servicing_busy_cached() -> bool {
    use std::cell::RefCell;
    use std::time::Instant;

    thread_local! {
        static BUSY_UNTIL: RefCell<Option<Instant>> = const { RefCell::new(None) };
    }

    BUSY_UNTIL.with(|c| {
        let mut slot = c.borrow_mut();
        cached_busy_core(&mut slot, Instant::now(), servicing_busy)
    })
}

/// Testable core of the busy-verdict cache: a cached "busy" wins until its
/// deadline; an "idle" verdict is never remembered.
fn cached_busy_core(
    cache: &mut Option<std::time::Instant>,
    now: std::time::Instant,
    live: impl FnOnce() -> bool,
) -> bool {
    use std::time::Duration;

    const BUSY_TTL: Duration = Duration::from_secs(15);

    if let Some(until) = *cache {
        if now < until {
            return true;
        }
    }
    if live() {
        *cache = Some(now + BUSY_TTL);
        true
    } else {
        *cache = None;
        false
    }
}

/// Pure admission check: only exact fixed layouts pass, case-insensitively.
pub fn is_deletable_layout(
    path: &str,
    layouts: &[(PathBuf, &'static str)],
) -> Option<&'static str> {
    let p = path
        .trim()
        .to_lowercase()
        .trim_end_matches('\\')
        .to_string();
    layouts
        .iter()
        .find(|(root, _)| root.to_string_lossy().to_lowercase().trim_end_matches('\\') == p)
        .map(|(_, reason)| *reason)
}

fn measure(path: &Path) -> (i64, bool) {
    crate::dirsize::walk_size_kb_capped(path)
}

/// Scan core over injected layouts — read-only, also the admission source for
/// deletion.
fn scan_with(layouts: &[(PathBuf, &'static str)], busy: bool) -> RegenerableScanResult {
    let mut items = Vec::new();
    let mut omitted = 0u32;
    for (path, reason) in layouts {
        if !path.try_exists().unwrap_or(false) {
            continue;
        }
        if items.len() >= RESULT_CAP {
            omitted += 1;
            continue;
        }
        let (size_kb, size_capped) = measure(path);
        let cbs = *reason == "cbs_temp";
        items.push(RegenerableItem {
            path: path.to_string_lossy().into_owned(),
            reason: reason.to_string(),
            size_kb,
            size_capped,
            deletable: !cbs || !busy,
            block_reason: if cbs && busy {
                Some("servicing_busy".into())
            } else {
                None
            },
        });
    }
    RegenerableScanResult { items, omitted }
}

/// Delete core over injected layouts. Re-admits every wanted path through a
/// fresh scan, keeps the layout directory, and reports locked children.
fn delete_with(
    layouts: &[(PathBuf, &'static str)],
    busy: bool,
    paths: &[String],
) -> Result<RegenerableCleanupReport, String> {
    let fresh = scan_with(layouts, busy);
    let fresh_paths: std::collections::HashSet<String> = fresh
        .items
        .iter()
        .filter(|i| i.deletable)
        .map(|i| i.path.to_lowercase())
        .collect();

    let mut report = RegenerableCleanupReport::default();
    for wanted in paths {
        let Some(reason) = is_deletable_layout(wanted, layouts) else {
            return Err(format!("not a regenerable layout: {wanted}"));
        };
        if !fresh_paths.contains(&wanted.to_lowercase()) {
            // Fresh scan no longer admits it (missing, or a gate flipped).
            let blocked = fresh
                .items
                .iter()
                .find(|i| i.path.to_lowercase() == wanted.to_lowercase())
                .filter(|i| !i.deletable);
            let why = blocked
                .and_then(|i| i.block_reason.clone())
                .unwrap_or_else(|| "layout is no longer present".into());
            report.skipped.push(RegenerableSkipped {
                path: wanted.clone(),
                message: why,
            });
            continue;
        }
        let path = Path::new(wanted);
        let (before, _) = measure(path);
        let mut failure: Option<String> = None;
        if let Ok(read) = std::fs::read_dir(path) {
            for entry in read.flatten() {
                let child = entry.path();
                // A link inside a regenerable layout points elsewhere; without
                // a backup the removal would lose the link (never its target).
                // Skip it and say so instead of silently dropping it.
                if crate::fsutil::is_reparse_point(&child) {
                    failure.get_or_insert_with(|| {
                        format!(
                            "{}: link not followed, kept",
                            entry.file_name().to_string_lossy()
                        )
                    });
                    continue;
                }
                let res = if child.is_dir() {
                    std::fs::remove_dir_all(&child)
                } else {
                    std::fs::remove_file(&child)
                };
                if let Err(e) = res {
                    // Locked/denied children stay and are reported, not hidden.
                    failure.get_or_insert_with(|| e.to_string());
                }
            }
        } else {
            failure = Some("layout directory unreadable".into());
        }
        let (after, _) = measure(path);
        report.reclaimed_kb += (before - after).max(0);
        match failure {
            Some(msg) => report.skipped.push(RegenerableSkipped {
                path: wanted.clone(),
                message: format!("{reason}: partially cleaned, {msg}"),
            }),
            None => report.cleaned.push(wanted.clone()),
        }
    }
    Ok(report)
}

/// Read-only scan of the fixed regenerable layouts on this machine.
pub fn scan_regenerable() -> RegenerableScanResult {
    scan_with(
        &fixed_layouts(&crate::triage::roots_from_env()),
        // Display tier: cached busy verdict is fine (fail-closed direction).
        servicing_busy_cached(),
    )
}

/// Delete the contents of the requested layouts after a fresh scan re-admits
/// every path. No backup (invariant 7): these layouts regenerate.
pub fn delete_regenerable(paths: &[String]) -> Result<RegenerableCleanupReport, String> {
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::executor::begin_cleanup_control();
    if crate::executor::reboot::any_pending() {
        return Err(crate::executor::reboot::PENDING_MESSAGE.to_string());
    }
    delete_with(
        &fixed_layouts(&crate::triage::roots_from_env()),
        servicing_busy(),
        paths,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_is_exact_and_case_insensitive() {
        let l = vec![
            (PathBuf::from(r"C:\W\Temp"), "win_temp"),
            (PathBuf::from(r"C:\U\T"), "user_temp"),
        ];
        assert_eq!(is_deletable_layout(r"c:\w\temp ", &l), Some("win_temp"));
        assert_eq!(is_deletable_layout(r"C:\U\T\", &l), Some("user_temp"));
        assert_eq!(is_deletable_layout(r"C:\W\Temp\Child", &l), None);
        assert_eq!(is_deletable_layout(r"C:\Elsewhere", &l), None);
        assert_eq!(
            is_deletable_layout(r"C:\W\Temp2", &l),
            None,
            "prefix is not enough"
        );
    }

    #[test]
    fn cbs_temp_is_blocked_while_servicing_is_busy() {
        let tmp = crate::test_support::TempDir::new("regenclean-busy");
        let cbs = tmp.path().join("CbsTemp");
        std::fs::create_dir_all(&cbs).unwrap();
        std::fs::write(cbs.join("pending.xml"), b"x").unwrap();
        let layouts = vec![(cbs.clone(), "cbs_temp")];
        let idle = scan_with(&layouts, false);
        assert!(idle.items[0].deletable);
        let busy = scan_with(&layouts, true);
        assert!(!busy.items[0].deletable);
        assert_eq!(
            busy.items[0].block_reason.as_deref(),
            Some("servicing_busy")
        );
        // A busy gate skips deletion instead of forcing it.
        let report = delete_with(&layouts, true, &[cbs.to_string_lossy().into_owned()]).unwrap();
        assert!(report.cleaned.is_empty());
        assert_eq!(report.skipped[0].message, "servicing_busy");
        assert!(cbs.join("pending.xml").exists(), "nothing was deleted");
    }

    #[cfg(windows)]
    #[test]
    fn delete_removes_contents_but_keeps_the_layout_directory() {
        let tmp = crate::test_support::TempDir::new("regenclean-del");
        let layout = tmp.path().join("Temp");
        std::fs::create_dir_all(layout.join("sub")).unwrap();
        std::fs::write(layout.join("a.bin"), b"xxxxxxxx").unwrap();
        std::fs::write(layout.join("sub").join("b.bin"), b"xxxxxxxx").unwrap();
        let layouts = vec![(layout.clone(), "win_temp")];
        let report =
            delete_with(&layouts, false, &[layout.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(report.cleaned, vec![layout.to_string_lossy().into_owned()]);
        assert!(report.skipped.is_empty());
        assert!(report.reclaimed_kb > 0, "reclaimed must be counted");
        assert!(layout.is_dir(), "the layout directory itself is kept");
        assert!(!layout.join("a.bin").exists());
        assert!(!layout.join("sub").exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_link_inside_the_layout_is_kept_and_reported() {
        let tmp = crate::test_support::TempDir::new("regenclean-link");
        let layout = tmp.path().join("Temp");
        std::fs::create_dir_all(&layout).unwrap();
        std::fs::write(layout.join("a.bin"), b"x").unwrap();
        // A junction child pointing elsewhere: the cleanup must keep it.
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(layout.join("alias"))
            .arg(&elsewhere)
            .status()
            .unwrap();
        assert!(status.success(), "junction fixture failed");

        let layouts = vec![(layout.clone(), "win_temp")];
        let report =
            delete_with(&layouts, false, &[layout.to_string_lossy().into_owned()]).unwrap();
        assert!(report
            .skipped
            .iter()
            .any(|s| s.message.contains("link not followed")));
        assert!(layout.join("alias").exists(), "the link must survive");
        assert!(
            elsewhere.join("keep.txt").exists() || elsewhere.is_dir(),
            "target untouched"
        );
        assert!(
            !layout.join("a.bin").exists(),
            "plain contents still removed"
        );
    }

    #[test]
    fn busy_verdict_cache_is_fail_closed_and_forgets_idle() {
        use std::time::{Duration, Instant};

        let t0 = Instant::now();
        let mut cache = None;
        // Idle is not remembered: the next call re-queries.
        assert!(!cached_busy_core(&mut cache, t0, || false));
        assert!(cache.is_none());
        // A busy verdict is cached: live() is not even consulted.
        assert!(cached_busy_core(&mut cache, t0, || true));
        let mut live_called = false;
        assert!(cached_busy_core(
            &mut cache,
            t0 + Duration::from_secs(5),
            || {
                live_called = true;
                false
            }
        ));
        assert!(!live_called, "a cached busy verdict must win until its TTL");
        // Past the deadline the verdict is re-measured.
        assert!(!cached_busy_core(
            &mut cache,
            t0 + Duration::from_secs(31),
            || false
        ));
        assert!(cache.is_none());
    }

    #[test]
    fn non_layout_paths_are_refused_before_any_delete() {
        let layouts = vec![(PathBuf::from(r"C:\W\Temp"), "win_temp")];
        let err = delete_with(&layouts, false, &[r"C:\Windows\System32".to_string()]).unwrap_err();
        assert!(err.starts_with("not a regenerable layout"));
    }

    #[cfg(windows)]
    #[test]
    fn a_pending_reboot_blocks_regenerable_deletion() {
        crate::executor::reboot::reset_for_test();
        crate::executor::reboot::mark_pending("source\u{0}key\u{0}Temp");
        let err = delete_regenerable(&[r"C:\Windows\Temp".to_string()]).unwrap_err();
        assert_eq!(err, crate::executor::reboot::PENDING_MESSAGE);
        crate::executor::reboot::reset_for_test();
    }
}
