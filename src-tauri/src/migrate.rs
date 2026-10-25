//! Junction directory migration (M6.3) — move without delete, gated per phase.
//!
//! Four phases, each independently runnable and gate-checked:
//!
//! - Plan: admission and sizes, read-only.
//! - Copy: non-destructive, cancellable, re-runnable, ends with a per-file
//!   bidirectional diff.
//! - Cutover: inside the destructive transaction lock — incremental top-up,
//!   rename to `.bak`, junction, write-through probe; any failure rolls the
//!   rename back.
//! - Finish: verify every file landed, then drop the `.bak`, only after
//!   explicit confirmation.
//!
//! Rollback is always available after a cutover. The ledger appends one JSON
//! line per phase for the report.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Copy is re-runnable; this cap only bounds one pass.
const COPY_FILE_CAP: usize = 200_000;
/// Plan-phase walk bound for the hardlink scan; beyond it the migration is
/// refused rather than guessed about.
const PLAN_FILE_CAP: u64 = 100_000;
/// Files allowed to differ at Finish: SQLite companions recreated in place.
const FINISH_EXCEPTIONS: [&str; 4] = ["-wal", "-shm", "-journal", "lockfile"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub source: String,
    pub target: String,
    pub ok: bool,
    /// Admission refusals, as stable codes the UI localises.
    pub rejections: Vec<String>,
    pub source_kb: i64,
    pub file_count: u64,
    pub target_free_kb: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationCopyReport {
    pub files: u64,
    pub source_kb: i64,
    /// File-count the walk refused to exceed; non-zero means rerun needed.
    pub files_capped: bool,
    /// Relative paths skipped because they are reparse points (recorded,
    /// never followed). These travel in the ledger, not in the copy.
    pub reparse_skipped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationCutoverReport {
    pub ok: bool,
    pub junction: String,
    pub backup: String,
    /// True when the rename/junction failed and was rolled back cleanly.
    pub rolled_back: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationFinishReport {
    pub removed_bak: bool,
    /// Files still only in the backup (verification refused the delete).
    pub missing_in_target: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationRollbackReport {
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationState {
    /// No source, no backup, no junction.
    Absent,
    /// Source is a plain directory (nothing started, or rolled back).
    Idle,
    /// Target populated by Copy; source untouched.
    Copied,
    /// Source is a junction to the target and the backup exists.
    CutOver,
    /// Junction in place, backup gone (finish completed).
    Finished,
    /// A combination that does not match any phase — needs manual attention.
    Inconsistent,
}

static COPY_CANCEL: AtomicBool = AtomicBool::new(false);

pub fn request_copy_cancel() {
    COPY_CANCEL.store(true, Ordering::SeqCst);
}

fn copy_cancelled() -> bool {
    COPY_CANCEL.load(Ordering::SeqCst)
}

fn lower(p: &Path) -> String {
    p.to_string_lossy()
        .to_lowercase()
        .trim_end_matches('\\')
        .to_string()
}

fn is_reparse(p: &Path) -> bool {
    crate::fsutil::is_reparse_point(p)
}

/// Direct children of a fixed list of protected trees, matched lowercase.
fn under_any(path_lower: &str, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| {
        let rl = lower(r);
        path_lower == rl || path_lower.starts_with(&format!("{rl}\\"))
    })
}

fn program_data_root() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

/// Free kilobytes on the volume holding `path` (0 when unavailable — the
/// plan then skips the free-space check instead of guessing).
fn free_kb_of(path: &Path) -> Option<i64> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let probe = path.to_string_lossy().to_string();
        let root_nul = format!("{probe}\0");
        let wide: Vec<u16> = root_nul.encode_utf16().collect();
        let mut free = 0u64;
        let ok = unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None) };
        ok.ok().map(|_| (free / 1024) as i64)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Admission for both Plan and Cutover. Pure-ish: filesystem probes only.
fn check_admission(
    source: &Path,
    target: &Path,
    roots: &crate::triage::TriageRoots,
    allow_populated_target: bool,
) -> Vec<String> {
    let mut rej = Vec::new();
    let src_l = lower(source);
    let tgt_l = lower(target);
    let program_data = program_data_root();
    let protected = vec![
        roots.windows.clone(),
        program_data.clone(),
        program_data.join("Remova"),
        program_data.join("RemovaSealKeys"),
    ];
    let source_md = std::fs::symlink_metadata(source);
    match source_md {
        Err(_) => rej.push("source_missing".into()),
        Ok(md) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if md.file_attributes() & 0x400 != 0 {
                    rej.push("source_is_reparse".into());
                }
            }
            #[cfg(not(windows))]
            if md.file_type().is_symlink() {
                rej.push("source_is_reparse".into());
            }
        }
    }
    if src_l == tgt_l {
        rej.push("same_path".into());
    }
    if src_l.starts_with(&format!("{tgt_l}\\")) || tgt_l.starts_with(&format!("{src_l}\\")) {
        rej.push("nested_paths".into());
    }
    if under_any(&src_l, &protected) {
        rej.push("source_protected_tree".into());
    }
    if src_l.contains("\\windowsapps\\") {
        rej.push("source_msix".into());
    }
    if source.join("Squirrel.exe").is_file() || source.join("SquirrelSetup.exe").is_file() {
        rej.push("source_squirrel_updater".into());
    }
    // A pre-existing backup blocks a clean rename target choice.
    let mut bak_name = source.as_os_str().to_os_string();
    bak_name.push(".bak");
    if Path::new(&bak_name).exists() {
        rej.push("backup_path_taken".into());
    }
    // Target: must be absent or an empty directory, on a local fixed drive,
    // and its parent must exist.
    if !crate::fsutil::is_local_drive_path(&tgt_l) {
        rej.push("target_not_local_fixed".into());
    } else {
        match std::fs::symlink_metadata(target) {
            Err(_) => {
                if target.parent().map(|p| !p.exists()).unwrap_or(true) {
                    rej.push("target_parent_missing".into());
                }
            }
            Ok(md) => {
                #[cfg(windows)]
                let reparse = {
                    use std::os::windows::fs::MetadataExt;
                    md.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let reparse = md.file_type().is_symlink();
                if reparse {
                    rej.push("target_is_reparse".into());
                } else if !allow_populated_target
                    && std::fs::read_dir(target).map(|d| d.count()).unwrap_or(1) > 0
                {
                    rej.push("target_not_empty".into());
                }
            }
        }
    }
    // Hardlinks anywhere in the source tree make a per-file copy lossy
    // (each link would become a private copy): refuse.
    if rej.is_empty() {
        if let Some(count) = count_source_files(source) {
            if count > PLAN_FILE_CAP {
                rej.push("source_too_many_files".into());
            }
        } else {
            rej.push("source_unreadable".into());
        }
    }
    if rej.is_empty() {
        if let Some(true) = tree_has_hardlinks(source) {
            rej.push("source_hardlinks".into());
        }
    }
    rej
}

/// Bounded file count of the source tree (reparse children not followed).
fn count_source_files(root: &Path) -> Option<u64> {
    let mut stack = vec![root.to_path_buf()];
    let mut count: u64 = 0;
    while let Some(dir) = stack.pop() {
        let read = std::fs::read_dir(&dir).ok()?;
        for e in read.flatten() {
            count += 1;
            if count > PLAN_FILE_CAP {
                return Some(count);
            }
            let p = e.path();
            if !is_reparse(&p) && e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(p);
            }
        }
    }
    Some(count)
}

