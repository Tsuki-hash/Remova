//! Junction directory migration — move without delete, gated per phase.
//!
//! Four phases, each independently runnable and gate-checked:
//!
//! - Plan: admission and sizes. Writes only the audit ledger.
//! - Copy: starts in an empty target and records the directory identities for
//!   resume. Existing files must already match; a copy never replaces them.
//! - Cutover: inside the destructive transaction lock — incremental top-up,
//!   rename to `.bak`, junction, write-through probe; any failure rolls the
//!   rename back.
//! - Finish: drop the backup only while its matching target entries are held.
//!
//! Rollback refuses diverged trees. The ledger appends one JSON line per phase.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) mod copy_operation;
#[cfg(windows)]
mod finishing;
#[cfg(windows)]
mod junction;
mod receipt;
#[cfg(windows)]
mod rollback;
#[cfg(windows)]
mod security;
#[cfg(windows)]
mod streams;

/// Copy is re-runnable; this cap only bounds one pass.
const COPY_FILE_CAP: usize = 200_000;
/// Plan-phase walk bound for the hardlink scan; beyond it the migration is
/// refused rather than guessed about.
const PLAN_FILE_CAP: u64 = 100_000;
const SOURCE_TOO_DEEP: &str = "source_tree_too_deep";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub source: String,
    pub target: String,
    pub ok: bool,
    /// Admission refusals, as stable codes the UI localises.
    pub rejections: Vec<String>,
    pub source_kb: i64,
    /// True when the size estimate hit the walk cap and is a lower bound; the
    /// UI must present it as an estimate, never as a confirmed capacity.
    #[serde(default)]
    pub source_kb_capped: bool,
    pub file_count: u64,
    pub target_free_kb: i64,
    /// False when the free-space query failed: the plan then refuses instead
    /// of silently skipping the capacity check.
    #[serde(default = "default_true")]
    pub free_space_known: bool,
    /// The target root does not exist yet or its permissions cannot be read.
    /// Copy still verifies each destination directory before writing payloads.
    #[serde(default)]
    pub root_permissions_pending: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationCopyReport {
    pub files: u64,
    pub source_kb: i64,
    /// Protocol-compat field: the copy cap now aborts the pass with an Err
    /// (see `copy_phase`), so this is always false on a successful report.
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
    #[serde(rename = "cutover", alias = "cut_over")]
    CutOver,
    /// Junction in place, backup gone (finish completed).
    Finished,
    /// A combination that does not match any phase — needs manual attention.
    Inconsistent,
    /// The verified rollback copy was committed for removal. Cleanup can be retried.
    CleanupPending,
}

use copy_operation::{cancelled as copy_cancelled, CANCELLED};

fn lower(p: &Path) -> String {
    p.to_string_lossy()
        .to_lowercase()
        .trim_end_matches(['\\', '/'])
        .to_string()
}

fn junction_target(source: &Path, target: &Path) -> std::io::Result<(PathBuf, (u32, u64))> {
    #[cfg(windows)]
    {
        let target = junction::Target::open(target)?;
        let junction = junction::Junction::open(source, false)?;
        let linked = junction.verify_target(&target)?;
        Ok((linked.path().into(), target.receipt_id()?))
    }
    #[cfg(not(windows))]
    {
        let _ = (source, target);
        Err(std::io::Error::other(
            "directory migration requires Windows",
        ))
    }
}

fn sibling_suffix(path: &Path, suffix: &str) -> PathBuf {
    let Some(name) = path.file_name() else {
        return path.join(suffix);
    };
    let mut name = name.to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn backup_path(source: &Path) -> PathBuf {
    sibling_suffix(source, ".bak")
}

fn pending_path(source: &Path) -> PathBuf {
    sibling_suffix(source, ".bak.cleanup")
}

fn is_reparse(p: &Path) -> bool {
    crate::fsutil::is_reparse_point(p)
}

/// Reparse-ness straight from metadata already in hand (no second probe).
fn meta_is_reparse(md: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        md.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        md.file_type().is_symlink()
    }
}

fn meta_is_directory(md: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
        md.file_attributes() & FILE_ATTRIBUTE_DIRECTORY.0 != 0
    }
    #[cfg(not(windows))]
    {
        md.is_dir() || md.file_type().is_symlink()
    }
}

/// True when any directory ABOVE `p` is a reparse point: a source reached
/// through a junction would make the migration operate on the junction's
/// destination, not the path the user typed.
fn ancestors_have_reparse(p: &Path) -> bool {
    p.ancestors().skip(1).any(is_reparse)
}

/// Free kilobytes on the volume that would hold `target`. The target usually
/// does not exist yet at planning time, and the free-space query fails on a
/// missing directory — walk up to the nearest existing ancestor (worst case
/// the drive root) so a not-yet-created target still gets a real answer.
fn free_kb_for_target(target: &Path) -> Option<i64> {
    let mut probe = target.to_path_buf();
    loop {
        if probe.exists() {
            return free_kb_of(&probe);
        }
        match probe.parent() {
            Some(parent) if parent != probe => probe = parent.to_path_buf(),
            _ => return None,
        }
    }
}

/// Byte-for-byte comparison of two files, streaming in 64 KiB chunks. This is
/// the copy gate: a same-length file with different content must never be
/// mistaken for a completed copy.
fn files_equal(a: &Path, b: &Path) -> Result<bool, String> {
    #[cfg(not(windows))]
    use std::fs::File::open;
    #[cfg(windows)]
    use streams::open;
    let fa = open(a).map_err(|e| format!("{}: {e}", a.display()))?;
    let fb = open(b).map_err(|e| format!("{}: {e}", b.display()))?;
    readers_equal(fa, fb).map_err(|err| err.to_string())
}

#[cfg(windows)]
use streams::equal as readers_equal;

#[cfg(not(windows))]
fn readers_equal(mut fa: std::fs::File, mut fb: std::fs::File) -> std::io::Result<bool> {
    use std::io::Read;
    fn read_fill(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut n = 0;
        while n < buf.len() {
            match f.read(&mut buf[n..]) {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(e) => return Err(e),
            }
        }
        Ok(n)
    }
    let mut ba = [0u8; 64 * 1024];
    let mut bb = [0u8; 64 * 1024];
    loop {
        // A large file comparison must honour a cancel request; without this
        // one multi-GB file kept the copy/cutover unresponsive until it ended.
        if copy_cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "migrate:cancelled",
            ));
        }
        let na = read_fill(&mut fa, &mut ba)?;
        let nb = read_fill(&mut fb, &mut bb)?;
        if na != nb {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
        if ba[..na] != bb[..nb] {
            return Ok(false);
        }
    }
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
///
/// The path strings are ultimately passed to `cmd.exe /c mklink /J` at the
/// cutover phase. std quotes each argument so cmd metacharacters inside a
/// quoted arg stay literal — but a path containing `&` can still terminate
/// the quoted region if the path also has no spaces (std only quotes when the
/// arg contains whitespace). Rejecting these characters at admission is the
/// cheapest defence; a native reparse-point API would be the full fix.
fn path_has_cmd_metachars(path_lower: &str) -> bool {
    path_lower
        .chars()
        .any(|c| matches!(c, '<' | '>' | '&' | '|' | '^' | '%' | '\u{0}'))
}

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
    if !crate::fsutil::is_local_drive_path(&src_l) {
        rej.push("source_not_local_fixed".into());
    }
    // cmd.exe /c mklink /J re-interprets & | < > ^ % even inside quoted args
    // when the arg has no spaces; reject at admission so the metacharacter
    // never reaches the subprocess.
    for (label, p) in [("source", source), ("target", target)] {
        if path_has_cmd_metachars(&p.to_string_lossy()) {
            rej.push(format!("{label}_cmd_metachar"));
        }
    }
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
    if [backup_path(source), pending_path(source)]
        .iter()
        .any(|path| std::fs::symlink_metadata(path).is_ok())
    {
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
                if meta_is_reparse(&md) {
                    rej.push("target_is_reparse".into());
                } else if !allow_populated_target
                    && std::fs::read_dir(target).map(|d| d.count()).unwrap_or(1) > 0
                {
                    rej.push("target_not_empty".into());
                }
            }
        }
    }
    // A target inside a protected tree would put the migrated data (and the
    // later junction) where the tool itself refuses to operate.
    if under_any(&tgt_l, &protected) {
        rej.push("target_protected_tree".into());
    }
    // Reaching the source or target through a junction/mount point would make
    // every phase operate on the link destination rather than the typed path.
    if ancestors_have_reparse(source) {
        rej.push("source_ancestor_reparse".into());
    }
    if ancestors_have_reparse(target) {
        rej.push("target_ancestor_reparse".into());
    }
    // Hardlinks anywhere in the source tree make a per-file copy lossy
    // (each link would become a private copy): refuse.
    if rej.is_empty() {
        if let Err(reason) = count_source_files(source) {
            rej.push(reason.into());
        }
    }
    if rej.is_empty() {
        match tree_has_hardlinks(source) {
            Some(true) => rej.push("source_hardlinks".into()),
            // A tree we could not fully scan is not a tree to migrate.
            None => rej.push("source_unreadable".into()),
            Some(false) => {}
        }
    }
    rej
}

/// Path-shape re-check for the destructive tail (finish/rollback). The full
/// plan admission cannot apply — the source is already a junction — so every
/// path these phases may touch is verified here instead, inside the exclusive
/// lock, right before any delete or rename.
fn check_destructive(source: &Path, target: &Path, backup: &Path) -> Vec<String> {
    let mut rej = Vec::new();
    let src_l = lower(source);
    let tgt_l = lower(target);
    if src_l == tgt_l {
        rej.push("same_path".into());
    }
    if src_l.starts_with(&format!("{tgt_l}\\")) || tgt_l.starts_with(&format!("{src_l}\\")) {
        rej.push("nested_paths".into());
    }
    let roots = crate::triage::roots_from_env();
    let program_data = program_data_root();
    let protected = vec![
        roots.windows.clone(),
        program_data.clone(),
        program_data.join("Remova"),
        program_data.join("RemovaSealKeys"),
    ];
    for (label, p) in [("source", source), ("target", target), ("backup", backup)] {
        if under_any(&lower(p), &protected) {
            rej.push(format!("{label}_protected_tree"));
        }
        // The source itself is a junction at this point; ancestors are not.
        if ancestors_have_reparse(p) {
            rej.push(format!("{label}_ancestor_reparse"));
        }
    }
    if !crate::fsutil::is_local_drive_path(&tgt_l) {
        rej.push("target_not_local_fixed".into());
    }
    match std::fs::symlink_metadata(target) {
        Err(_) => rej.push("target_missing".into()),
        Ok(md) => {
            if meta_is_reparse(&md) {
                rej.push("target_is_reparse".into());
            } else if !md.is_dir() {
                rej.push("target_not_directory".into());
            }
        }
    }
    match std::fs::symlink_metadata(backup) {
        Err(_) => rej.push("backup_missing".into()),
        Ok(md) => {
            if meta_is_reparse(&md) {
                rej.push("backup_is_reparse".into());
            } else if !md.is_dir() {
                rej.push("backup_not_directory".into());
            }
        }
    }
    rej
}

