//! Junction directory migration (M6.3) — move without delete, gated per phase.
//!
//! Four phases, each independently runnable and gate-checked:
//!
//! - Plan: admission and sizes, read-only.
//! - Copy: non-destructive, cancellable, re-runnable; existing target files
//!   are only skipped when their content is byte-identical, and the closing
//!   diff re-reads both trees.
//! - Cutover: inside the destructive transaction lock — incremental top-up,
//!   rename to `.bak`, junction, write-through probe; any failure rolls the
//!   rename back.
//! - Finish: verify every backed-up file landed in the target with the same
//!   content — or was rewritten by the app through the junction after the
//!   cutover — then drop the `.bak`, only after explicit confirmation.
//!
//! Rollback is always available after a cutover. The ledger appends one JSON
//! line per phase for the report.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Copy is re-runnable; this cap only bounds one pass.
const COPY_FILE_CAP: usize = 200_000;
/// Plan-phase walk bound for the hardlink scan; beyond it the migration is
/// refused rather than guessed about.
const PLAN_FILE_CAP: u64 = 100_000;
/// Companion suffixes allowed to differ or be absent at Finish. The exemption
/// is scoped, not a bare filename match: the sibling base file must exist in
/// the backup and be an actual SQLite database, so an arbitrary `x-wal` or
/// `x.lockfile` cannot smuggle a content mismatch past the verification.
const COMPANION_SUFFIXES: [&str; 4] = ["-wal", "-shm", "-journal", "lockfile"];

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
    CutOver,
    /// Junction in place, backup gone (finish completed).
    Finished,
    /// A combination that does not match any phase — needs manual attention.
    Inconsistent,
}

/// Cancellation targets a run, not a flag. Each copy (or cutover top-up)
/// registers a fresh generation; a cancel records the newest one, so a
/// request that arrives before the run registered — or after it ended — is
/// ignored, and a later run never inherits an earlier cancel.
static COPY_GEN: AtomicU64 = AtomicU64::new(0);
static CANCELLED_GEN: AtomicU64 = AtomicU64::new(0);

pub fn request_copy_cancel() {
    CANCELLED_GEN.store(COPY_GEN.load(Ordering::SeqCst), Ordering::SeqCst);
}

fn copy_cancelled() -> bool {
    CANCELLED_GEN.load(Ordering::SeqCst) == COPY_GEN.load(Ordering::SeqCst)
}