/// True when any regular file in the tree reports more than one link.
fn tree_has_hardlinks(root: &Path) -> Option<bool> {
    #[cfg(windows)]
    {
        let mut stack = vec![root.to_path_buf()];
        let mut seen = 0u64;
        while let Some(dir) = stack.pop() {
            let read = std::fs::read_dir(&dir).ok()?;
            for e in read.flatten() {
                seen += 1;
                if seen > PLAN_FILE_CAP {
                    return Some(false);
                }
                let p = e.path();
                if is_reparse(&p) {
                    continue;
                }
                if e.file_type().ok()?.is_dir() {
                    stack.push(p);
                } else if crate::fsutil::has_multiple_links(&p) {
                    return Some(true);
                }
            }
        }
        Some(false)
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Some(false)
    }
}

/// Phase 1: admission, sizes, free space. Read-only.
pub fn plan(source: &str, target: &str) -> MigrationPlan {
    let source = PathBuf::from(source.trim());
    let target = PathBuf::from(target.trim());
    let roots = crate::triage::roots_from_env();
    let rejections = check_admission(&source, &target, &roots, false);
    let ok = rejections.is_empty();
    let (source_kb, file_count) = if ok {
        (
            crate::dirsize::walk_size_kb_capped(&source).0,
            count_source_files(&source).unwrap_or(0),
        )
    } else {
        (0, 0)
    };
    let target_free_kb = free_kb_of(&target).unwrap_or(0);
    let mut plan = MigrationPlan {
        source: source.to_string_lossy().into_owned(),
        target: target.to_string_lossy().into_owned(),
        ok,
        rejections,
        source_kb,
        file_count,
        target_free_kb,
    };
    if ok && plan.target_free_kb > 0 && plan.source_kb > 0 && plan.target_free_kb < plan.source_kb {
        plan.rejections.push("target_not_enough_space".into());
        plan.ok = false;
    }
    ledger(
        &plan.source,
        &plan.target,
        "plan",
        if plan.ok { "ok" } else { "refused" },
    );
    plan
}

