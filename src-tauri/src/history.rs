//! Local cleanup history jsonl.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, Write};
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

const HISTORY_MAX_BYTES: usize = 2 * 1024 * 1024;
const HISTORY_MAX_ROWS: usize = 2000;

#[cfg(test)]
thread_local! {
    static TEST_HISTORY_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct TestHistoryDirGuard;
#[cfg(test)]
impl Drop for TestHistoryDirGuard {
    fn drop(&mut self) {
        TEST_HISTORY_DIR.with(|d| *d.borrow_mut() = None);
    }
}

#[cfg(test)]
fn use_history_dir(dir: &Path) -> TestHistoryDirGuard {
    TEST_HISTORY_DIR.with(|d| *d.borrow_mut() = Some(dir.to_path_buf()));
    TestHistoryDirGuard
}

fn history_path() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = TEST_HISTORY_DIR.with(|d| d.borrow().clone()) {
        return dir.join("Remova").join("history.jsonl");
    }
    #[cfg(windows)]
    if let Some(pd) = crate::sysops::known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramData)
    {
        return pd.join("Remova").join("history.jsonl");
    }
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

fn history_line(raw: &[u8]) -> &[u8] {
    let line = raw.strip_suffix(b"\n").unwrap_or(raw);
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Pin the history file's ancestor chain and refuse a reparse point. Every
/// rewrite (append, compact, delete, clear) goes through this: a local user can
/// pre-plant a junction under the shared ProgramData tree, and an elevated
/// Remova would otherwise write through it. Caller must hold [`FILE_LOCK`].
fn pin_history_path(p: &Path) -> Result<Vec<crate::fsutil::DirPin>, String> {
    let parent = p.parent().ok_or("history path has no parent")?;
    let pins = crate::fsutil::create_dirs_pinned(parent)
        .map_err(|e| format!("could not pin the history directory: {e}"))?;
    if crate::fsutil::is_reparse_point(parent) || crate::fsutil::is_reparse_point(p) {
        return Err("reparse point in the history path".into());
    }
    // A hard link is another way to make this user-writable path name a
    // protected file; refuse it before an elevated append writes through it.
    if crate::fsutil::has_multiple_links(p) {
        return Err("history file has multiple hard links".into());
    }
    Ok(pins)
}

/// Re-apply the private ACL after a rewrite created a fresh inode: the atomic
/// staging file inherits the directory ACL, not the file's previous private ACL.
fn resecure_history(p: &Path) {
    if let Err(e) = crate::backup::secure_private_path(p) {
        eprintln!("cleanup history rewritten but not made private: {e}");
    }
}

fn history_requires_compaction(
    reader: &mut BufReader<fs::File>,
    incoming: usize,
) -> io::Result<bool> {
    let available = HISTORY_MAX_BYTES - incoming;
    if reader.get_ref().metadata()?.len() > available as u64 {
        return Ok(true);
    }
    let mut rows = 0;
    let mut bytes = 0;
    let mut complete = true;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(!complete);
        }
        rows += chunk.iter().filter(|&&byte| byte == b'\n').count();
        bytes += chunk.len();
        if rows >= HISTORY_MAX_ROWS || bytes > available {
            return Ok(true);
        }
        complete = chunk.last() == Some(&b'\n');
        let consumed = chunk.len();
        reader.consume(consumed);
    }
}