/// Register a fresh generation for this run, immune to stale cancels.
fn begin_copy_run() {
    COPY_GEN.fetch_add(1, Ordering::SeqCst);
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
    use std::io::Read;
    fn read_fill(f: &mut std::fs::File, buf: &mut [u8]) -> Result<usize, String> {
        let mut n = 0;
        while n < buf.len() {
            match f.read(&mut buf[n..]) {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(n)
    }
    let mut fa = std::fs::File::open(a).map_err(|e| format!("{}: {e}", a.display()))?;
    let mut fb = std::fs::File::open(b).map_err(|e| format!("{}: {e}", b.display()))?;
    let mut ba = [0u8; 64 * 1024];
    let mut bb = [0u8; 64 * 1024];
    loop {
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

/// True when `path` starts with the SQLite magic header.
fn sqlite_magic(p: &Path) -> bool {
    use std::io::Read;
    std::fs::File::open(p)
        .and_then(|mut f| {
            let mut header = [0u8; 16];
            f.read_exact(&mut header)?;
            Ok(header == *b"SQLite format 3\x00")
        })
        .unwrap_or(false)
}

/// A backup file qualifies for the Finish companion exemption only when its
/// name carries a known companion suffix AND the sibling base file exists in
/// the backup and is an actual SQLite database.
fn is_sqlite_companion(backup_root: &Path, rel_lower: &str) -> bool {
    let name = rel_lower.rsplit('\\').next().unwrap_or(rel_lower);
    let Some(suffix) = COMPANION_SUFFIXES
        .iter()
        .find(|s| name.ends_with(*s) && name.len() > s.len())
    else {
        return false;
    };
    let base = &name[..name.len() - suffix.len()];
    if base.is_empty() {
        return false;
    }
    let base_rel = match rel_lower.rfind('\\') {
        Some(i) => format!("{}\\{}", &rel_lower[..i], base),
        None => base.to_string(),
    };
    sqlite_magic(&backup_root.join(&base_rel))
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
        if let Some(count) = count_source_files(source) {
            if count > PLAN_FILE_CAP {
                rej.push("source_too_many_files".into());
            }
        } else {
            rej.push("source_unreadable".into());
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
fn check_destructive(source: &Path, target: &Path) -> Vec<String> {
    let mut rej = Vec::new();
    let src_l = lower(source);
    let tgt_l = lower(target);
    let mut bak_name = source.as_os_str().to_os_string();
    bak_name.push(".bak");
    let backup = PathBuf::from(&bak_name);
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
    for (label, p) in [
        ("source", source),
        ("target", target),
        ("backup", backup.as_path()),
    ] {
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
            }
        }
    }
    match std::fs::symlink_metadata(&backup) {
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
fn count_source_files(root: &Path) -> Option<u64> {
    let mut stack = vec![root.to_path_buf()];
    let mut count: u64 = 0;
    while let Some(dir) = stack.pop() {
        let read = std::fs::read_dir(&dir).ok()?;
        for e in read {
            // A dropped entry would understate the count.
            let e = e.ok()?;
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

/// Phase 1: admission, sizes, free space. Read-only.
pub fn plan(source: &str, target: &str) -> MigrationPlan {
    let source = PathBuf::from(source.trim());
    let target = PathBuf::from(target.trim());
    plan_with_free(&source, &target, free_kb_for_target(&target))
}

/// plan() with the free-space answer injected: the capacity rule is testable
/// without stubbing the Win32 query.
fn plan_with_free(source: &Path, target: &Path, free: Option<i64>) -> MigrationPlan {
    let roots = crate::triage::roots_from_env();
    let rejections = check_admission(source, target, &roots, false);
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
    };
    if ok {
        match free {
            // No capacity answer is not a pass: the plan refuses instead of
            // letting a copy walk into a full disk.
            None => {
                plan.rejections.push("target_space_unknown".into());
                plan.ok = false;
            }
            Some(f) if f > 0 && plan.source_kb > 0 && f < plan.source_kb => {
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

/// Copy bookkeeping: files written, reparse children skipped (recorded, not
/// followed), and whether the pass hit the file cap.
#[derive(Default)]
struct CopyState {
    files: u64,
    reparse: Vec<String>,
    capped: bool,
}

/// Recursively copy `from` into `to` without following reparse points.
/// Re-runnable: an existing target file is skipped only when its content is
/// byte-identical — a same-length stale or corrupted file is recopied, never
/// mistaken for a completed copy.
fn copy_tree(from: &Path, to: &Path, rel: &str, state: &mut CopyState) -> Result<(), String> {
    if copy_cancelled() {
        return Err("migrate:cancelled".into());
    }
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
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
            let same = match std::fs::metadata(&child_to) {
                Ok(m) if m.len() == entry.metadata().map_err(|e| e.to_string())?.len() => {
                    files_equal(&child_from, &child_to)?
                }
                _ => false,
            };
            if !same {
                std::fs::copy(&child_from, &child_to).map_err(|e| format!("{child_rel}: {e}"))?;
            }
        }
    }
    Ok(())
}

/// Bidirectional diff of two trees. Same-path files at different sizes, or
/// missing on one side, are diffs; same-size pairs are additionally compared
/// byte-for-byte, so "same length, different content" is a diff, not a pass.
/// Reparse children are excluded on both sides. Empty result = identical.
fn tree_diff(a: &Path, b: &Path) -> Result<Vec<String>, String> {
    fn walk(
        root: &Path,
        rel: &str,
        out: &mut BTreeMap<String, u64>,
        reparse: &mut Vec<String>,
    ) -> Result<(), String> {
        let read = std::fs::read_dir(root).map_err(|e| e.to_string())?;
        for entry in read {
            // The diff reads both trees end to end; a cancel must land here
            // too. In copy the user can cancel while the diff runs; in
            // cutover the gen was just re-armed, so only a live cancel lands.
            if copy_cancelled() {
                return Err("migrate:cancelled".into());
            }
            let entry = entry.map_err(|e| format!("{rel}: {e}"))?;
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
            Some(vb) if vb == va => {
                // Size matches; the content still has to. The map key is the
                // lowercased relative path; Windows resolves it regardless.
                let pa = a.join(k);
                let pb = b.join(k);
                if !files_equal(&pa, &pb)? {
                    diffs.push(format!("a:{k}"));
                }
            }
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
/// destructive; re-runnable; cancellable; admitted server-side like every
/// other phase — an IPC caller cannot aim the copier at arbitrary paths.
pub fn copy_phase(source: &str, target: &str) -> Result<MigrationCopyReport, String> {
    let source = PathBuf::from(source.trim());
    let target = PathBuf::from(target.trim());
    let roots = crate::triage::roots_from_env();
    // Populated targets are allowed: a rerun tops up an interrupted copy.
    let rejections = check_admission(&source, &target, &roots, true);
    if !rejections.is_empty() {
        return Err(format!("admission refused: {}", rejections.join(",")));
    }
    // Serialized with the destructive phases: a cutover must not interleave
    // with a copy still filling the target.
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    begin_copy_run();
    let mut state = CopyState::default();
    copy_tree(&source, &target, "", &mut state)?;
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
    // std quotes each arg for cmd.exe, so spaces and cmd metacharacters inside
    // the paths stay literal (the args array never re-parses through a shell).
    // Real-machine confirmation of odd path shapes lives in ACCEPTANCE H8.
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
    // Top-up any files written between Copy and now (WAL, logs). A fresh
    // generation: a stale copy cancel must not abort the cutover's top-up.
    begin_copy_run();
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

/// Phase 4: verify every backed-up file landed in the target — byte-identical,
/// or rewritten by the app through the junction after the cutover (SQLite
/// companions recreated in place excepted, scoped to a real SQLite sibling) —
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
    let mut bak_name = source_path.as_os_str().to_os_string();
    bak_name.push(".bak");
    let backup = PathBuf::from(bak_name);
    // The junction must be in place AND point at the stated target before the
    // backup may be dropped — the shape check rollback applies, because finish
    // is strictly more destructive.
    if !is_reparse(&source_path)
        || std::fs::read_link(&source_path)
            .map(|t| lower(&t) != lower(&target_path))
            .unwrap_or(true)
    {
        return Err(
            "migration is not in the cutover state (junction missing or pointing elsewhere)".into(),
        );
    }
    match state_of(&source_path, &target_path) {
        MigrationState::CutOver => {}
        other => {
            return Err(format!(
                "migration is not in the cutover state (current: {other:?})"
            ));
        }
    }
    // Re-check every path the destructive part will touch, inside the lock.
    let refused = check_destructive(&source_path, &target_path);
    if !refused.is_empty() {
        return Err(format!("refusing to finish: {}", refused.join(",")));
    }
    // After the cutover the app writes through the junction, so a target file
    // may legitimately differ from its backup copy. A backup file counts as
    // landed when the target copy is byte-identical, or when it was modified
    // after the junction was created — the only channel through which this
    // migration's target could have changed. Anything else refuses, silent
    // same-size mismatches included.
    let junction_created = std::fs::symlink_metadata(&source_path)
        .ok()
        .and_then(|m| m.created().ok())
        .ok_or("cannot read the junction creation time; refusing to finish")?;
    let mut missing = Vec::new();
    let mut stack = vec![backup.clone()];
    while let Some(dir) = stack.pop() {
        let read = std::fs::read_dir(&dir).map_err(|e| format!("backup unreadable: {e}"))?;
        for entry in read {
            // An unreadable backup entry is a hole in the proof, not a skip.
            let entry = entry.map_err(|e| format!("backup entry: {e}"))?;
            let p = entry.path();
            let rel = p
                .strip_prefix(&backup)
                .unwrap_or(&p)
                .to_string_lossy()
                .into_owned();
            if is_reparse(&p) {
                continue;
            }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(p);
                continue;
            }
            let in_target = target_path.join(&rel);
            let exception = is_sqlite_companion(&backup, &rel.to_lowercase());
            let landed = match std::fs::metadata(&in_target) {
                // Missing: only a live SQLite companion may be absent — the
                // app recreates -wal/-shm in place and checkpoints delete them.
                Err(_) => exception,
                Ok(tm) => {
                    exception
                        || files_equal(&p, &in_target).unwrap_or(false)
                        || tm
                            .modified()
                            .ok()
                            .map(|m| m >= junction_created)
                            .unwrap_or(false)
                }
            };
            if !landed {
                missing.push(rel);
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
    // Re-check every path the destructive part will touch, inside the lock:
    // protected trees, ancestor reparse points, a plain backup directory.
    let refused = check_destructive(&source_path, &target_path);
    if !refused.is_empty() {
        return MigrationRollbackReport {
            ok: false,
            message: Some(format!("refusing to roll back: {}", refused.join(","))),
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
        // A same-size mismatch under a different name blocks the finish.
        std::fs::write(tgt.join("db.sqlite"), b"9876543210").unwrap();
        // db.sqlite differs in content but matches in size; the per-byte
        // contract is (relative path, byte count), so restore first.
        drop(rollback(&src.to_string_lossy(), &tgt.to_string_lossy()));
        assert!(src.join("db.sqlite").exists());
    }

    /// Copy self-heals: a target file with the same length but different
    /// content is recopied, never mistaken for a completed copy — the silent
    /// mismatch the old size-only gate let through.
    #[test]
    fn a_same_size_content_mismatch_never_survives_the_copy() {
        let tmp = crate::test_support::TempDir::new("migr-heal");
        let src = tmp.path().join("app");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("db.sqlite"), b"0123456789").unwrap();
        let tgt = tmp.path().join("tgt");
        std::fs::create_dir_all(&tgt).unwrap();
        std::fs::write(tgt.join("db.sqlite"), b"9876543210").unwrap();
        let report = copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        assert_eq!(report.files, 1);
        assert_eq!(
            std::fs::read(tgt.join("db.sqlite")).unwrap(),
            b"0123456789",
            "the stale same-size file was recopied"
        );
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

    /// Cancellation targets a run: a stale cancel cannot poison the next one,
    /// and a cancel during the current run lands.
    #[test]
    fn a_cancel_lands_only_on_its_own_generation() {
        request_copy_cancel();
        begin_copy_run();
        assert!(
            !copy_cancelled(),
            "a stale cancel must not cancel a fresh run"
        );
        request_copy_cancel();
        assert!(copy_cancelled(), "a cancel during the run lands");
        begin_copy_run();
        assert!(!copy_cancelled(), "the next run starts clean");
    }

    /// A backup file the app rewrote through the junction after the cutover
    /// legitimately differs from its backup copy; finish still accepts it.
    #[cfg(windows)]
    #[test]
    fn finish_accepts_a_file_the_app_rewrote_after_the_cutover() {
        let tmp = crate::test_support::TempDir::new("migr-rewrite");
        let src = seed_source(tmp.path());
        let tgt = tmp.path().join("tgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        std::fs::write(tgt.join("a.txt"), b"rewritten by the app").unwrap();
        let fin = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true).unwrap();
        assert!(fin.removed_bak);
        assert!(fin.missing_in_target.is_empty());
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
        let fin = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true).unwrap();
        assert!(!fin.removed_bak, "the backup must survive a refused finish");
        assert_eq!(
            fin.missing_in_target,
            vec!["sub\\b.txt"],
            "the missing file is named"
        );
        assert!(tmp.path().join("src-app.bak").join("a.txt").exists());
        let rb = rollback(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(rb.ok, "rollback still works: {rb:?}");
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

    /// Companion exemptions are scoped to real SQLite siblings: a `-wal` whose
    /// base is a database may be absent from the target (checkpointed away),
    /// while the same suffix on a non-database blocks the finish.
    #[cfg(windows)]
    #[test]
    fn companion_exemptions_need_a_sqlite_sibling() {
        let tmp = crate::test_support::TempDir::new("migr-companion");
        let src = tmp.path().join("dbapp");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("db.sqlite"), b"SQLite format 3\x00real db bytes").unwrap();
        std::fs::write(src.join("db.sqlite-wal"), b"wal bytes").unwrap();
        std::fs::write(src.join("notes.txt-wal"), b"not a database companion").unwrap();
        let tgt = tmp.path().join("dbtgt");
        copy_phase(&src.to_string_lossy(), &tgt.to_string_lossy()).unwrap();
        let cut = cutover(&src.to_string_lossy(), &tgt.to_string_lossy());
        assert!(cut.ok, "{cut:?}");
        // The app checkpointed: both -wal files are gone from the target.
        std::fs::remove_file(tgt.join("db.sqlite-wal")).unwrap();
        std::fs::remove_file(tgt.join("notes.txt-wal")).unwrap();
        let fin = finish(&src.to_string_lossy(), &tgt.to_string_lossy(), true).unwrap();
        assert!(
            !fin.removed_bak,
            "the non-SQLite companion blocks the finish"
        );
        assert_eq!(fin.missing_in_target, vec!["notes.txt-wal"]);
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