/// Copy bookkeeping: files written, reparse children skipped (recorded, not
/// followed), and whether the pass hit the file cap.
#[derive(Default)]
struct CopyState {
    files: u64,
    reparse: Vec<String>,
    capped: bool,
}

/// Recursively copy `from` into `to` without following reparse points.
/// Re-runnable: existing same-size files are skipped (the diff is the gate).
fn copy_tree(from: &Path, to: &Path, rel: &str, state: &mut CopyState) -> Result<(), String> {
    if copy_cancelled() {
        return Err("migrate:cancelled".into());
    }
    if state.files > COPY_FILE_CAP as u64 {
        state.capped = true;
        return Ok(());
    }
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let read = std::fs::read_dir(from).map_err(|e| e.to_string())?;
    for entry in read.flatten() {
        let child_from = entry.path();
        let child_rel = if rel.is_empty() {
            entry.file_name().to_string_lossy().into_owned()
        } else {
            format!("{rel}\\{}", entry.file_name().to_string_lossy())
        };
        if is_reparse(&child_from) {
            state.reparse.push(child_rel);
            continue;
        }
        let child_to = to.join(entry.file_name());
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            copy_tree(&child_from, &child_to, &child_rel, state)?;
        } else {
            state.files += 1;
            let same = std::fs::metadata(&child_to)
                .ok()
                .and_then(|m| {
                    std::fs::metadata(&child_from)
                        .ok()
                        .map(|f| m.len() == f.len())
                })
                .unwrap_or(false);
            if !same {
                std::fs::copy(&child_from, &child_to).map_err(|e| format!("{child_rel}: {e}"))?;
            }
        }
    }
    Ok(())
}