/// Publish the newest valid rows and the incoming row together. Limit both the
/// retained tail and the buffer for any single row, including malformed rows.
/// Caller must hold [`FILE_LOCK`].
fn compact_with_append(
    p: &Path,
    mut reader: BufReader<fs::File>,
    incoming: &str,
) -> io::Result<()> {
    let available = HISTORY_MAX_BYTES - (incoming.len() + 1);
    let mut ring: VecDeque<Vec<u8>> = VecDeque::new();
    let mut retained_bytes = 0;
    let mut raw = Vec::new();
    reader.rewind()?;
    loop {
        raw.clear();
        if reader
            .by_ref()
            .take((available + 1) as u64)
            .read_until(b'\n', &mut raw)?
            == 0
        {
            break;
        }
        if raw.len() > available {
            if raw.last() != Some(&b'\n') {
                reader.skip_until(b'\n')?;
            }
            continue;
        }
        if raw.last() != Some(&b'\n') {
            // A complete final JSON value may have no line terminator.
            raw.push(b'\n');
        }
        if raw.len() > available
            || !std::str::from_utf8(history_line(&raw))
                .is_ok_and(|line| serde_json::from_str::<HistoryEntry>(line).is_ok())
        {
            continue;
        }
        retained_bytes += raw.len();
        ring.push_back(std::mem::take(&mut raw));
        while ring.len() >= HISTORY_MAX_ROWS || retained_bytes > available {
            retained_bytes -= ring.pop_front().unwrap().len();
        }
    }
    drop(reader);
    let (tmp, out) = crate::fsutil::create_atomic_tmp(p)?;
    let result = (|| -> io::Result<()> {
        let mut w = BufWriter::new(out);
        for l in &ring {
            w.write_all(l)?;
        }
        writeln!(w, "{incoming}")?;
        w.into_inner()?.sync_all()?;
        fs::rename(&tmp, p)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    resecure_history(p);
    Ok(())
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
) -> Result<(), String> {
    if dry_run {
        return Ok(());
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
    let line =
        serde_json::to_string(&entry).map_err(|e| format!("cannot encode history row: {e}"))?;
    let incoming = line.len() + 1;
    if incoming > HISTORY_MAX_BYTES {
        return Err("history row exceeds the history size limit".into());
    }
    let p = history_path();
    let _g = lock_file();
    // The history file lives under the shared ProgramData tree. A local user can
    // pre-plant a junction there; writing through it from an elevated Remova
    // would append outside the intended directory. Pin the ancestor chain before
    // the reparse check so a swap after the check is refused.
    let _pins = pin_history_path(&p).map_err(|e| format!("history path refused: {e}"))?;
    #[cfg(windows)]
    let existing = crate::fsutil::open_local_regular_file(&p);
    #[cfg(not(windows))]
    let existing = fs::File::open(&p);
    match existing {
        Ok(file) => {
            let mut reader = BufReader::new(file);
            if history_requires_compaction(&mut reader, incoming)
                .map_err(|e| format!("could not inspect history: {e}"))?
            {
                return compact_with_append(&p, reader, &line)
                    .map_err(|e| format!("history compaction failed: {e}"));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("could not open history: {error}")),
    }
    let mut opts = fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the link itself, never its target; deny delete/rename while the
        // handle is open so the path cannot be swapped between write and ACL.
        // FILE_SHARE_READ|WRITE lets the ACL call open the same file for
        // WRITE_DAC; delete sharing stays off.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        opts.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        opts.share_mode(0x0000_0003); // FILE_SHARE_READ | FILE_SHARE_WRITE
    }
    let mut f = opts
        .open(&p)
        .map_err(|e| format!("could not open history: {e}"))?;
    writeln!(f, "{line}").map_err(|e| format!("could not write history row: {e}"))?;
    if let Err(e) = crate::backup::secure_private_path(&p) {
        eprintln!("cleanup history written but not made private: {e}");
    }
    Ok(())
}

/// Read the newest `limit` rows. A missing file is an empty history; an
/// unreadable or corrupt file is an error, so the UI never shows "no cleanup
/// history" for data it could not read.
pub fn load_checked(limit: usize) -> Result<Vec<HistoryEntry>, String> {
    let (entries, corrupt) = read_history(limit)?;
    if corrupt > 0 {
        return Err(format!("history read failed: {corrupt} unreadable row(s)"));
    }
    Ok(entries)
}

/// Lenient reader for internal callers that can work with the readable rows.
pub fn load(limit: usize) -> Vec<HistoryEntry> {
    read_history(limit)
        .map(|(entries, _)| entries)
        .unwrap_or_default()
}

fn read_history(limit: usize) -> Result<(Vec<HistoryEntry>, usize), String> {
    if limit == 0 {
        return Ok((vec![], 0));
    }
    let _g = lock_file();
    let p = history_path();
    let file = match fs::File::open(&p) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], 0)),
        Err(e) => return Err(format!("history read failed: {e}")),
    };
    // Keep only the newest rows and count each valid ID in file order, the
    // same way delete_by_ids does. Malformed bytes have no addressable ID.
    let mut reader = std::io::BufReader::new(file);
    let mut raw = Vec::new();
    let mut ring = VecDeque::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    loop {
        raw.clear();
        if reader
            .read_until(b'\n', &mut raw)
            .map_err(|e| format!("history read failed: {e}"))?
            == 0
        {
            break;
        }
        let line = history_line(&raw);
        if line.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        let row = std::str::from_utf8(line)
            .ok()
            .map(|text| (text.to_owned(), next_line_id(&mut seen, text)));
        if ring.len() == limit {
            ring.pop_front();
        }
        ring.push_back(row);
    }
    let mut out: Vec<HistoryEntry> = Vec::with_capacity(ring.len());
    let mut corrupt = 0usize;
    for row in ring.into_iter().rev() {
        let entry = row.and_then(|(line, id)| {
            let mut entry: HistoryEntry = serde_json::from_str(&line).ok()?;
            entry.id = id;
            Some(entry)
        });
        match entry {
            Some(entry) => out.push(entry),
            None => corrupt += 1,
        }
    }
    Ok((out, corrupt))
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
    let _pins = pin_history_path(&p).map_err(|e| format!("history rewrite refused: {e}"))?;
    let file = match fs::File::open(&p) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("history read failed: {e}")),
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
        let selected = std::str::from_utf8(history_line(&raw))
            .ok()
            .filter(|text| !text.bytes().all(|b| b.is_ascii_whitespace()))
            .is_some_and(|text| drop_ids.contains(&next_line_id(&mut seen, text)));
        if selected {
            removed += 1;
            continue;
        }
        // Keep untouched records byte-for-byte, including malformed rows and
        // their original LF/CRLF boundaries.
        if w.write_all(&raw).is_err() {
            drop(w);
            let _ = fs::remove_file(&tmp);
            return Err("history rewrite failed".into());
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
    // The rewrite produced a fresh file; restore the private ACL on it.
    resecure_history(&p);
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
    let _pins = pin_history_path(&p).map_err(|e| format!("history rewrite refused: {e}"))?;
    write_history_file(&p, "")?;
    resecure_history(&p);
    Ok(())
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
    use std::collections::HashMap;

    fn row(app_name: &str) -> String {
        serde_json::to_string(&super::HistoryEntry {
            id: String::new(),
            app_name: app_name.into(),
            deleted: 1,
            failed: 0,
            skipped: 0,
            delayed: 0,
            aborted: false,
            dry_run: false,
            backup_dir: String::new(),
            created_at: "1".into(),
        })
        .unwrap()
    }

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
        let dir = crate::test_support::TempDir::new("history-compact-temp");
        dir.subdir("Remova");
        let _dir = super::use_history_dir(dir.path());
        let path = super::history_path();
        let previous = row("Previous");
        // A valid unterminated last record forces the append to compact.
        std::fs::write(&path, &previous).unwrap();
        let tmp = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
        std::fs::write(&tmp, "old partial").unwrap();
        // Deny writes to the pre-existing file.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x4)
            .open(&tmp)
            .unwrap();
        super::append("New", 1, 0, 0, 0, false, false, "").unwrap();
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "old partial");
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .starts_with(&format!("{previous}\n")));
        assert_eq!(super::load_checked(10).unwrap().len(), 2);
        drop(held);
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

    #[test]
    fn compact_discards_malformed_and_oversized_rows_and_respects_byte_limit() {
        let dir = crate::test_support::TempDir::new("history-compact-bytes");
        dir.subdir("Remova");
        let _dir = super::use_history_dir(dir.path());
        let path = super::history_path();
        let older = row(&"A".repeat(super::HISTORY_MAX_BYTES / 2));
        let newest_name = "B".repeat(super::HISTORY_MAX_BYTES / 2);
        let newest = row(&newest_name);
        let mut bytes = vec![0xff, b'\n'];
        bytes.extend_from_slice(format!("{older}\n").as_bytes());
        bytes.extend_from_slice(b"\xff\n");
        bytes.extend(std::iter::repeat_n(b'x', super::HISTORY_MAX_BYTES + 1));
        bytes.extend_from_slice(format!("\n{newest}\r\n{{invalid}}\n").as_bytes());
        bytes.push(0xfe); // torn invalid UTF-8 tail without a terminator
        std::fs::write(&path, bytes).unwrap();

        super::append("New", 1, 0, 0, 0, false, false, "").unwrap();
        let retained = std::fs::read(&path).unwrap();
        assert!(retained.len() <= super::HISTORY_MAX_BYTES);
        assert!(retained.starts_with(format!("{newest}\r\n").as_bytes()));
        let rows = super::load_checked(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].app_name, "New");
        assert_eq!(rows[1].app_name, newest_name);
        assert!(super::append(
            &"C".repeat(super::HISTORY_MAX_BYTES),
            1,
            0,
            0,
            0,
            false,
            false,
            ""
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), retained);
    }

    #[test]
    fn compact_respects_row_limit_below_byte_limit() {
        let dir = crate::test_support::TempDir::new("history-compact-rows");
        dir.subdir("Remova");
        let _dir = super::use_history_dir(dir.path());
        let path = super::history_path();
        let mut previous = String::new();
        for i in 0..super::HISTORY_MAX_ROWS {
            previous.push_str(&row(&i.to_string()));
            previous.push('\n');
        }
        assert!(previous.len() < super::HISTORY_MAX_BYTES);
        std::fs::write(&path, previous).unwrap();

        super::append("New", 1, 0, 0, 0, false, false, "").unwrap();
        let rows = super::load_checked(super::HISTORY_MAX_ROWS + 1).unwrap();
        assert_eq!(rows.len(), super::HISTORY_MAX_ROWS);
        assert_eq!(rows[0].app_name, "New");
        assert_eq!(rows[1].app_name, (super::HISTORY_MAX_ROWS - 1).to_string());
        assert_eq!(rows.last().unwrap().app_name, "1");
    }

    #[cfg(windows)]
    #[test]
    fn compaction_failure_is_reported_and_keeps_prior_history() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = crate::test_support::TempDir::new("history-compact-refused");
        dir.subdir("Remova");
        let _dir = super::use_history_dir(dir.path());
        let path = super::history_path();
        let previous = row("Previous");
        std::fs::write(&path, &previous).unwrap();
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2) // block the final rename
            .open(&path)
            .unwrap();

        let error = super::append("New", 1, 0, 0, 0, false, false, "").unwrap_err();
        assert!(error.starts_with("history compaction failed:"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), previous);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    /// Disk round-trip under a temp PROGRAMDATA (does not touch the real history file).
    #[test]
    fn disk_append_load_delete_clear_roundtrip() {
        let tmp = crate::test_support::TempDir::new("hist");
        tmp.subdir("Remova");
        // a thread-local test root isolates this fixture without relying
        // on a process-wide environment override that production no longer reads.
        let _dir = super::use_history_dir(tmp.path());
        let p = super::history_path();
        assert!(p.starts_with(tmp.path()));

        super::append("A", 1, 0, 0, 0, false, false, r"C:\b").unwrap();
        super::append("A", 1, 0, 0, 0, false, false, r"C:\b").unwrap();
        super::append("B", 2, 0, 0, 0, false, false, r"C:\b").unwrap();
        let rows = super::load(10);
        assert_eq!(rows.len(), 3);
        assert_ne!(rows[0].id, rows[1].id, "identical rows need distinct ids");
        assert_eq!(rows[0].app_name, "B"); // newest first

        let removed = super::delete_by_ids(&[rows[2].id.clone()]).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(super::load(10).len(), 2);

        super::clear_all().unwrap();
        assert!(super::load(10).is_empty());
    }

    #[test]
    fn deletes_the_selected_occurrence_in_mixed_lf_and_crlf_history() {
        let tmp = crate::test_support::TempDir::new("history_mixed_endings");
        let _dir = super::use_history_dir(tmp.path());
        super::append("First", 1, 0, 0, 0, false, false, "").unwrap();
        let path = super::history_path();
        let sample = std::fs::read_to_string(&path).unwrap();
        let row = sample.strip_suffix('\n').unwrap();
        let unrelated = row.replace("First", "Other");
        std::fs::write(&path, format!("{row}\r\n{row}\n{unrelated}")).unwrap();

        let listed = super::load_checked(3).unwrap();
        assert_eq!(listed.len(), 3);
        assert!(listed[1].id.ends_with("-1"));
        assert!(listed[2].id.ends_with("-0"));
        assert_eq!(super::delete_by_ids(&[listed[2].id.clone()]).unwrap(), 1);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{row}\n{unrelated}")
        );
    }

    #[test]
    fn cannot_read_history_is_an_error_but_missing_history_removes_zero() {
        let tmp = crate::test_support::TempDir::new("history_read_error");
        let _dir = super::use_history_dir(tmp.path());
        let ids = vec!["H0000000000000000-0".into()];
        assert_eq!(super::delete_by_ids(&ids).unwrap(), 0);
        std::fs::create_dir(super::history_path()).unwrap();
        assert!(super::delete_by_ids(&ids).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn rewrite_refuses_a_junction_in_the_history_path() {
        let tmp = crate::test_support::TempDir::new("hist-junction");
        let target = tmp.subdir("target");
        let link = tmp.path().join("Remova");
        let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success(), "junction fixture creation failed");
        assert!(crate::fsutil::is_reparse_point(&link));

        let _dir = super::use_history_dir(tmp.path());
        assert!(
            super::clear_all().is_err(),
            "clearing through a junction must be refused"
        );
        assert!(
            super::delete_by_ids(&["H0000000000000000-0".into()]).is_err(),
            "deleting through a junction must be refused"
        );
        let _ = std::fs::remove_dir(&link);
    }

    #[cfg(windows)]
    #[test]
    fn rewrite_refuses_a_hard_linked_history_file() {
        let tmp = crate::test_support::TempDir::new("hist-hardlink");
        tmp.subdir("Remova");
        let _dir = super::use_history_dir(tmp.path());
        let target = tmp.path().join("protected.jsonl");
        std::fs::write(&target, "protected\n").unwrap();
        let link = super::history_path();
        std::fs::hard_link(&target, &link).unwrap();
        assert!(
            super::clear_all().is_err(),
            "clearing a hard-linked history file must be refused"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "protected\n");
    }

    /// a torn non-UTF-8 row must survive delete rewrites untouched.
    #[test]
    fn delete_preserves_non_utf8_lines() {
        let tmp = crate::test_support::TempDir::new("hist_nu");
        tmp.subdir("Remova");
        let _dir = super::use_history_dir(tmp.path());
        let p = super::history_path();

        super::append("A", 1, 0, 0, 0, false, false, r"C:\b").unwrap();
        super::append("B", 2, 0, 0, 0, false, false, r"C:\b").unwrap();
        // Inject a torn non-UTF-8 line between the two good rows.
        let lines = std::fs::read(&p).unwrap();
        let split = lines.iter().position(|&b| b == b'\n').unwrap();
        let mut rebuilt = lines[..=split].to_vec();
        rebuilt.extend_from_slice(b"{\"app_name\":\"");
        rebuilt.extend_from_slice(&[0xFF, 0xFE, 0x80]);
        rebuilt.extend_from_slice(b"\"}\r\n");
        rebuilt.extend_from_slice(&lines[split + 1..]);
        std::fs::write(&p, &rebuilt).unwrap();

        // Load reports the unreadable row instead of pretending the history is
        // shorter than it is; the strict reader is the one the UI now uses.
        assert!(
            super::load_checked(10).is_err(),
            "an unreadable row must surface as an error"
        );
        let rows = super::load(10);
        assert_eq!(rows.len(), 2, "the lenient reader keeps the good rows");
        // Delete the oldest good row (newest-first → index 1) — opaque line remains.
        let removed = super::delete_by_ids(&[rows[1].id.clone()]).unwrap();
        assert_eq!(removed, 1);
        let out = std::fs::read(&p).unwrap();
        assert_eq!(out, rebuilt[split + 1..]);
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
    }
}