/// Bounded file count of the source tree (reparse children not followed).
fn count_source_files(root: &Path) -> Result<u64, &'static str> {
    let mut stack = vec![(root.to_path_buf(), 0)];
    let mut count: u64 = 0;
    while let Some((dir, depth)) = stack.pop() {
        let read = std::fs::read_dir(&dir).map_err(|_| "source_unreadable")?;
        for e in read {
            // A dropped entry would understate the count.
            let e = e.map_err(|_| "source_unreadable")?;
            let child_depth = depth + 1;
            if child_depth > crate::fsutil::PINNED_TREE_MAX_DEPTH {
                return Err(SOURCE_TOO_DEEP);
            }
            count += 1;
            if count > PLAN_FILE_CAP {
                return Err("source_too_many_files");
            }
            let p = e.path();
            if !is_reparse(&p) && e.file_type().map_err(|_| "source_unreadable")?.is_dir() {
                stack.push((p, child_depth));
            }
        }
    }
    Ok(count)
}

/// True when any regular file in the tree reports more than one link.
fn tree_has_hardlinks(root: &Path) -> Option<bool> {
    #[cfg(windows)]
    {
        let mut stack = vec![root.to_path_buf()];
        let mut seen = 0u64;
        while let Some(dir) = stack.pop() {
            let read = std::fs::read_dir(&dir).ok()?;
            for e in read {
                // A dropped entry would skip its hardlink check.
                let e = e.ok()?;
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

/// Phase 1: admission, sizes, free space. Leaves both trees untouched.
pub fn plan(source: &str, target: &str) -> MigrationPlan {
    let source = PathBuf::from(source.trim());
    let target = PathBuf::from(target.trim());
    plan_with_free(&source, &target, free_kb_for_target(&target))
}

/// plan() with the free-space answer injected: the capacity rule is testable
/// without stubbing the Win32 query.
fn plan_with_free(source: &Path, target: &Path, free: Option<i64>) -> MigrationPlan {
    let roots = crate::triage::roots_from_env();
    let resuming = receipt::Receipt::load(source, target).is_some();
    let mut rejections = check_admission(source, target, &roots, resuming);
    let root_permissions_pending = if rejections.is_empty() {
        match existing_root_permissions_match(source, target) {
            Some(true) => false,
            Some(false) => {
                rejections.push("target_directory_permissions_differ".into());
                false
            }
            None => true,
        }
    } else {
        false
    };
    let ok = rejections.is_empty();
    let (source_kb, source_kb_capped, file_count) = if ok {
        let (kb, capped) = crate::dirsize::walk_size_kb_capped(source);
        (kb, capped, count_source_files(source).unwrap_or(0))
    } else {
        (0, false, 0)
    };
    let mut plan = MigrationPlan {
        source: source.to_string_lossy().into_owned(),
        target: target.to_string_lossy().into_owned(),
        ok,
        rejections,
        source_kb,
        source_kb_capped,
        file_count,
        target_free_kb: free.unwrap_or(0),
        free_space_known: free.is_some(),
        root_permissions_pending,
    };
    if ok {
        match free {
            // No capacity answer is not a pass: the plan refuses instead of
            // letting a copy walk into a full disk.
            None => {
                plan.rejections.push("target_space_unknown".into());
                plan.ok = false;
            }
            Some(f) if !resuming && f >= 0 && plan.source_kb > f => {
                plan.rejections.push("target_not_enough_space".into());
                plan.ok = false;
            }
            _ => {}
        }
    }
    ledger(
        &plan.source,
        &plan.target,
        "plan",
        if plan.ok { "ok" } else { "refused" },
    );
    plan
}

#[cfg(windows)]
fn existing_root_permissions_match(source: &Path, target: &Path) -> Option<bool> {
    let _source_path = crate::fsutil::pin_local_scan_path(source).ok()?;
    let _target_path = crate::fsutil::pin_local_scan_path(target).ok()?;
    let source = streams::open(source).ok()?;
    let target = streams::open(target).ok()?;
    security::directories_equal(&source, &target).ok()
}

#[cfg(not(windows))]
fn existing_root_permissions_match(_: &Path, _: &Path) -> Option<bool> {
    None
}

/// Copy bookkeeping: files written, reparse children skipped (recorded, not
/// followed), and whether the pass hit the file cap.
#[derive(Default)]
struct CopyState {
    files: u64,
    reparse: Vec<String>,
    capped: bool,
}

/// Use Windows' copy routine to preserve file attributes and streams while
/// accepting cancellation. The temporary file is published without replace.
#[cfg(windows)]
fn copy_new_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let flag = copy_operation::flag();
    copy_new_file_cancellable(source, destination, &|_| {
        flag.as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    })
}

#[cfg(windows)]
fn copy_new_file_cancellable<C: Fn(i64) -> bool + Sync>(
    source: &Path,
    destination: &Path,
    cancelled: &C,
) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        CopyFileExW, MoveFileExW, LPPROGRESS_ROUTINE_CALLBACK_REASON, MOVE_FILE_FLAGS,
    };

    unsafe extern "system" fn progress<C: Fn(i64) -> bool>(
        _: i64,
        transferred: i64,
        _: i64,
        _: i64,
        _: u32,
        _: LPPROGRESS_ROUTINE_CALLBACK_REASON,
        _: HANDLE,
        _: HANDLE,
        context: *const std::ffi::c_void,
    ) -> u32 {
        // CopyFileEx invokes the callback before it returns, while the
        // borrowed cancellation check is still alive.
        let cancelled = unsafe { &*context.cast::<C>() };
        u32::from(cancelled(transferred)) // PROGRESS_CONTINUE or PROGRESS_CANCEL
    }

    let _source_pins = crate::fsutil::pin_local_scan_path(source).map_err(|err| err.to_string())?;
    let temp = crate::fsutil::atomic_tmp_path(destination);
    let wide_source = crate::fsutil::wide_path_for_security(source);
    let wide_temp = crate::fsutil::wide_path_for_security(&temp);
    let wide_destination = crate::fsutil::wide_path_for_security(destination);
    let copy = unsafe {
        CopyFileExW(
            PCWSTR(wide_source.as_ptr()),
            PCWSTR(wide_temp.as_ptr()),
            Some(progress::<C>),
            Some(std::ptr::from_ref(cancelled).cast()),
            None,
            1, // COPY_FILE_FAIL_IF_EXISTS
        )
    };
    if let Err(err) = copy {
        if !matches!(err.code().0 as u32, 0x80070050 | 0x800700B7) {
            let _ = std::fs::remove_file(&temp);
        }
        return Err(if err.code().0 as u32 == 0x800704D3 || copy_cancelled() {
            "migrate:cancelled".into()
        } else {
            err.to_string()
        });
    }
    let moved = if copy_cancelled() {
        Err("migrate:cancelled".into())
    } else {
        unsafe {
            MoveFileExW(
                PCWSTR(wide_temp.as_ptr()),
                PCWSTR(wide_destination.as_ptr()),
                MOVE_FILE_FLAGS(0),
            )
        }
        .map_err(|err| err.to_string())
    };
    if moved.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    moved
}

#[cfg(not(windows))]
fn copy_new_file(_source: &Path, _destination: &Path) -> Result<(), String> {
    Err("directory migration requires Windows".into())
}

/// Recursively copy missing entries. Existing bytes are never replaced.
fn copy_tree(
    from: &Path,
    to: &Path,
    rel: &str,
    depth: usize,
    state: &mut CopyState,
) -> Result<(), String> {
    if copy_cancelled() {
        return Err("migrate:cancelled".into());
    }
    // Admission checks the source and target roots only. A pre-planted junction
    // or symlink below the target would make every later write land outside the
    // target tree, so the copy refuses the whole operation instead of following it.
    if is_reparse(to) {
        return Err(format!(
            "migrate:target reparse point refused: {}",
            to.display()
        ));
    }
    let _source = crate::fsutil::pin_target_readonly(from).map_err(|err| err.to_string())?;
    let _target = crate::fsutil::create_dirs_pinned(to).map_err(|err| err.to_string())?;
    #[cfg(windows)]
    {
        let source = streams::open(from).map_err(|err| err.to_string())?;
        let target = streams::open(to).map_err(|err| err.to_string())?;
        if !security::directories_equal(&source, &target).map_err(|err| err.to_string())? {
            return Err(format!(
                "migrate:directory_permissions_differ: {}",
                to.display()
            ));
        }
    }
    let read = std::fs::read_dir(from).map_err(|e| e.to_string())?;
    for entry in read {
        // An enumeration error is a hole in the copy, not a skippable entry.
        let entry = entry.map_err(|e| format!("{rel}: {e}"))?;
        if copy_cancelled() {
            return Err("migrate:cancelled".into());
        }
        if state.files >= COPY_FILE_CAP as u64 {
            state.capped = true;
            return Ok(());
        }
        let child_depth = depth + 1;
        if child_depth > crate::fsutil::PINNED_TREE_MAX_DEPTH {
            return Err(SOURCE_TOO_DEEP.into());
        }
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
        let is_dir = entry.file_type().map_err(|err| err.to_string())?.is_dir();
        if is_dir {
            copy_tree(&child_from, &child_to, &child_rel, child_depth, state)?;
        } else {
            state.files += 1;
            match std::fs::symlink_metadata(&child_to) {
                Ok(meta)
                    if meta.is_file()
                        && !meta_is_reparse(&meta)
                        && !crate::fsutil::has_multiple_links(&child_to)
                        && files_equal(&child_from, &child_to)? => {}
                Ok(_) => return Err(format!("migrate:copy_conflict: {child_rel}")),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    copy_new_file(&child_from, &child_to).map_err(|err| {
                        if err == "migrate:cancelled" {
                            err
                        } else {
                            format!("{child_rel}: {err}")
                        }
                    })?;
                }
                Err(err) => return Err(format!("{child_rel}: {err}")),
            }
        }
    }
    Ok(())
}