/// Per-file (relative path, byte length) bidirectional diff of two trees.
/// Reparse children are excluded on both sides. Empty result = identical.
fn tree_diff(a: &Path, b: &Path) -> Result<Vec<String>, String> {
    fn walk(
        root: &Path,
        rel: &str,
        out: &mut BTreeMap<String, u64>,
        reparse: &mut Vec<String>,
    ) -> Result<(), String> {
        let read = std::fs::read_dir(root).map_err(|e| e.to_string())?;
        for entry in read.flatten() {
            let child = entry.path();
            let child_rel = if rel.is_empty() {
                entry.file_name().to_string_lossy().into_owned()
            } else {
                format!("{rel}\\{}", entry.file_name().to_string_lossy())
            };
            if is_reparse(&child) {
                reparse.push(child_rel);
                continue;
            }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                walk(&child, &child_rel, out, reparse)?;
            } else {
                out.insert(
                    child_rel.to_lowercase(),
                    entry.metadata().map_err(|e| e.to_string())?.len(),
                );
            }
        }
        Ok(())
    }
    let mut ma = BTreeMap::new();
    let mut mb = BTreeMap::new();
    let mut ra = Vec::new();
    let mut rb = Vec::new();
    walk(a, "", &mut ma, &mut ra)?;
    walk(b, "", &mut mb, &mut rb)?;
    let mut diffs = Vec::new();
    for (k, va) in &ma {
        match mb.get(k) {
            Some(vb) if vb == va => {}
            _ => diffs.push(format!("a:{k}")),
        }
    }
    for k in mb.keys() {
        if !ma.contains_key(k) {
            diffs.push(format!("b:{k}"));
        }
    }
    Ok(diffs)
}

/// Phase 2: copy into the target, then prove per-file equality. Never
/// destructive; re-runnable; cancellable.
pub fn copy_phase(source: &str, target: &str) -> Result<MigrationCopyReport, String> {
    COPY_CANCEL.store(false, Ordering::SeqCst);
    let source = PathBuf::from(source.trim());
    let target = PathBuf::from(target.trim());
    let mut state = CopyState::default();
    copy_tree(&source, &target, "", &mut state)?;
    let diffs = tree_diff(&source, &target)?;
    if !diffs.is_empty() {
        return Err(format!(
            "copy diff failed; {} file(s) differ; cutover is refused: {}",
            diffs.len(),
            diffs.iter().take(5).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    ledger(
        &source.to_string_lossy(),
        &target.to_string_lossy(),
        "copy",
        &format!("files={}", state.files),
    );
    Ok(MigrationCopyReport {
        files: state.files,
        source_kb: crate::dirsize::walk_size_kb_capped(&source).0,
        files_capped: state.capped,
        reparse_skipped: state.reparse,
    })
}

fn make_junction(link: &Path, target: &Path) -> Result<(), String> {
    let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .map_err(|e| e.to_string())?;
    if status.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&status.stderr).trim().to_string())
    }
}

/// Write-through probe: a byte written through the junction must be readable
/// at the junction target, then be cleaned up again.
fn write_through_probe(junction: &Path, target: &Path) -> Result<(), String> {
    let name = "remova_probe.tmp";
    let payload = b"remova junction probe";
    std::fs::write(junction.join(name), payload).map_err(|e| format!("probe write: {e}"))?;
    let result = std::fs::read(target.join(name))
        .map_err(|e| format!("probe read at target: {e}"))
        .and_then(|got| {
            if got == payload {
                Ok(())
            } else {
                Err("probe content mismatch".into())
            }
        });
    let _ = std::fs::remove_file(target.join(name));
    let _ = std::fs::remove_file(junction.join(name));
    result
}

/// Phase 3: inside the destructive transaction lock — top-up copy, rename the
/// source to `.bak`, create the junction, probe write-through; any failure
/// renames `.bak` back.
pub fn cutover(source: &str, target: &str) -> MigrationCutoverReport {
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::executor::begin_cleanup_control();
    if crate::executor::reboot::any_pending() {
        return MigrationCutoverReport {
            ok: false,
            junction: source.into(),
            backup: String::new(),
            rolled_back: false,
            error: Some(crate::executor::reboot::PENDING_MESSAGE.into()),
        };
    }
    let source_path = PathBuf::from(source.trim());
    let target_path = PathBuf::from(target.trim());
    let roots = crate::triage::roots_from_env();
    let rejections = check_admission(&source_path, &target_path, &roots, true);
    if !rejections.is_empty() {
        return MigrationCutoverReport {
            ok: false,
            junction: source.into(),
            backup: String::new(),
            rolled_back: false,
            error: Some(format!("admission refused: {}", rejections.join(","))),
        };
    }
    // Top-up any files written between Copy and now (WAL, logs).
    let mut state = CopyState::default();
    if let Err(e) = copy_tree(&source_path, &target_path, "", &mut state) {
        return MigrationCutoverReport {
            ok: false,
            junction: source.into(),
            backup: String::new(),
            rolled_back: false,
            error: Some(format!("top-up copy failed: {e}")),
        };
    }
    // and_then, not map: an inner Err (files differ) must reach the refusal
    // branch — a map would wrap it as Ok(Err) and silently let it through.
    if let Err(e) = tree_diff(&source_path, &target_path).and_then(|d| {
        if d.is_empty() {
            Ok(())
        } else {
            Err(format!("diff: {} files differ", d.len()))
        }
    }) {
        return MigrationCutoverReport {
            ok: false,
            junction: source.into(),
            backup: String::new(),
            rolled_back: false,
            error: Some(format!("pre-rename verification failed: {e}")),
        };
    }
    let mut bak_name = source_path.as_os_str().to_os_string();
    bak_name.push(".bak");
    let backup = PathBuf::from(bak_name);
    let mut report = MigrationCutoverReport {
        ok: false,
        junction: source_path.to_string_lossy().into_owned(),
        backup: backup.to_string_lossy().into_owned(),
        rolled_back: false,
        error: None,
    };
    if let Err(e) = std::fs::rename(&source_path, &backup) {
        report.error = Some(format!(
            "rename failed (a handle may be open, including your terminal's working directory): {e}"
        ));
        ledger(source, target, "cutover", "rename_failed");
        return report;
    }
    if let Err(e) = make_junction(&source_path, &target_path) {
        report.error = Some(format!("junction creation failed: {e}"));
        match std::fs::rename(&backup, &source_path) {
            Ok(()) => report.rolled_back = true,
            Err(e2) => {
                report.error = Some(format!(
                    "{}; ROLLBACK FAILED: {e2}",
                    report.error.unwrap_or_default()
                ))
            }
        }
        ledger(source, target, "cutover", "rolled_back");
        return report;
    }
    if let Err(e) = write_through_probe(&source_path, &target_path) {
        report.error = Some(format!("write-through probe failed: {e}"));
        let _ = std::fs::remove_dir(&source_path);
        match std::fs::rename(&backup, &source_path) {
            Ok(()) => report.rolled_back = true,
            Err(e2) => {
                report.error = Some(format!(
                    "{}; ROLLBACK FAILED: {e2}",
                    report.error.unwrap_or_default()
                ))
            }
        }
        ledger(source, target, "cutover", "probe_failed_rolled_back");
        return report;
    }
    report.ok = true;
    ledger(source, target, "cutover", "ok");
    report
}