/// Compare exact relative paths, including empty directories. When resuming,
/// allow additions in the source but reject any divergence of existing bytes.
fn compare_trees(a: &Path, b: &Path, allow_extra_in_b: bool) -> Result<Vec<String>, String> {
    fn walk(
        root: &Path,
        rel: &Path,
        depth: usize,
        out: &mut BTreeMap<PathBuf, Option<u64>>,
    ) -> Result<(), String> {
        let _pin = crate::fsutil::pin_target_readonly(root).map_err(|err| err.to_string())?;
        let read = std::fs::read_dir(root).map_err(|e| e.to_string())?;
        for entry in read {
            // The copy can be cancelled while its final diff reads both trees.
            if copy_cancelled() {
                return Err("migrate:cancelled".into());
            }
            let entry = entry.map_err(|e| format!("{}: {e}", rel.display()))?;
            let child_depth = depth + 1;
            if child_depth > crate::fsutil::PINNED_TREE_MAX_DEPTH {
                return Err(SOURCE_TOO_DEEP.into());
            }
            let child = entry.path();
            let child_rel = rel.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&child).map_err(|err| err.to_string())?;
            if meta_is_reparse(&metadata) || !(metadata.is_file() || metadata.is_dir()) {
                return Err(format!("migrate:copy_conflict: {}", child_rel.display()));
            }
            if out.len() >= COPY_FILE_CAP {
                return Err("migrate:copy inventory limit exceeded".into());
            }
            out.insert(
                child_rel.clone(),
                metadata.is_file().then_some(metadata.len()),
            );
            if metadata.is_dir() {
                walk(&child, &child_rel, child_depth, out)?;
            }
        }
        Ok(())
    }
    let mut ma = BTreeMap::new();
    let mut mb = BTreeMap::new();
    walk(a, Path::new(""), 0, &mut ma)?;
    walk(b, Path::new(""), 0, &mut mb)?;
    let mut diffs = Vec::new();
    if !files_equal(a, b)? {
        diffs.push("root directory streams or permissions".into());
    }
    for (k, va) in &ma {
        match mb.get(k) {
            Some(vb) if vb == va => {
                if !files_equal(&a.join(k), &b.join(k))? {
                    diffs.push(k.display().to_string());
                }
            }
            _ => diffs.push(k.display().to_string()),
        }
    }
    if !allow_extra_in_b {
        diffs.extend(
            mb.keys()
                .filter(|key| !ma.contains_key(*key))
                .map(|key| key.display().to_string()),
        );
    }
    Ok(diffs)
}

fn tree_diff(a: &Path, b: &Path) -> Result<Vec<String>, String> {
    compare_trees(a, b, false)
}