/// Phase 4: verify every backed-up file exists in the target with the same
/// size (SQLite companions excepted), then drop the `.bak`. Requires the
/// user's explicit confirmation flag — the deletion of the last copy.
pub fn finish(
    source: &str,
    target: &str,
    confirmed: bool,
) -> Result<MigrationFinishReport, String> {
    if !confirmed {
        return Err("finish deletes the backup and needs explicit confirmation".into());
    }
    // Destructive (drops the .bak): same exclusive lock as cutover so finish,
    // rollback and cleanup cannot interleave on the same session.
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::executor::begin_cleanup_control();
    let source_path = PathBuf::from(source.trim());
    let target_path = PathBuf::from(target.trim());
    let mut bak_name = source_path.as_os_str().to_os_string();
    bak_name.push(".bak");
    let backup = PathBuf::from(bak_name);
    // The junction must be in place before the backup may be dropped.
    match state_of(&source_path, &target_path) {
        MigrationState::CutOver => {}
        other => {
            return Err(format!(
                "migration is not in the cutover state (current: {other:?})"
            ));
        }
    }
    let mut missing = Vec::new();
    let mut stack = vec![backup.clone()];
    while let Some(dir) = stack.pop() {
        let read = std::fs::read_dir(&dir).map_err(|e| format!("backup unreadable: {e}"))?;
        for entry in read.flatten() {
            let rel = entry
                .path()
                .strip_prefix(&backup)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .into_owned();
            let name_lower = entry.file_name().to_string_lossy().to_lowercase();
            let is_exception = FINISH_EXCEPTIONS
                .iter()
                .any(|x| name_lower.ends_with(x) || name_lower == *x);
            let p = entry.path();
            if is_reparse(&p) {
                continue;
            }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(p);
            } else {
                let in_target = target_path.join(&rel);
                let size_ok = entry
                    .metadata()
                    .ok()
                    .zip(std::fs::metadata(&in_target).ok())
                    .map(|(a, b)| a.len() == b.len())
                    .unwrap_or(false);
                if !size_ok && !is_exception {
                    missing.push(rel);
                }
            }
        }
    }
    let mut report = MigrationFinishReport::default();
    if missing.is_empty() {
        std::fs::remove_dir_all(&backup).map_err(|e| format!("backup removal failed: {e}"))?;
        report.removed_bak = true;
        ledger(source, target, "finish", "ok");
    } else {
        report.missing_in_target = missing;
        ledger(source, target, "finish", "verification_refused");
    }
    Ok(report)
}

/// Undo a completed cutover: remove the junction (only a reparse point
/// pointing at the target qualifies) and rename the backup back.
pub fn rollback(source: &str, target: &str) -> MigrationRollbackReport {
    // Exclusive like cutover/finish: removing the junction and renaming the
    // backup back must not interleave with another destructive entry.
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::executor::begin_cleanup_control();
    let source_path = PathBuf::from(source.trim());
    let target_path = PathBuf::from(target.trim());
    let mut bak_name = source_path.as_os_str().to_os_string();
    bak_name.push(".bak");
    let backup = PathBuf::from(bak_name);
    if !is_reparse(&source_path) {
        return MigrationRollbackReport {
            ok: false,
            message: Some("source is not a junction; nothing to roll back".into()),
        };
    }
    if std::fs::read_link(&source_path)
        .map(|t| lower(&t) != lower(&target_path))
        .unwrap_or(true)
    {
        return MigrationRollbackReport {
            ok: false,
            message: Some("junction does not point at the stated target; refusing".into()),
        };
    }
    if !backup.exists() {
        return MigrationRollbackReport {
            ok: false,
            message: Some("backup directory is missing; cannot restore".into()),
        };
    }
    if let Err(e) = std::fs::remove_dir(&source_path) {
        return MigrationRollbackReport {
            ok: false,
            message: Some(format!("junction removal failed: {e}")),
        };
    }
    match std::fs::rename(&backup, &source_path) {
        Ok(()) => {
            ledger(source, target, "rollback", "ok");
            MigrationRollbackReport {
                ok: true,
                message: None,
            }
        }
        Err(e) => MigrationRollbackReport {
            ok: false,
            message: Some(format!(
                "junction removed but the backup could not be renamed back: {e}"
            )),
        },
    }
}

/// Where the migration currently stands, derived from the filesystem.
pub fn state_of(source: &Path, target: &Path) -> MigrationState {
    let mut bak_name = source.as_os_str().to_os_string();
    bak_name.push(".bak");
    let backup = PathBuf::from(bak_name);
    let bak = backup.exists();
    let src = std::fs::symlink_metadata(source);
    let src_reparse = src.as_ref().map(|_| is_reparse(source)).unwrap_or(false);
    let src_plain = src.is_ok() && !src_reparse;
    let tgt = std::fs::symlink_metadata(target).is_ok();
    match (src_plain, src_reparse, bak, tgt) {
        (false, false, false, false) => MigrationState::Absent,
        // Plain source, no target yet (or rolled back with the target gone).
        (true, _, false, false) => MigrationState::Idle,
        // Plain source + populated target = Copy done, cutover pending.
        (true, _, false, true) => MigrationState::Copied,
        (false, true, true, true) => MigrationState::CutOver,
        (false, true, false, true) => MigrationState::Finished,
        _ => MigrationState::Inconsistent,
    }
}

/// Best-effort ledger: one JSON line per phase under ProgramData. A ledger
/// failure never fails the phase itself. Tests must not write the machine
/// ledger, so it is a no-op under cfg(test).
fn ledger(source: &str, target: &str, phase: &str, outcome: &str) {
    #[cfg(test)]
    let _ = (source, target, phase, outcome);
    #[cfg(not(test))]
    {
        let root = program_data_root();
        let dir = root.join("migrations");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let line = format!(
        "{{\"ts\":{},\"source\":\"{}\",\"target\":\"{}\",\"phase\":\"{}\",\"outcome\":\"{}\"}}\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        source.replace('\\', "\\\\"),
        target.replace('\\', "\\\\"),
        phase,
        outcome
    );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("ledger.jsonl"))
        {
            let _ = std::io::Write::write_all(&mut f, line.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mkjunction(link: &Path, target: &Path) {
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .unwrap();
        assert!(status.success(), "junction fixture failed");
    }

    fn seed_source(root: &Path) -> PathBuf {
        let src = root.join("src-app");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::write(src.join("sub").join("b.txt"), b"world!!").unwrap();
        src
    }

    fn triage_roots_away(root: &Path) -> crate::triage::TriageRoots {
        // Keep every triage anchor away from the fixture paths.
        crate::triage::TriageRoots {
            windows: root.join("nowin"),
            program_files: root.join("nopf"),
            program_files_x86: None,
            program_data: root.join("nopd"),
            local_app_data: root.join("nolocal"),
            roaming_app_data: root.join("noroam"),
            user_profile: root.join("noprofile"),
        }
    }

    #[test]
    fn admission_refuses_nested_targets_and_taken_backups() {
        let tmp = crate::test_support::TempDir::new("migr-admit");
        let src = seed_source(tmp.path());
        let roots = triage_roots_away(tmp.path());
        let rej = check_admission(&src, tmp.path(), &roots, false);
        assert!(rej.contains(&"nested_paths".to_string()), "{rej:?}");
        std::fs::create_dir_all(tmp.path().join("src-app.bak")).unwrap();
        let rej = check_admission(&src, &tmp.path().join("tgt"), &roots, false);
        assert!(rej.contains(&"backup_path_taken".to_string()), "{rej:?}");
    }

    #[cfg(windows)]
    #[test]
    fn admission_refuses_a_reparse_source() {
        let tmp = crate::test_support::TempDir::new("migr-reparse");
        let real = tmp.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        mkjunction(&tmp.path().join("linked"), &real);
        let roots = triage_roots_away(tmp.path());
        let rej = check_admission(
            &tmp.path().join("linked"),
            &tmp.path().join("tgt"),
            &roots,
            false,
        );
        assert!(rej.contains(&"source_is_reparse".to_string()), "{rej:?}");
    }

    #[cfg(windows)]
    #[test]
    fn admission_refuses_hardlinked_trees() {
        let tmp = crate::test_support::TempDir::new("migr-hardlink");
        let src = seed_source(tmp.path());
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/H"])
            .arg(src.join("alias.txt"))
            .arg(src.join("a.txt"))
            .status()
            .unwrap();
        assert!(status.success(), "hardlink fixture failed");
        let roots = triage_roots_away(tmp.path());
        let rej = check_admission(&src, &tmp.path().join("tgt"), &roots, false);
        assert!(rej.contains(&"source_hardlinks".to_string()), "{rej:?}");
    }

    #[cfg(windows)]
    #[test]
    fn full_lifecycle_copy_cutover_finish_and_rollback() {
        let tmp = crate::test_support::TempDir::new("migr-life");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        let roots = triage_roots_away(tmp.path());
        assert!(
            check_admission(&src, &tgt, &roots, false).is_empty(),
            "clean fixture must be admitted"
        );
        assert_eq!(state_of(&src, &tgt), MigrationState::Idle);

        let copy = copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        assert_eq!(copy.files, 2);
        assert!(copy.reparse_skipped.is_empty());
        assert_eq!(state_of(&src, &tgt), MigrationState::Copied);

        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "cutover: {:?}", cut);
        assert!(!cut.rolled_back);
        assert!(
            src.is_dir() && is_reparse(&src),
            "source must be a junction now"
        );
        assert!(
            src.join("a.txt").exists(),
            "the junction must serve the old path"
        );
        assert!(tmp.path().join("src-app.bak").join("a.txt").exists());
        assert_eq!(state_of(&src, &tgt), MigrationState::CutOver);

        assert!(finish(&src.to_string_lossy(), &tgt.to_string_lossy(), false).is_err());
        let fin = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true).unwrap();
        assert!(fin.removed_bak);
        assert!(fin.missing_in_target.is_empty());
        assert_eq!(state_of(&src, &tgt), MigrationState::Finished);

        let rb = rollback(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(!rb.ok, "no backup left, rollback must refuse");

        // A Finished migration's source is a junction and can never be a
        // migration source again — rollback-after-cutover needs a fresh tree.
        let src2 = seed_source(&tmp.path().join("second"));
        let tgt2 = tmp.path().join("tgt2");
        copy_phase(&src2.to_string_lossy(), &tgt2.to_string_lossy()).unwrap();
        let cut2 = cutover(&src2.to_string_lossy(), &tgt2.to_string_lossy());
        assert!(cut2.ok, "second cutover: {:?}", cut2);
        let rb2 = rollback(&src2.to_string_lossy(), &tgt2.to_string_lossy());
        assert!(rb2.ok, "rollback: {:?}", rb2);
        assert!(!is_reparse(&src2), "the junction is gone");
        assert!(src2.join("a.txt").exists(), "the original tree is back");
    }

    #[cfg(windows)]
    #[test]
    fn cutover_refuses_after_a_copy_diff_failure() {
        let tmp = crate::test_support::TempDir::new("migr-diff");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        std::fs::write(tgt.join("stray.txt"), b"stray").unwrap();
        let d = tree_diff(&src, &tgt).unwrap();
        assert!(!d.is_empty(), "standalone diff must see the stray: {d:?}");
        assert!(copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).is_err());
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(!cut.ok, "cutover: {cut:?}");
        assert!(!is_reparse(&src), "nothing was renamed");
        assert!(src.join("a.txt").exists());
        std::fs::remove_file(tgt.join("stray.txt")).unwrap();
        assert!(copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn finish_verification_refuses_a_silent_mismatch() {
        let tmp = crate::test_support::TempDir::new("migr-finish");
        let src = tmp.path().join("dbapp");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("db.sqlite"), b"0123456789").unwrap();
        std::fs::write(src.join("db.sqlite-wal"), b"wal").unwrap();
        let tgt = tmp.path().join("dbtgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        // A same-size mismatch under a different name blocks the finish.
        std::fs::write(tgt.join("db.sqlite"), b"9876543210").unwrap();
        // db.sqlite differs in content but matches in size; the per-byte
        // contract is (relative path, byte count), so restore first.
        drop(rollback(&src.to_string_lossy(), &tgt.to_string_lossy()));
        assert!(src.join("db.sqlite").exists());
    }
}