/// Phase 2: copy into the target, then prove per-file equality. Never
/// destructive; re-runnable; cancellable; admitted server-side like every
/// other phase — an IPC caller cannot aim the copier at arbitrary paths.
fn copy_phase(source: &str, target: &str) -> Result<MigrationCopyReport, String> {
    let source = PathBuf::from(source.trim());
    let target = PathBuf::from(target.trim());
    // Serialized with the destructive phases: a cutover must not interleave
    // with a copy still filling the target.
    let _guard = crate::executor::wait_for_cleanup_lock(copy_cancelled).ok_or(CANCELLED)?;
    let roots = crate::triage::roots_from_env();
    let receipt = receipt::Receipt::load(&source, &target);
    let rejections = check_admission(&source, &target, &roots, receipt.is_some());
    if !rejections.is_empty() {
        return Err(format!("admission refused: {}", rejections.join(",")));
    }
    if copy_cancelled() {
        return Err(CANCELLED.into());
    }
    if !target.exists() {
        std::fs::create_dir(&target).map_err(|err| err.to_string())?;
    }
    let _target_pin = crate::fsutil::pin_target_readonly(&target).map_err(|err| err.to_string())?;
    let mut receipt = match receipt {
        Some(receipt) => receipt,
        None => {
            if std::fs::read_dir(&target)
                .map_err(|err| err.to_string())?
                .next()
                .is_some()
            {
                return Err("admission refused: target_not_empty".into());
            }
            let receipt = receipt::Receipt::new(&source, &target).map_err(|err| err.to_string())?;
            receipt.save(&target).map_err(|err| err.to_string())?;
            receipt
        }
    };
    verify_existing_target(&source, &target)?;
    let mut state = CopyState::default();
    copy_tree(&source, &target, "", 0, &mut state)?;
    if state.capped {
        return Err(format!(
            "copy stopped at the file cap ({}); rerun to continue",
            COPY_FILE_CAP
        ));
    }
    let diffs = tree_diff(&source, &target)?;
    if !diffs.is_empty() {
        return Err(format!(
            "copy diff failed; {} file(s) differ; cutover is refused: {}",
            diffs.len(),
            diffs.iter().take(5).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    if copy_cancelled() {
        return Err(CANCELLED.into());
    }
    receipt.complete = true;
    receipt.save(&target).map_err(|err| err.to_string())?;
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

fn verify_existing_target(source: &Path, target: &Path) -> Result<(), String> {
    let diffs = compare_trees(target, source, true)?;
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "migrate:copy_conflict: {}",
            diffs.into_iter().take(10).collect::<Vec<_>>().join(", ")
        ))
    }
}

#[cfg(windows)]
fn make_junction(link: &Path, target: &Path) -> Result<(), String> {
    // std quotes each arg for cmd.exe, so spaces and cmd metacharacters inside
    // the paths stay literal (the args array never re-parses through a shell).
    // Real-machine confirmation of odd path shapes lives in ACCEPTANCE H8.
    // mklink treats a trailing forward slash as part of a switch. The
    // admitted paths both name directories below a drive root.
    let link = link.to_str().ok_or("invalid junction path")?;
    let target = target.to_str().ok_or("invalid junction target")?;
    let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
        .args(["/c", "mklink", "/J"])
        .arg(link.trim_end_matches(['\\', '/']))
        .arg(target.trim_end_matches(['\\', '/']))
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
#[cfg(windows)]
fn write_through_probe(junction: &Path, target: &Path) -> Result<(), String> {
    // The probe file lands inside the user's directory: the name must be
    // unique so an existing file can never be overwritten (and then deleted)
    // by the probe. Timestamp + pid + attempt gives that, with a pre-check.
    let payload = b"remova junction probe";
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut name = String::new();
    for attempt in 0..8u32 {
        let candidate = format!(
            "remova_probe_{nanos}_{}_{}.tmp",
            std::process::id(),
            attempt
        );
        if !junction.join(&candidate).exists() {
            name = candidate;
            break;
        }
    }
    if name.is_empty() {
        return Err("probe could not find a free file name".into());
    }
    let result = std::fs::write(junction.join(&name), payload)
        .map_err(|e| format!("probe write: {e}"))
        .and_then(|()| {
            std::fs::read(target.join(&name))
                .map_err(|e| format!("probe read at target: {e}"))
                .and_then(|got| {
                    if got == payload {
                        Ok(())
                    } else {
                        Err("probe content mismatch".into())
                    }
                })
        });
    let _ = std::fs::remove_file(target.join(&name));
    let _ = std::fs::remove_file(junction.join(&name));
    result
}

/// Phase 3: inside the destructive transaction lock — top-up copy, rename the
/// source to `.bak`, create the junction, probe write-through; any failure
/// renames `.bak` back.
#[cfg(windows)]
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
    if !receipt::Receipt::load(&source_path, &target_path).is_some_and(|receipt| receipt.complete) {
        return MigrationCutoverReport {
            ok: false,
            junction: source.into(),
            backup: String::new(),
            rolled_back: false,
            error: Some("migrate:copy_required".into()),
        };
    }
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
    // Top up files written between Copy and now (WAL, logs).
    let mut state = CopyState::default();
    if let Err(e) = verify_existing_target(&source_path, &target_path)
        .and_then(|()| copy_tree(&source_path, &target_path, "", 0, &mut state))
    {
        return MigrationCutoverReport {
            ok: false,
            junction: source.into(),
            backup: String::new(),
            rolled_back: false,
            error: Some(e),
        };
    }
    let verified = junction::Target::open(&target_path).and_then(|target| {
        if !receipt::Receipt::load(&source_path, target.path())
            .is_some_and(|receipt| receipt.complete)
        {
            return Err(std::io::Error::other("migrate:copy_required"));
        }
        let snapshot = crate::fsutil::PinnedLocalDeleteTree::with_rename_security(
            &source_path,
            COPY_FILE_CAP + 1,
        )?;
        let pins = target.verify_copy(&snapshot, true)?;
        Ok((target, pins, snapshot))
    });
    let (held_target, _target_contents, mut snapshot) = match verified {
        Ok(verified) => verified,
        Err(e) => {
            return MigrationCutoverReport {
                ok: false,
                junction: source.into(),
                backup: String::new(),
                rolled_back: false,
                error: Some(format!("pre-rename verification failed: {e}")),
            }
        }
    };
    let backup = backup_path(&source_path);
    let mut report = MigrationCutoverReport {
        ok: false,
        junction: source_path.to_string_lossy().into_owned(),
        backup: backup.to_string_lossy().into_owned(),
        rolled_back: false,
        error: None,
    };
    let source_root = snapshot.release_children_for_rename();
    if let Err(e) = finishing::rename_to_sibling(source_root, &source_path, &backup) {
        report.error = Some(format!(
            "rename failed (a handle may be open, including your terminal's working directory): {e}"
        ));
        ledger(source, target, "cutover", "rename_failed");
        return report;
    }
    if let Err(e) = make_junction(&source_path, &target_path) {
        report.error = Some(format!("junction creation failed: {e}"));
        match finishing::rename_to_sibling(source_root, &backup, &source_path) {
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
    let junction = junction::Junction::open(&source_path, true);
    let probe = junction
        .as_ref()
        .map_err(ToString::to_string)
        .and_then(|junction| {
            let _linked = junction
                .verify_target(&held_target)
                .map_err(|err| err.to_string())?;
            write_through_probe(&source_path, &target_path)?;
            junction
                .verify_target(&held_target)
                .map_err(|err| err.to_string())?;
            Ok(())
        });
    if let Err(e) = probe {
        report.error = Some(format!("write-through probe failed: {e}"));
        let restored = junction
            .and_then(rollback::unlink_junction)
            .and_then(|()| finishing::rename_to_sibling(source_root, &backup, &source_path));
        match restored {
            Ok(()) => report.rolled_back = true,
            Err(e2) => {
                report.error = Some(format!(
                    "{}; ROLLBACK FAILED: {e2}",
                    report.error.unwrap_or_default()
                ))
            }
        }
        let outcome = if report.rolled_back {
            "probe_failed_rolled_back"
        } else {
            "rollback_failed"
        };
        ledger(source, target, "cutover", outcome);
        return report;
    }
    report.ok = true;
    ledger(source, target, "cutover", "ok");
    report
}

#[cfg(not(windows))]
pub fn cutover(source: &str, _: &str) -> MigrationCutoverReport {
    MigrationCutoverReport {
        ok: false,
        junction: source.into(),
        backup: String::new(),
        rolled_back: false,
        error: Some("directory migration requires Windows".into()),
    }
}

/// Phase 4: verify every backed-up entry against a held target counterpart.
/// then drop the `.bak`. Requires the user's explicit confirmation flag — the
/// deletion of the last copy.
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
    // Fails closed like every other destructive entry: with a pending reboot
    // (1641) the machine must not delete anything.
    if crate::executor::reboot::any_pending() {
        return Err(crate::executor::reboot::PENDING_MESSAGE.to_string());
    }
    let source_path = PathBuf::from(source.trim());
    let target_path = PathBuf::from(target.trim());
    let backup = backup_path(&source_path);
    let pending = pending_path(&source_path);
    // The junction must be in place AND point at the stated target before the
    // backup may be dropped — the shape check rollback applies, because finish
    // is strictly more destructive.
    if junction_target(&source_path, &target_path).is_err() {
        return Err(
            "migration is not in the cutover state (junction missing or pointing elsewhere)".into(),
        );
    }
    let staged = match state_of(&source_path, &target_path) {
        MigrationState::CutOver => false,
        MigrationState::CleanupPending => true,
        other => {
            return Err(format!(
                "migration is not in the cutover state (current: {other:?})"
            ));
        }
    };
    // Re-check every path the destructive part will touch, inside the lock.
    let refused = check_destructive(
        &source_path,
        &target_path,
        if staged { &pending } else { &backup },
    );
    if !refused.is_empty() {
        return Err(format!("refusing to finish: {}", refused.join(",")));
    }
    finish_backup(&source_path, &backup, &pending, &target_path, staged).inspect_err(|err| {
        let outcome = if err.starts_with("migrate:cleanup_pending:") {
            "cleanup_pending"
        } else {
            "verification_refused"
        };
        ledger(source, target, "finish", outcome);
    })?;
    let report = MigrationFinishReport {
        removed_bak: true,
        missing_in_target: Vec::new(),
    };
    ledger(source, target, "finish", "ok");
    Ok(report)
}

#[cfg(windows)]
use finishing::finish as finish_backup;

#[cfg(not(windows))]
fn finish_backup(_: &Path, _: &Path, _: &Path, _: &Path, _: bool) -> Result<(), String> {
    Err("directory migration requires Windows".into())
}

#[cfg(windows)]
use rollback::switch as rollback_backup;

#[cfg(not(windows))]
fn rollback_backup(
    _: &Path,
    _: &Path,
    _: &Path,
    _: impl FnOnce(),
) -> Result<(), (&'static str, String)> {
    Err(("unsupported", "directory migration requires Windows".into()))
}

/// Undo a completed cutover: remove the junction (only a reparse point
/// pointing at the target qualifies) and rename the backup back.
pub fn rollback(source: &str, target: &str) -> MigrationRollbackReport {
    rollback_with(source, target, || {})
}

fn rollback_with(
    source: &str,
    target: &str,
    after_verification: impl FnOnce(),
) -> MigrationRollbackReport {
    // Exclusive like cutover/finish: removing the junction and renaming the
    // backup back must not interleave with another destructive entry.
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::executor::begin_cleanup_control();
    // Fails closed like every other destructive entry: with a pending reboot
    // (1641) the machine must not move or delete anything.
    if crate::executor::reboot::any_pending() {
        return MigrationRollbackReport {
            ok: false,
            message: Some(crate::executor::reboot::PENDING_MESSAGE.to_string()),
        };
    }
    let source_path = PathBuf::from(source.trim());
    let target_path = PathBuf::from(target.trim());
    let backup = backup_path(&source_path);
    if !is_reparse(&source_path) {
        return MigrationRollbackReport {
            ok: false,
            message: Some("source is not a junction; nothing to roll back".into()),
        };
    }
    if junction_target(&source_path, &target_path).is_err() {
        return MigrationRollbackReport {
            ok: false,
            message: Some("junction does not point at the stated target; refusing".into()),
        };
    }
    if !backup.exists() {
        if state_of(&source_path, &target_path) == MigrationState::CleanupPending {
            return MigrationRollbackReport {
                ok: false,
                message: Some("finish committed the old copy for removal; the live target remains available and finish cleanup can be retried".into()),
            };
        }
        return MigrationRollbackReport {
            ok: false,
            message: Some("backup directory is missing; cannot restore".into()),
        };
    }
    // Re-check every path the destructive part will touch, inside the lock:
    // protected trees, ancestor reparse points, a plain backup directory.
    let refused = check_destructive(&source_path, &target_path, &backup);
    if !refused.is_empty() {
        return MigrationRollbackReport {
            ok: false,
            message: Some(format!("refusing to roll back: {}", refused.join(","))),
        };
    }
    match rollback_backup(&source_path, &backup, &target_path, after_verification) {
        Ok(()) => {
            ledger(source, target, "rollback", "ok");
            MigrationRollbackReport {
                ok: true,
                message: None,
            }
        }
        Err((outcome, message)) => {
            ledger(source, target, "rollback", outcome);
            MigrationRollbackReport {
                ok: false,
                message: Some(message),
            }
        }
    }
}

fn metadata_or_absent(path: &Path) -> std::io::Result<Option<std::fs::Metadata>> {
    #[cfg(windows)]
    let metadata = crate::fsutil::open_local_scan_leaf(path, 0x80, 1)
        .and_then(|(_parents, pin)| crate::fsutil::pin_metadata(&pin));
    #[cfg(not(windows))]
    let metadata = std::fs::symlink_metadata(path);
    match metadata {
        Ok(metadata) => Ok(Some(metadata)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Where the migration currently stands, derived from the filesystem.
pub fn state_of(source: &Path, target: &Path) -> MigrationState {
    let backup = backup_path(source);
    let pending = pending_path(source);
    let (Ok(src), Ok(tgt), Ok(bak), Ok(queued)) = (
        metadata_or_absent(source),
        metadata_or_absent(target),
        metadata_or_absent(&backup),
        metadata_or_absent(&pending),
    ) else {
        return MigrationState::Inconsistent;
    };
    if src
        .as_ref()
        .is_some_and(|metadata| !meta_is_directory(metadata))
        || [&tgt, &bak, &queued]
            .into_iter()
            .flatten()
            .any(|metadata| !metadata.is_dir() || meta_is_reparse(metadata))
    {
        return MigrationState::Inconsistent;
    }
    let src_reparse = src.as_ref().is_some_and(meta_is_reparse);
    let src_plain = src.is_some() && !src_reparse;
    let linked_target = if tgt.is_some() && src_reparse {
        junction_target(source, target).ok()
    } else {
        None
    };
    let target_matches = linked_target.is_some();
    if src_reparse && !target_matches {
        return MigrationState::Inconsistent;
    }
    if queued.is_some() {
        let matches_receipt = linked_target.is_some_and(|(linked, target_id)| {
            crate::fsutil::local_directory_id(&pending).is_ok_and(|backup_id| {
                receipt::Receipt::matches_cleanup(source, [&linked, target], backup_id, target_id)
            })
        });
        if bak.is_none() && matches_receipt {
            return MigrationState::CleanupPending;
        }
        return MigrationState::Inconsistent;
    }
    if let (None, Some((linked, target_id))) = (&bak, &linked_target) {
        let misplaced = source.with_file_name(".bak");
        match metadata_or_absent(&misplaced) {
            Ok(Some(metadata)) if meta_is_directory(&metadata) && !meta_is_reparse(&metadata) => {
                let confirmed_unrelated =
                    crate::fsutil::local_directory_id(&misplaced).is_ok_and(|id| {
                        receipt::Receipt::finished_with_other_backup(
                            source,
                            [linked, target],
                            *target_id,
                            id,
                        )
                    });
                if !confirmed_unrelated {
                    // A legacy rollback copy may survive after its receipt becomes unavailable.
                    return MigrationState::Inconsistent;
                }
            }
            Err(_) => return MigrationState::Inconsistent,
            _ => {}
        }
    }
    match (src_plain, target_matches, bak.is_some(), tgt.is_some()) {
        (false, false, false, false) => MigrationState::Absent,
        // Plain source, no target yet (or rolled back with the target gone).
        (true, _, false, false) => MigrationState::Idle,
        (true, _, false, true) => {
            if receipt::Receipt::load(source, target).is_some_and(|receipt| receipt.complete) {
                MigrationState::Copied
            } else {
                MigrationState::Idle
            }
        }
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
            eprintln!("migration ledger: could not create {}", dir.display());
            return;
        }
        // serde_json builds the line: hand-rolled escaping only covered
        // backslashes, so a quote or newline in an IPC string broke the row.
        let line = {
            let record = serde_json::json!({
                "ts": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                "source": source,
                "target": target,
                "phase": phase,
                "outcome": outcome,
            });
            format!("{record}\n")
        };
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("ledger.jsonl"))
        {
            let _ = std::io::Write::write_all(&mut f, line.as_bytes());
        } else {
            eprintln!("migration ledger: append failed for phase {phase}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn trailing_separators_keep_the_backup_beside_the_source() {
        let fixture = crate::test_support::TempDir::new("migration_trailing_separator");
        for (name, separator, roll_back) in [("first", '\\', true), ("second", '/', false)] {
            let source = fixture.subdir(name);
            let target = fixture.join(format!("{name}-target"));
            let entered = format!("{}{separator}", source.display());
            let destination = target.to_str().unwrap();
            let copy_destination = format!("{destination}{separator}");
            let archive = fixture.join(format!("{name}.bak"));
            std::fs::write(source.join("data"), b"fixture").unwrap();

            assert!(plan(&entered, destination).ok);
            copy_phase(&entered, &copy_destination).unwrap();
            assert_eq!(
                state_of(Path::new(&entered), &target),
                MigrationState::Copied
            );
            let report = cutover(&entered, destination);
            assert!(report.ok, "{report:?}");
            assert_eq!(Path::new(&report.backup), archive);
            assert_eq!(std::fs::read(archive.join("data")).unwrap(), b"fixture");
            assert!(!fixture.join(".bak").exists());
            assert_eq!(state_of(&source, &target), MigrationState::CutOver);

            if roll_back {
                assert!(rollback(&entered, destination).ok);
                assert!(!is_reparse(&source));
            } else {
                assert!(finish(&entered, destination, true).unwrap().removed_bak);
                assert_eq!(state_of(&source, &target), MigrationState::Finished);
            }
            assert!(!archive.exists());
            assert_eq!(std::fs::read(source.join("data")).unwrap(), b"fixture");
            assert_eq!(std::fs::read(target.join("data")).unwrap(), b"fixture");
        }
    }

    #[cfg(windows)]
    #[test]
    fn copy_with_a_legacy_forward_slash_receipt_can_continue() {
        let fixture = crate::test_support::TempDir::new("migration_legacy_slash");
        let source = fixture.subdir("source");
        let target = fixture.join("target");
        let from = source.to_str().unwrap();
        let to = target.to_str().unwrap();
        std::fs::write(source.join("data"), b"fixture").unwrap();
        copy_phase(from, to).unwrap();

        let key = lower(&target);
        let receipts = fixture.join("receipts");
        let filename = |key: &str| format!("{}.json", crate::path_seal::map_digest(key.as_bytes()));
        let current = receipts.join(filename(&key));
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&current).unwrap()).unwrap();
        let legacy_key = format!("{key}/");
        receipt["target"] = serde_json::Value::String(legacy_key.clone());
        std::fs::write(
            receipts.join(filename(&legacy_key)),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        std::fs::remove_file(current).unwrap();

        assert_eq!(state_of(&source, &target), MigrationState::Copied);
        assert!(cutover(from, to).ok);
        assert!(rollback(from, to).ok);
        assert_eq!(std::fs::read(source.join("data")).unwrap(), b"fixture");
    }

    #[cfg(windows)]
    #[test]
    fn a_misplaced_backup_with_a_matching_copy_receipt_is_preserved() {
        let fixture = crate::test_support::TempDir::new("migration_misplaced_archive");
        let source = fixture.subdir("source");
        let target = fixture.join("target");
        let misplaced = fixture.join(".bak");
        let from = source.to_str().unwrap();
        let to = target.to_str().unwrap();
        std::fs::write(source.join("data"), b"fixture").unwrap();
        copy_phase(from, to).unwrap();
        std::fs::rename(&source, &misplaced).unwrap();
        mkjunction(&source, &target);

        assert_eq!(state_of(&source, &target), MigrationState::Inconsistent);
        assert!(!rollback(from, to).ok);
        assert!(finish(from, to, true).is_err());
        assert_eq!(std::fs::read(misplaced.join("data")).unwrap(), b"fixture");
        assert_eq!(std::fs::read(source.join("data")).unwrap(), b"fixture");

        std::fs::rename(fixture.join("receipts"), fixture.join("offline-receipts")).unwrap();
        assert_eq!(state_of(&source, &target), MigrationState::Inconsistent);
        assert!(!rollback(from, to).ok);
        assert!(finish(from, to, true).is_err());
        assert_eq!(std::fs::read(misplaced.join("data")).unwrap(), b"fixture");
    }

    #[test]
    fn migration_state_wire_matches_frontend_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../src/__tests__/fixtures/migration-state-wire.json"
        ))
        .unwrap();
        let states = [
            MigrationState::Absent,
            MigrationState::Idle,
            MigrationState::Copied,
            MigrationState::CutOver,
            MigrationState::Finished,
            MigrationState::Inconsistent,
            MigrationState::CleanupPending,
        ];
        assert_eq!(serde_json::to_value(states).unwrap(), fixture["states"]);
        for state in states {
            let wire = serde_json::to_string(&state).unwrap();
            assert_eq!(
                serde_json::from_str::<MigrationState>(&wire).unwrap(),
                state
            );
        }
        assert_eq!(
            serde_json::from_value::<MigrationState>(fixture["legacy_cutover"].clone()).unwrap(),
            MigrationState::CutOver
        );
    }

    fn mkjunction(link: &Path, target: &Path) {
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .unwrap();
        assert!(status.success(), "junction fixture failed");
    }

    #[cfg(windows)]
    #[test]
    fn copy_refuses_a_linked_target_subtree_without_touching_the_target() {
        let tmp = crate::test_support::TempDir::new("migr-linked-target");
        let source = tmp.path().join("source");
        std::fs::create_dir_all(source.join("sub")).unwrap();
        std::fs::write(source.join("sub").join("sentinel.bin"), b"source bytes").unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        // A junction planted at the exact path the copy would write into.
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel.bin"), b"outside must stay").unwrap();
        mkjunction(&target.join("sub"), &outside);

        // A cancel is armed when no run registered a generation; register this
        // fixture's run so the copy proceeds to the target check.
        let mut state = CopyState::default();
        let err = copy_tree(&source, &target, "", 0, &mut state).unwrap_err();
        assert!(
            err.contains("target reparse point refused"),
            "must refuse the linked target subtree: {err}"
        );
        // The external tree is byte-identical: nothing was written through the link.
        assert_eq!(
            std::fs::read(outside.join("sentinel.bin")).unwrap(),
            b"outside must stay"
        );
        assert!(!outside.join("sentinel2.bin").exists());
        assert_eq!(
            std::fs::read(source.join("sub").join("sentinel.bin")).unwrap(),
            b"source bytes",
            "the source is never modified"
        );
    }

    #[cfg(windows)]
    #[test]
    fn migration_state_wire_derived_cutover_uses_canonical_value() {
        // Only a new isolated fixture is changed; no migration/ledger or host
        // registry/API is invoked. Exercise the actual status -> serde boundary.
        let tmp = crate::test_support::TempDir::new("migr-wire");
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(tmp.path().join("source.bak")).unwrap();
        mkjunction(&source, &target);
        let state = state_of(&source, &target);
        // Remove only the junction, not its target, even before assertions.
        std::fs::remove_dir(&source).unwrap();
        assert_eq!(state, MigrationState::CutOver);
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../src/__tests__/fixtures/migration-state-wire.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), fixture["states"][3]);
    }

    #[cfg(windows)]
    #[test]
    fn migration_state_requires_a_live_junction_to_the_requested_directory() {
        let tmp = crate::test_support::TempDir::new("migration_status_targets");
        let source = tmp.join("source");
        let target = tmp.subdir("target");
        let other = tmp.subdir("different-target");
        let missing = tmp.join("missing");
        let ordinary_file = tmp.join("file");
        std::fs::write(&ordinary_file, b"fixture").unwrap();
        mkjunction(&source, &target);

        assert_eq!(state_of(&source, &target), MigrationState::Finished);
        for incorrect in [&other, &missing, &ordinary_file] {
            assert_eq!(state_of(&source, incorrect), MigrationState::Inconsistent);
        }
        std::fs::create_dir(backup_path(&source)).unwrap();
        assert_eq!(state_of(&source, &target), MigrationState::CutOver);
        assert_eq!(state_of(&source, &other), MigrationState::Inconsistent);

        std::fs::remove_dir(&target).unwrap();
        assert_eq!(state_of(&source, &target), MigrationState::Inconsistent);
    }

    #[cfg(windows)]
    #[test]
    fn case_sensitive_target_siblings_cannot_authorize_finish_or_rollback() {
        let tmp = crate::test_support::TempDir::new("migration_case_sensitive_target");
        let parent = tmp.subdir("case-sensitive");
        let capability = std::process::Command::new(crate::regops::sys_tool("fsutil.exe"))
            .args(["file", "SetCaseSensitiveInfo"])
            .arg(&parent)
            .arg("enable")
            .output()
            .unwrap();
        if !capability.status.success() {
            eprintln!("Windows volume does not allow this case-sensitive fixture: {capability:?}");
            return;
        }
        let target = parent.join("Target");
        let sibling = parent.join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        assert_ne!(
            crate::fsutil::local_directory_id(&target).unwrap(),
            crate::fsutil::local_directory_id(&sibling).unwrap()
        );

        let source = tmp.join("source");
        let backup = tmp.subdir("source.bak");
        std::fs::write(backup.join("data"), b"archived data").unwrap();
        std::fs::write(sibling.join("data"), b"archived data").unwrap();
        std::fs::write(target.join("data"), b"late live data").unwrap();
        mkjunction(&source, &target);

        assert_eq!(state_of(&source, &target), MigrationState::CutOver);
        assert_eq!(state_of(&source, &sibling), MigrationState::Inconsistent);
        let src = source.to_str().unwrap();
        let wrong = sibling.to_str().unwrap();
        assert!(!rollback(src, wrong).ok);
        assert!(finish(src, wrong, true).is_err());
        assert_eq!(
            std::fs::read(source.join("data")).unwrap(),
            b"late live data"
        );
        assert_eq!(
            std::fs::read(backup.join("data")).unwrap(),
            b"archived data"
        );
        assert_eq!(
            std::fs::read(sibling.join("data")).unwrap(),
            b"archived data"
        );
        assert!(!pending_path(&source).exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_replaced_junction_target_ancestor_refuses_state_and_destructive_phases() {
        let tmp = crate::test_support::TempDir::new("migration_target_ancestor");
        let source = tmp.join("source");
        let target = tmp.subdir("parent").join("target");
        let decoy = tmp.subdir("decoy").join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&decoy).unwrap();
        let original = tmp.join("original");
        let backup = tmp.subdir("source.bak");
        std::fs::write(target.join("data"), b"original data").unwrap();
        std::fs::write(decoy.join("data"), b"decoy data").unwrap();
        std::fs::write(backup.join("data"), b"decoy data").unwrap();
        mkjunction(&source, &target);
        assert_eq!(state_of(&source, &target), MigrationState::CutOver);

        std::fs::rename(target.parent().unwrap(), &original).unwrap();
        mkjunction(target.parent().unwrap(), decoy.parent().unwrap());
        assert_eq!(std::fs::read(source.join("data")).unwrap(), b"decoy data");
        for requested in [&target, &decoy] {
            assert_eq!(state_of(&source, requested), MigrationState::Inconsistent);
            assert!(!rollback(source.to_str().unwrap(), requested.to_str().unwrap()).ok);
            assert!(finish(source.to_str().unwrap(), requested.to_str().unwrap(), true).is_err());
        }
        assert!(is_reparse(&source));
        assert_eq!(std::fs::read(backup.join("data")).unwrap(), b"decoy data");
        assert_eq!(
            std::fs::read(original.join("target/data")).unwrap(),
            b"original data"
        );
        assert!(!pending_path(&source).exists());
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_path_aliases_for_the_same_target_support_finish_and_rollback() {
        let tmp = crate::test_support::TempDir::new("migration_target_alias");
        let target = tmp.subdir("ordinary-target");
        let alias = PathBuf::from(format!("{}\\\\ORDINARY-TARGET", tmp.path().display()));
        assert_ne!(lower(&alias), lower(&target));
        assert_eq!(
            crate::fsutil::local_directory_id(&target).unwrap(),
            crate::fsutil::local_directory_id(&alias).unwrap()
        );
        std::fs::write(target.join("data"), b"matching data").unwrap();
        let source = tmp.join("source");
        let backup = tmp.subdir("source.bak");
        std::fs::write(backup.join("data"), b"matching data").unwrap();
        mkjunction(&source, &target);
        assert_eq!(state_of(&source, &alias), MigrationState::CutOver);
        assert!(rollback(source.to_str().unwrap(), alias.to_str().unwrap()).ok);
        assert!(!is_reparse(&source));
        assert_eq!(
            std::fs::read(source.join("data")).unwrap(),
            b"matching data"
        );
        std::fs::rename(&source, &backup).unwrap();
        mkjunction(&source, &target);
        assert!(
            finish(source.to_str().unwrap(), alias.to_str().unwrap(), true)
                .unwrap()
                .removed_bak
        );
        assert_eq!(state_of(&source, &alias), MigrationState::Finished);
        assert_eq!(
            std::fs::read(source.join("data")).unwrap(),
            b"matching data"
        );
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
    fn rollback_keeps_the_junction_when_the_backup_is_locked() {
        let tmp = crate::test_support::TempDir::new("migr-rb-rename");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        let roots = triage_roots_away(tmp.path());
        assert!(check_admission(&src, &tgt, &roots, false).is_empty());
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        cutover(&src.to_string_lossy(), &tgt.to_string_lossy());

        // Hold the backup open with no delete sharing. The rollback must
        // preserve the source until the backup is safe to restore.
        let backup = tmp.path().join("src-app.bak");
        // A handle on a file inside the directory blocks renaming that directory
        // (verified on Windows); a handle on the directory itself does not.
        use std::os::windows::fs::OpenOptionsExt;
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x0000_0001)
            .open(backup.join("a.txt"))
            .unwrap();
        let rb = rollback(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(!rb.ok, "the rename cannot succeed while the handle is held");
        // The migrated data stays reachable and the rollback remains retryable.
        assert!(src.is_dir(), "the source path must still resolve");
        assert!(src.join("a.txt").is_file(), "migrated data stays reachable");
        let retry = rollback(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(!retry.ok, "still held: the retry reports failure honestly");
        drop(held);
        let recovered = rollback(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(
            recovered.ok,
            "after release the same entry recovers: {recovered:?}"
        );
        assert!(src.join("a.txt").is_file(), "the original tree is back");
    }

    #[cfg(windows)]
    #[test]
    fn rollback_pins_the_verified_archive_and_junction_until_the_switch() {
        let tmp = crate::test_support::TempDir::new("migration_rollback_identity");
        let source = seed_source(tmp.path());
        let target = tmp.join("tgt");
        let src = source.to_str().unwrap();
        let dst = target.to_str().unwrap();
        copy_phase(src, dst).unwrap();
        assert!(cutover(src, dst).ok);
        let backup = backup_path(&source);
        let original = std::fs::read(backup.join("a.txt")).unwrap();
        let result = rollback_with(src, dst, || {
            assert!(std::fs::rename(&backup, tmp.join("displaced")).is_err());
            assert!(std::fs::rename(&source, tmp.join("displaced-junction")).is_err());
            assert!(std::fs::remove_dir(&source).is_err());
        });
        assert!(result.ok, "{result:?}");
        assert!(!is_reparse(&source));
        assert!(!backup.exists());
        assert_eq!(std::fs::read(source.join("a.txt")).unwrap(), original);
        assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), original);
    }

    #[cfg(windows)]
    #[test]
    fn rollback_restores_the_junction_when_the_archive_changes_at_the_switch() {
        let tmp = crate::test_support::TempDir::new("migration_rollback_change");
        let source = seed_source(tmp.path());
        let target = tmp.join("tgt");
        let src = source.to_str().unwrap();
        let dst = target.to_str().unwrap();
        copy_phase(src, dst).unwrap();
        assert!(cutover(src, dst).ok);
        let backup = backup_path(&source);
        let original = std::fs::read(target.join("a.txt")).unwrap();
        let result = rollback_with(src, dst, || {
            std::fs::write(backup.join("a.txt"), b"changed during rollback").unwrap();
        });
        assert!(!result.ok, "{result:?}");
        let error = result.message.unwrap();
        assert!(
            error.contains("migrate:rollback_conflict:")
                && error.contains("the junction was restored"),
            "{error}"
        );
        assert_eq!(lower(&std::fs::read_link(&source).unwrap()), lower(&target));
        assert_eq!(
            std::fs::read(backup.join("a.txt")).unwrap(),
            b"changed during rollback"
        );
        assert_eq!(std::fs::read(source.join("a.txt")).unwrap(), original);
        assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), original);

        std::fs::write(backup.join("a.txt"), &original).unwrap();
        let late_empty = backup.join("late-empty");
        let result = rollback_with(src, dst, || {
            std::fs::create_dir(&late_empty).unwrap();
        });
        assert!(!result.ok, "{result:?}");
        assert!(result
            .message
            .unwrap()
            .contains("migrate:rollback_conflict:"));
        assert!(late_empty.is_dir());
        assert!(!target.join("late-empty").exists());
        assert_eq!(lower(&std::fs::read_link(&source).unwrap()), lower(&target));

        std::fs::remove_dir(&late_empty).unwrap();
        let name = backup.join("a.txt");
        let result = rollback_with(src, dst, || {
            std::fs::remove_file(&name).unwrap();
            std::fs::write(&name, &original).unwrap();
        });
        assert!(!result.ok, "{result:?}");
        assert!(result
            .message
            .unwrap()
            .contains("backup entry identities changed"));
        assert_eq!(std::fs::read(name).unwrap(), original);
        assert_eq!(lower(&std::fs::read_link(&source).unwrap()), lower(&target));
    }

    #[cfg(windows)]
    #[test]
    fn rollback_recovers_from_a_lock_acquired_after_verification() {
        use std::os::windows::fs::OpenOptionsExt;

        let tmp = crate::test_support::TempDir::new("migration_rollback_late_lock");
        let source = seed_source(tmp.path());
        let target = tmp.join("tgt");
        let src = source.to_str().unwrap();
        let dst = target.to_str().unwrap();
        copy_phase(src, dst).unwrap();
        assert!(cutover(src, dst).ok);
        let backup = backup_path(&source);
        let original = std::fs::read(target.join("a.txt")).unwrap();
        let mut held = None;
        let result = rollback_with(src, dst, || {
            held = Some(
                std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1)
                    .open(backup.join("a.txt"))
                    .unwrap(),
            );
        });
        assert!(!result.ok, "{result:?}");
        assert!(result
            .message
            .unwrap()
            .contains("the junction was restored"));
        assert_eq!(lower(&std::fs::read_link(&source).unwrap()), lower(&target));
        assert_eq!(std::fs::read(backup.join("a.txt")).unwrap(), original);
        assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), original);
        drop(held);
        assert!(rollback(src, dst).ok);
        assert!(!is_reparse(&source));
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
    fn grant_guest_read(path: &Path) {
        let result = std::process::Command::new(crate::regops::sys_tool("icacls.exe"))
            .arg(path)
            .args(["/grant", "*S-1-5-32-546:(OI)(CI)(RX)"])
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
    }

    #[cfg(windows)]
    #[test]
    fn copy_refuses_a_different_inherited_directory_acl_before_writing_payload() {
        let tmp = crate::test_support::TempDir::new("migration_parent_acl");
        let source = seed_source(tmp.path());
        let target_parent = tmp.subdir("different_inheritance");
        grant_guest_read(&target_parent);
        let target = target_parent.join("tgt");
        let matching = tmp.subdir("matching");
        let matching_plan = plan(&source.to_string_lossy(), &matching.to_string_lossy());
        assert!(matching_plan.ok);
        assert!(!matching_plan.root_permissions_pending);
        let missing_plan = plan(&source.to_string_lossy(), &target.to_string_lossy());
        assert!(missing_plan.ok);
        assert!(missing_plan.root_permissions_pending);
        assert!(!target.exists(), "planning must not create the destination");

        let error = copy_phase(&source.to_string_lossy(), &target.to_string_lossy()).unwrap_err();
        assert!(error.contains("permissions"), "{error}");
        assert!(std::fs::read_dir(&target).unwrap().next().is_none());
        let mismatched_plan = plan(&source.to_string_lossy(), &target.to_string_lossy());
        assert!(!mismatched_plan.ok);
        assert_eq!(
            mismatched_plan.rejections,
            ["target_directory_permissions_differ"]
        );
        assert!(!mismatched_plan.root_permissions_pending);
        assert!(source.join("a.txt").is_file());
        assert!(!backup_path(&source).exists());
    }

    #[cfg(windows)]
    #[test]
    fn migration_applies_the_snapshot_depth_limit_before_cutover() {
        let tmp = crate::test_support::TempDir::new("migration_depth_limit");
        let source = tmp.subdir("source");
        let target = tmp.join("target");
        let src = source.to_str().unwrap();
        let dst = target.to_str().unwrap();
        let mut deepest = PathBuf::new();
        for _ in 1..crate::fsutil::PINNED_TREE_MAX_DEPTH {
            deepest.push("n");
            std::fs::create_dir(source.join(&deepest)).unwrap();
        }
        let accepted = deepest.join("data");
        std::fs::write(source.join(&accepted), b"verified data").unwrap();
        assert!(plan(src, dst).ok);
        copy_phase(src, dst).unwrap();

        // An added directory brings its child past the maximum object depth.
        let too_deep = deepest.join("n");
        std::fs::create_dir(source.join(&too_deep)).unwrap();
        let late = source.join(&too_deep).join("late");
        std::fs::write(&late, b"new data").unwrap();
        let unused = tmp.join("unused");
        let unused = unused.to_str().unwrap();
        assert!(plan(src, unused)
            .rejections
            .iter()
            .any(|r| r == SOURCE_TOO_DEEP));
        assert!(copy_phase(src, unused)
            .unwrap_err()
            .contains(SOURCE_TOO_DEEP));
        assert!(!Path::new(unused).exists());
        let cut = cutover(src, dst);
        assert!(!cut.ok, "{cut:?}");
        assert!(cut.error.unwrap().contains(SOURCE_TOO_DEEP));
        assert!(!is_reparse(&source));
        assert!(!backup_path(&source).exists());
        assert_eq!(std::fs::read(&late).unwrap(), b"new data");
        assert_eq!(
            std::fs::read(target.join(&accepted)).unwrap(),
            b"verified data"
        );

        std::fs::remove_file(&late).unwrap();
        std::fs::remove_dir(source.join(&too_deep)).unwrap();
        let cut = cutover(src, dst);
        assert!(cut.ok, "{cut:?}");
        assert!(finish(src, dst, true).unwrap().removed_bak);
        assert_eq!(
            std::fs::read(source.join(&accepted)).unwrap(),
            b"verified data"
        );
    }

    #[cfg(windows)]
    #[test]
    fn directory_acl_drift_blocks_finish_and_rollback_without_changing_either_tree() {
        let tmp = crate::test_support::TempDir::new("migration_target_acl");
        let source = seed_source(tmp.path());
        let target = tmp.join("tgt");
        let src = source.to_string_lossy();
        let dst = target.to_string_lossy();
        copy_phase(&src, &dst).unwrap();
        assert!(cutover(&src, &dst).ok);
        grant_guest_read(&target);
        assert!(finish(&src, &dst, true).is_err());
        assert!(!rollback(&src, &dst).ok);
        assert_eq!(state_of(&source, &target), MigrationState::CutOver);
        assert!(backup_path(&source).join("a.txt").is_file());
        assert!(source.join("a.txt").is_file());
        assert!(!pending_path(&source).exists());
    }

    #[cfg(windows)]
    #[test]
    fn finish_refuses_a_locked_entry_before_committing_the_rollback_copy() {
        use std::os::windows::fs::OpenOptionsExt;

        let tmp = crate::test_support::TempDir::new("migration_finish_lock");
        let source = seed_source(tmp.path());
        let target = tmp.join("tgt");
        let src = source.to_string_lossy();
        let dst = target.to_string_lossy();
        copy_phase(&src, &dst).unwrap();
        assert!(cutover(&src, &dst).ok);
        let backup = backup_path(&source);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1) // FILE_SHARE_READ
            .open(backup.join("a.txt"))
            .unwrap();
        let error = finish(&src, &dst, true).unwrap_err();
        assert!(!error.starts_with("migrate:cleanup_pending:"), "{error}");
        assert!(backup.join("a.txt").is_file());
        assert!(!pending_path(&source).exists());
        assert_eq!(state_of(&source, &target), MigrationState::CutOver);
        drop(held);
        assert!(rollback(&src, &dst).ok);
        assert!(!is_reparse(&source));
    }

    #[cfg(windows)]
    #[test]
    fn finish_retries_partial_cleanup_and_refuses_a_substituted_pending_copy() {
        let tmp = crate::test_support::TempDir::new("migration_finish_retry");
        let source = seed_source(tmp.path());
        let target = tmp.join("tgt");
        let alias = format!("{}\\\\TGT", tmp.path().display());
        let src = source.to_string_lossy();
        let dst = target.to_string_lossy();
        std::fs::write(source.join("zzz-readonly.txt"), b"readonly").unwrap();
        copy_phase(&src, &dst).unwrap();
        assert!(cutover(&src, &dst).ok);
        let old_readonly = backup_path(&source).join("zzz-readonly.txt");
        let original_permissions = old_readonly.metadata().unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        std::fs::set_permissions(&old_readonly, readonly_permissions).unwrap();

        let error = finish(&src, &alias, true).unwrap_err();
        assert!(error.starts_with("migrate:cleanup_pending:"), "{error}");
        assert_eq!(state_of(&source, &target), MigrationState::CleanupPending);
        assert_eq!(
            state_of(&source, Path::new(&alias)),
            MigrationState::CleanupPending
        );
        assert_eq!(
            state_of(&source, &tmp.subdir("another-target")),
            MigrationState::Inconsistent
        );
        let pending = pending_path(&source);
        assert!(!backup_path(&source).exists());
        assert!(
            !pending.join("a.txt").exists(),
            "an earlier child was removed"
        );
        assert_eq!(
            std::fs::read(source.join("zzz-readonly.txt")).unwrap(),
            b"readonly"
        );
        assert!(source.join("a.txt").is_file());
        let rollback = rollback(&src, &dst);
        assert!(!rollback.ok);
        assert!(rollback.message.unwrap().contains("cleanup can be retried"));

        let displaced = tmp.join("displaced");
        std::fs::rename(&pending, &displaced).unwrap();
        std::fs::create_dir(&pending).unwrap();
        std::fs::write(
            pending.join("a.txt"),
            std::fs::read(target.join("a.txt")).unwrap(),
        )
        .unwrap();
        assert_eq!(state_of(&source, &target), MigrationState::Inconsistent);
        assert!(finish(&src, &dst, true).is_err());
        assert!(pending.join("a.txt").is_file());
        std::fs::remove_file(pending.join("a.txt")).unwrap();
        std::fs::remove_dir(&pending).unwrap();
        std::fs::rename(displaced, &pending).unwrap();

        std::fs::set_permissions(pending.join("zzz-readonly.txt"), original_permissions).unwrap();
        assert!(finish(&src, &dst, true).unwrap().removed_bak);
        assert!(!pending.exists());
        assert!(source.join("a.txt").is_file());
        assert_eq!(state_of(&source, &target), MigrationState::Finished);
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
    fn the_probe_never_touches_an_existing_probe_named_file() {
        let tmp = crate::test_support::TempDir::new("migr-probe");
        let target = tmp.path().join("tgt");
        std::fs::create_dir_all(&target).unwrap();
        // A stale file with the historic probe name must survive a cutover.
        // It lives in the source so the copy diff stays green and the file
        // travels to the target like any other content.
        let src = seed_source(tmp.path());
        std::fs::write(src.join("remova_probe.tmp"), b"user data").unwrap();
        copy_phase(&src.to_string_lossy(), &target.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &target.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        assert_eq!(
            std::fs::read(target.join("remova_probe.tmp")).unwrap(),
            b"user data",
            "the probe must not overwrite or delete an existing file"
        );
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
        // A same-size, different-content target written before the junction
        // was created must block finish itself. The earlier version of this
        // test only called rollback, so a finish that accepted the mismatch
        // still passed.
        let tampered = tgt.join("db.sqlite");
        std::fs::write(&tampered, b"9876543210").unwrap();
        // Force the write strictly earlier than the junction so it cannot be
        // excused as a legitimate post-cutover application write.
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        set_file_modified(&tampered, old);
        let backup = tmp.path().join("dbapp.bak");
        let result = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true);
        assert!(
            result.is_err(),
            "finish must refuse a pre-cutover content mismatch: {result:?}"
        );
        assert!(backup.is_dir(), "the backup must survive the refusal");
        assert_eq!(
            std::fs::read(tgt.join("db.sqlite")).unwrap(),
            b"9876543210",
            "finish must not rewrite the target on refusal"
        );
        // The timestamp cannot prove that a post-cutover rewrite is safe.
        std::fs::write(&tampered, b"9876543210").unwrap();
        set_file_modified(&tampered, std::time::SystemTime::now());
        let refused = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true);
        assert!(
            refused.is_err() && backup.is_dir(),
            "a post-cutover write must preserve both copies: {refused:?}"
        );
    }

    /// Set a file's modification time on Windows (used to separate a
    /// pre-cutover tamper from a legitimate post-cutover write).
    fn set_file_modified(path: &std::path::Path, when: std::time::SystemTime) {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let file = std::fs::OpenOptions::new()
                .write(true)
                .share_mode(0x7)
                .open(path)
                .unwrap();
            file.set_modified(when).unwrap();
        }
        #[cfg(not(windows))]
        {
            let _ = (path, when);
        }
    }

    /// A same-sized foreign file cannot authorize a copy or a cutover.
    #[test]
    fn copy_and_cutover_preserve_an_unowned_populated_target() {
        let tmp = crate::test_support::TempDir::new("migr-heal");
        let src = tmp.path().join("app");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("db.sqlite"), b"0123456789").unwrap();
        let tgt = tmp.path().join("tgt");
        std::fs::create_dir_all(&tgt).unwrap();
        std::fs::write(tgt.join("db.sqlite"), b"9876543210").unwrap();
        std::fs::write(tgt.join("extra.txt"), b"independent data").unwrap();
        assert_eq!(state_of(&src, &tgt), MigrationState::Idle);
        assert!(copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).is_err());
        let cutover = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert_eq!(cutover.error.as_deref(), Some("migrate:copy_required"));
        assert_eq!(std::fs::read(tgt.join("db.sqlite")).unwrap(), b"9876543210");
        assert_eq!(
            std::fs::read(tgt.join("extra.txt")).unwrap(),
            b"independent data"
        );
    }

    #[cfg(windows)]
    #[test]
    fn interrupted_copy_resumes_only_for_the_recorded_directories() {
        let tmp = crate::test_support::TempDir::new("migr-resume");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        std::fs::create_dir(&tgt).unwrap();
        receipt::Receipt::new(&src, &tgt)
            .unwrap()
            .save(&tgt)
            .unwrap();
        std::fs::copy(src.join("a.txt"), tgt.join("a.txt")).unwrap();
        assert_eq!(state_of(&src, &tgt), MigrationState::Idle);
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        assert_eq!(state_of(&src, &tgt), MigrationState::Copied);
        std::fs::rename(&tgt, tmp.path().join("previous")).unwrap();
        std::fs::create_dir(&tgt).unwrap();
        std::fs::write(tgt.join("a.txt"), b"replaced directory").unwrap();
        assert_eq!(state_of(&src, &tgt), MigrationState::Idle);
        assert!(copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).is_err());
        assert_eq!(
            std::fs::read(tgt.join("a.txt")).unwrap(),
            b"replaced directory"
        );
    }

    #[cfg(windows)]
    #[test]
    fn cutover_tops_up_new_files_but_preserves_conflicting_target_bytes() {
        let tmp = crate::test_support::TempDir::new("migr-top-up");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        let source = src.to_string_lossy();
        let target = tgt.to_string_lossy();
        copy_phase(&source, &target).unwrap();
        std::fs::write(src.join("new.txt"), b"new file").unwrap();
        std::fs::write(tgt.join("a.txt"), b"target edit").unwrap();
        assert!(!cutover(&source, &target).ok);
        assert_eq!(std::fs::read(tgt.join("a.txt")).unwrap(), b"target edit");
        assert!(!tgt.join("new.txt").exists());
        std::fs::write(tgt.join("a.txt"), b"hello").unwrap();
        assert!(cutover(&source, &target).ok);
        assert_eq!(std::fs::read(tgt.join("new.txt")).unwrap(), b"new file");
    }

    #[cfg(windows)]
    #[test]
    fn migration_native_copy_cancels_during_transfer_without_publishing() {
        use std::sync::atomic::Ordering;

        let tmp = crate::test_support::TempDir::new("migr-copy-cancel");
        let source = tmp.path().join("source.bin");
        let target = tmp.path().join("target.bin");
        std::fs::write(&source, vec![42; 4 * 1024 * 1024]).unwrap();
        let saw_bytes = std::sync::atomic::AtomicBool::new(false);
        let result = copy_new_file_cancellable(&source, &target, &|bytes| {
            if bytes > 0 {
                saw_bytes.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        });
        assert!(saw_bytes.load(Ordering::SeqCst));
        assert_eq!(result.unwrap_err(), "migrate:cancelled");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
        assert!(!target.exists());
    }

    #[cfg(windows)]
    #[test]
    fn finish_and_rollback_preserve_a_different_named_data_stream() {
        let tmp = crate::test_support::TempDir::new("migr-streams");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        let source = src.to_string_lossy();
        let target = tgt.to_string_lossy();
        let stream = |root: &Path| format!("{}:fixture", root.join("a.txt").display());
        std::fs::write(stream(&src), b"original stream").unwrap();
        copy_phase(&source, &target).unwrap();
        assert!(cutover(&source, &target).ok);
        assert_eq!(std::fs::read(stream(&tgt)).unwrap(), b"original stream");
        std::fs::write(stream(&tgt), b"changed stream").unwrap();
        assert!(finish(&source, &target, true).is_err());
        assert!(!rollback(&source, &target).ok);
        assert!(is_reparse(&src));
        assert_eq!(
            std::fs::read(stream(&tmp.path().join("src-app.bak"))).unwrap(),
            b"original stream"
        );
    }

    #[cfg(windows)]
    #[test]
    fn rollback_preserves_the_junction_and_both_copies_when_live_data_changed() {
        let tmp = crate::test_support::TempDir::new("migr-rollback-diff");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        let source = src.to_string_lossy();
        let target = tgt.to_string_lossy();
        copy_phase(&source, &target).unwrap();
        assert!(cutover(&source, &target).ok);
        std::fs::write(src.join("a.txt"), b"updated").unwrap();
        std::fs::write(src.join("new.txt"), b"new file").unwrap();
        let report = rollback(&source, &target);
        assert!(!report.ok && is_reparse(&src));
        assert_eq!(std::fs::read(src.join("a.txt")).unwrap(), b"updated");
        assert!(src.join("new.txt").is_file());
        assert_eq!(
            std::fs::read(tmp.path().join("src-app.bak").join("a.txt")).unwrap(),
            b"hello"
        );
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::remove_file(src.join("new.txt")).unwrap();
        assert!(rollback(&source, &target).ok);
        assert!(!is_reparse(&src));
    }

    #[test]
    fn files_equal_tells_content_apart() {
        let tmp = crate::test_support::TempDir::new("migr-eq");
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::write(&a, b"same").unwrap();
        std::fs::write(&b, b"same").unwrap();
        assert!(files_equal(&a, &b).unwrap());
        std::fs::write(&b, b"sane").unwrap();
        assert!(
            !files_equal(&a, &b).unwrap(),
            "same length, different bytes"
        );
        std::fs::write(&b, b"same and more").unwrap();
        assert!(
            !files_equal(&a, &b).unwrap(),
            "a length difference is a difference"
        );
    }

    /// A plan whose free-space query failed refuses instead of silently
    /// skipping the capacity check; a known-but-tight target is refused too.
    #[test]
    fn a_plan_refuses_when_the_free_space_is_unknown_or_tight() {
        let tmp = crate::test_support::TempDir::new("migr-space");
        let src = seed_source(tmp.path());
        // A source of at least 2 KB so a 1 KB free target is genuinely tight.
        std::fs::write(src.join("big.bin"), vec![0u8; 4096]).unwrap();
        let tgt = tmp.path().join("tgt");
        let unknown = plan_with_free(&src, &tgt, None);
        assert!(
            unknown
                .rejections
                .contains(&"target_space_unknown".to_string()),
            "{:?}",
            unknown.rejections
        );
        assert!(!unknown.ok);
        assert!(!unknown.free_space_known);
        let ok = plan_with_free(&src, &tgt, Some(1 << 20));
        assert!(ok.ok, "{:?}", ok.rejections);
        assert!(ok.free_space_known);
        let tight = plan_with_free(&src, &tgt, Some(1));
        assert!(
            tight
                .rejections
                .contains(&"target_not_enough_space".to_string()),
            "{:?}",
            tight.rejections
        );
        // The real plan() answers for a not-yet-created target through its
        // nearest existing ancestor: the normal planning flow (type a path
        // the copy will create) still gets a capacity check, not "unknown".
        #[cfg(windows)]
        {
            let fresh_target = tmp.path().join("deep").join("new-target");
            let planned = plan(&src.to_string_lossy(), &fresh_target.to_string_lossy());
            assert!(
                !planned
                    .rejections
                    .contains(&"target_space_unknown".to_string()),
                "{:?}",
                planned.rejections
            );
            assert!(planned.free_space_known);
        }
    }

    /// The copy runs the same server-side admission: an IPC caller cannot aim
    /// it at arbitrary paths, and nothing is written on a refusal.
    #[test]
    fn copy_phase_refuses_an_unadmitted_target() {
        let tmp = crate::test_support::TempDir::new("migr-copyrefuse");
        let src = seed_source(tmp.path());
        let protected = program_data_root().join("Remova").join("migr-refuse-test");
        let err = copy_phase(&src.to_string_lossy(), &protected.to_string_lossy()).unwrap_err();
        assert!(err.contains("target_protected_tree"), "{err}");
        assert!(!protected.exists(), "nothing was written");
    }

    /// A source reached through a junction would make every phase operate on
    /// the link destination: admission refuses it.
    #[cfg(windows)]
    #[test]
    fn admission_refuses_a_source_behind_a_junction() {
        let tmp = crate::test_support::TempDir::new("migr-ancestor");
        let real = tmp.path().join("real");
        std::fs::create_dir_all(real.join("inner")).unwrap();
        std::fs::write(real.join("inner").join("f.txt"), b"x").unwrap();
        mkjunction(&tmp.path().join("linked"), &real);
        let through = tmp.path().join("linked").join("inner");
        let roots = triage_roots_away(tmp.path());
        let rej = check_admission(&through, &tmp.path().join("tgt"), &roots, false);
        assert!(
            rej.contains(&"source_ancestor_reparse".to_string()),
            "{rej:?}"
        );
    }

    /// A backup file the app rewrote through the junction after the cutover
    /// legitimately differs from its backup copy; finish still accepts it.
    #[cfg(windows)]
    #[test]
    fn finish_refuses_a_newer_directory_in_place_of_a_file() {
        let tmp = crate::test_support::TempDir::new("migr-rewrite");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        std::fs::remove_file(tgt.join("a.txt")).unwrap();
        std::fs::create_dir(tgt.join("a.txt")).unwrap();
        let fin = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true);
        assert!(fin.is_err());
        assert_eq!(
            std::fs::read(tmp.path().join("src-app.bak").join("a.txt")).unwrap(),
            b"hello"
        );
        assert!(tgt.join("a.txt").is_dir());
    }

    /// A target file that vanished after the cutover is not landed: finish
    /// refuses, the backup survives, and rollback remains available.
    #[cfg(windows)]
    #[test]
    fn finish_refuses_a_file_missing_from_the_target_and_keeps_the_backup() {
        let tmp = crate::test_support::TempDir::new("migr-missing");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        std::fs::remove_file(tgt.join("sub").join("b.txt")).unwrap();
        let err = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true)
            .expect_err("a missing target file must refuse the finish");
        assert!(
            err.contains("sub\\b.txt"),
            "the missing file is named: {err}"
        );
        assert!(tmp.path().join("src-app.bak").join("a.txt").exists());
        let rb = rollback(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(
            !rb.ok && is_reparse(&src),
            "diverged copies must remain accessible: {rb:?}"
        );
        std::fs::copy(
            tmp.path().join("src-app.bak").join("sub").join("b.txt"),
            tgt.join("sub").join("b.txt"),
        )
        .unwrap();
        assert!(rollback(&src.to_string_lossy(), &tgt.to_string_lossy()).ok);
    }

    /// The junction must point at the stated target before the backup may be
    /// dropped; a redirected junction refuses the finish and keeps the backup.
    #[cfg(windows)]
    #[test]
    fn finish_refuses_when_the_junction_points_elsewhere() {
        let tmp = crate::test_support::TempDir::new("migr-redirect");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        let other = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::remove_dir(&src).unwrap();
        mkjunction(&src, &other);
        let err = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true).unwrap_err();
        assert!(err.contains("pointing elsewhere"), "{err}");
        assert!(
            tmp.path().join("src-app.bak").join("a.txt").exists(),
            "the backup is untouched"
        );
    }

    /// A database companion can hold unique data after the other copy changes.
    #[cfg(windows)]
    #[test]
    fn finish_preserves_a_missing_sqlite_companion() {
        let tmp = crate::test_support::TempDir::new("migr-companion");
        let src = tmp.path().join("dbapp");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("db.sqlite"), b"SQLite format 3\x00real db bytes").unwrap();
        std::fs::write(src.join("db.sqlite-wal"), b"wal bytes").unwrap();
        let tgt = tmp.path().join("dbtgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        std::fs::remove_file(tgt.join("db.sqlite-wal")).unwrap();
        let err = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true)
            .expect_err("a missing companion must block the finish");
        assert!(
            err.contains("db.sqlite-wal"),
            "the blocking file is named: {err}"
        );
        assert!(tmp.path().join("dbapp.bak").join("db.sqlite-wal").is_file());
    }

    /// A backup directory swapped for a reparse point is not this migration's
    /// copy of anything: finish and rollback both refuse it.
    #[cfg(windows)]
    #[test]
    fn destructive_phases_refuse_a_reparse_backup() {
        let tmp = crate::test_support::TempDir::new("migr-bakswap");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        // Swap the backup for a junction pointing at a decoy.
        let bak = tmp.path().join("src-app.bak");
        let decoy = tmp.path().join("decoy");
        std::fs::rename(&bak, &decoy).unwrap();
        mkjunction(&bak, &decoy);
        let fin = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true);
        assert!(fin.is_err(), "finish must refuse a reparse backup");
        let rb = rollback(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(!rb.ok, "rollback must refuse a reparse backup: {rb:?}");
        assert!(decoy.join("a.txt").exists(), "the decoy was never touched");
    }

    /// With a pending reboot (1641) every destructive entry fails closed —
    /// finish and rollback are no exceptions to that pairing.
    #[test]
    fn a_pending_reboot_blocks_finish_and_rollback() {
        crate::executor::reboot::reset_for_test();
        crate::executor::reboot::mark_pending("source\u{0}key\u{0}App");
        let err = finish(r"C:\no-such-app", r"D:\no-such-target", true).unwrap_err();
        assert_eq!(err, crate::executor::reboot::PENDING_MESSAGE);
        let rolled = rollback(r"C:\no-such-app", r"D:\no-such-target");
        assert!(!rolled.ok);
        assert_eq!(
            rolled.message.as_deref(),
            Some(crate::executor::reboot::PENDING_MESSAGE)
        );
        crate::executor::reboot::reset_for_test();
    }
}
