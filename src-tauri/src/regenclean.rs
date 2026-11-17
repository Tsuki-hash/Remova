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

fn system_layouts() -> Result<Vec<(PathBuf, &'static str)>, String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
        use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_ProgramData};
        let mut buffer = vec![0u16; 32768];
        let len = unsafe { GetSystemWindowsDirectoryW(Some(&mut buffer)) } as usize;
        let unavailable = || "could not resolve Windows cache roots".to_string();
        if len == 0 || len >= buffer.len() {
            return Err(unavailable());
        }
        let windows = String::from_utf16(&buffer[..len]).map_err(|_| unavailable())?;
        if !crate::fsutil::is_local_drive_path(&windows) {
            return Err(unavailable());
        }
        Ok(fixed_layouts(&crate::triage::TriageRoots {
            windows: windows.into(),
            program_data: crate::sysops::known_folder(&FOLDERID_ProgramData)
                .ok_or_else(unavailable)?,
            local_app_data: crate::sysops::known_folder(&FOLDERID_LocalAppData)
                .ok_or_else(unavailable)?,
            ..Default::default()
        }))
    }
    #[cfg(not(windows))]
    Err("could not resolve Windows cache roots".into())
}

/// Only a complete process enumeration with no servicing process proves idle.
#[cfg(any(windows, test))]
fn servicing_busy_from(mut next: impl FnMut() -> Result<Option<String>, ()>) -> bool {
    for _ in 0..65_536 {
        match next() {
            Ok(Some(name)) if !name.is_empty() => {
                if ["TiWorker.exe", "dism.exe"]
                    .iter()
                    .any(|exe| name.eq_ignore_ascii_case(exe))
                {
                    return true;
                }
            }
            Ok(None) => return false,
            _ => return true,
        }
    }
    true
}

fn servicing_busy() -> bool {
    #[cfg(windows)]
    {
        use windows::core::HRESULT;
        use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES};
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let Ok(handle) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return true;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut first = true;
        let busy = servicing_busy_from(|| {
            let reading_first = std::mem::replace(&mut first, false);
            let result = unsafe {
                if reading_first {
                    Process32FirstW(handle, &mut entry)
                } else {
                    Process32NextW(handle, &mut entry)
                }
            };
            match result {
                Ok(()) => {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|unit| *unit == 0)
                        .ok_or(())?;
                    String::from_utf16(&entry.szExeFile[..len])
                        .map(Some)
                        .map_err(|_| ())
                }
                Err(error)
                    if !reading_first
                        && error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) =>
                {
                    Ok(None)
                }
                Err(_) => Err(()),
            }
        });
        let _ = unsafe { CloseHandle(handle) };
        busy
    }
    #[cfg(not(windows))]
    true
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
    matching_layout(path, layouts).map(|(_, reason)| *reason)
}

fn matching_layout<'a>(
    path: &str,
    layouts: &'a [(PathBuf, &'static str)],
) -> Option<&'a (PathBuf, &'static str)> {
    let p = path
        .trim()
        .to_lowercase()
        .trim_end_matches('\\')
        .to_string();
    layouts
        .iter()
        .find(|(root, _)| root.to_string_lossy().to_lowercase().trim_end_matches('\\') == p)
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
        let Ok(pins) = crate::fsutil::pin_local_scan_path(path) else {
            continue;
        };
        let Some(root) = pins.last() else {
            continue;
        };
        if !crate::fsutil::is_plain_pinned_directory(root) {
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

/// Check the entire request before deleting any contents, then keep the normal
/// local directory handles open through measurement and deletion.
fn delete_with(
    layouts: &[(PathBuf, &'static str)],
    servicing_blocked: impl Fn() -> bool,
    paths: &[String],
) -> Result<RegenerableCleanupReport, String> {
    let mut roots = std::collections::HashSet::new();
    let mut selected = Vec::new();
    for wanted in paths {
        let (path, reason) = matching_layout(wanted, layouts)
            .ok_or_else(|| format!("not a regenerable layout: {wanted}"))?;
        if !roots.insert(path) {
            continue;
        }
        let pins = match crate::fsutil::pin_local_scan_path(path) {
            Ok(pins)
                if pins
                    .last()
                    .is_some_and(crate::fsutil::is_plain_pinned_directory) =>
            {
                Some(pins)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            _ => {
                return Err(format!(
                    "regenerable layout root is redirected or not local: {}",
                    path.display()
                ))
            }
        };
        selected.push((path, *reason, pins));
    }
    let mut report = RegenerableCleanupReport::default();
    for (path, reason, pins) in selected {
        let display = path.to_string_lossy().into_owned();
        let Some(root) = pins.as_ref().and_then(|pins| pins.last()) else {
            report.skipped.push(RegenerableSkipped {
                path: display,
                message: "layout is no longer present".into(),
            });
            continue;
        };
        let (before, _) = measure(path);
        if reason == "cbs_temp" && servicing_blocked() {
            report.skipped.push(RegenerableSkipped {
                path: display,
                message: "servicing_busy".into(),
            });
            continue;
        }
        let result = crate::fsutil::remove_regenerable_contents(root, path);
        let (after, _) = measure(path);
        report.reclaimed_kb += (before - after).max(0);
        match result {
            Ok(()) => report.cleaned.push(display),
            Err(error) => report.skipped.push(RegenerableSkipped {
                path: display,
                message: format!("{reason}: partially cleaned, {error}"),
            }),
        }
    }
    Ok(report)
}

pub fn scan_regenerable() -> Result<RegenerableScanResult, String> {
    Ok(scan_with(&system_layouts()?, servicing_busy_cached()))
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
    #[cfg(test)]
    if let Some((layouts, busy)) = test_layouts::current() {
        return delete_with(&layouts, || busy, paths);
    }
    delete_with(&system_layouts()?, servicing_busy, paths)
}

/// Inject only directories created under an owned temporary root. The real
/// entry point and reboot gate still run; neither environment nor OS roots
/// are redirected, and an open gate can only delete disposable fixture data.
#[cfg(test)]
mod test_layouts {
    use super::*;
    type Layouts = (Vec<(PathBuf, &'static str)>, bool);
    thread_local! {
        static LAYOUTS: std::cell::RefCell<Option<Layouts>> = const { std::cell::RefCell::new(None) };
    }

    pub fn current() -> Option<Layouts> {
        LAYOUTS.with(|layouts| layouts.borrow().clone())
    }

    #[must_use = "keep the temporary layouts installed until cleanup returns"]
    pub struct Fixture<'a> {
        previous: Option<Layouts>,
        _root: &'a crate::test_support::TempDir,
        _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
    }

    impl<'a> Fixture<'a> {
        pub fn new(root: &'a crate::test_support::TempDir) -> Self {
            let layouts = vec![(root.subdir("Temp"), "win_temp")];
            Self {
                previous: LAYOUTS.with(|slot| slot.replace(Some((layouts, false)))),
                _root: root,
                _not_send: std::marker::PhantomData,
            }
        }
    }

    impl Drop for Fixture<'_> {
        fn drop(&mut self) {
            LAYOUTS.with(|slot| slot.replace(self.previous.take()));
        }
    }
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
        let report = delete_with(&layouts, || true, &[cbs.to_string_lossy().into_owned()]).unwrap();
        assert!(report.cleaned.is_empty());
        assert_eq!(report.skipped[0].message, "servicing_busy");
        assert!(cbs.join("pending.xml").exists(), "nothing was deleted");
    }

    #[test]
    fn servicing_requires_a_complete_process_scan() {
        let mut idle = [Ok(Some("notepad.exe".to_string())), Ok(None)].into_iter();
        assert!(!servicing_busy_from(|| idle.next().unwrap()));
        assert!(servicing_busy_from(|| Err(())));
        let mut incomplete = [Ok(Some("notepad.exe".to_string())), Err(())].into_iter();
        assert!(servicing_busy_from(|| incomplete.next().unwrap()));
        for running in ["TiWorker.exe", "DISM.EXE"] {
            assert!(servicing_busy_from(|| Ok(Some(running.to_string()))));
        }
    }

    #[cfg(windows)]
    #[test]
    fn environment_roots_do_not_authorize_cleaning_an_unrelated_directory() {
        let fixture = crate::test_support::TempDir::new("regenclean-env-roots");
        let temp = fixture.subdir("Temp");
        let marker = temp.join("keep.txt");
        std::fs::write(&marker, b"keep").unwrap();
        let expected = system_layouts().unwrap();
        let _lock = crate::test_support::EnvLock::new();
        let _env = crate::test_support::EnvVar::set("LOCALAPPDATA", fixture.path());
        assert_eq!(system_layouts().unwrap(), expected);
        let result = delete_regenerable(&[temp.to_string_lossy().into_owned()]);
        assert!(result.unwrap_err().starts_with("not a regenerable layout"));
        assert_eq!(std::fs::read(marker).unwrap(), b"keep");
    }

    #[cfg(windows)]
    #[test]
    fn the_admitted_cache_root_cannot_be_replaced_while_servicing_is_checked() {
        let fixture = crate::test_support::TempDir::new("regenclean-pinned-root");
        let cbs = fixture.subdir("CbsTemp");
        let unrelated = fixture.subdir("unrelated");
        let marker = unrelated.join("keep.txt");
        let cached = cbs.join("generated.txt");
        std::fs::write(&marker, b"preserve").unwrap();
        std::fs::write(&cached, b"cache").unwrap();
        let layouts = vec![(cbs.clone(), "cbs_temp")];
        let report = delete_with(
            &layouts,
            || {
                assert!(std::fs::rename(&cbs, fixture.join("moved")).is_err());
                false
            },
            &[cbs.to_string_lossy().into_owned()],
        )
        .unwrap();
        assert_eq!(report.cleaned.len(), 1);
        assert!(!cached.exists());
        assert_eq!(std::fs::read(marker).unwrap(), b"preserve");
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
            delete_with(&layouts, || false, &[layout.to_string_lossy().into_owned()]).unwrap();
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
            delete_with(&layouts, || false, &[layout.to_string_lossy().into_owned()]).unwrap();
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
    fn a_redirected_layout_root_is_refused_before_any_delete() {
        let tmp = crate::test_support::TempDir::new("regenclean-redirected");
        let outside = tmp.subdir("outside");
        let payload = outside.join("user-data.bin");
        std::fs::write(&payload, b"must survive").unwrap();
        // The layout name itself is a junction pointing at unrelated data.
        let layout = tmp.path().join("Temp");
        #[cfg(windows)]
        {
            assert!(
                std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
                    .args(["/c", "mklink", "/J"])
                    .arg(&layout)
                    .arg(&outside)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let layouts = vec![(layout.clone(), "win_temp")];
        let err =
            delete_with(&layouts, || false, &[layout.to_string_lossy().into_owned()]).unwrap_err();
        assert!(
            err.contains("redirected or not local"),
            "a redirected layout root must be refused: {err}"
        );
        assert_eq!(
            std::fs::read(&payload).unwrap(),
            b"must survive",
            "nothing outside the layout may be deleted"
        );
    }

    #[test]
    fn non_layout_paths_are_refused_before_any_delete() {
        let layouts = vec![(PathBuf::from(r"C:\W\Temp"), "win_temp")];
        let err =
            delete_with(&layouts, || false, &[r"C:\Windows\System32".to_string()]).unwrap_err();
        assert!(err.starts_with("not a regenerable layout"));
    }

    #[test]
    fn a_pending_reboot_blocks_regenerable_deletion() {
        struct ResetReboot;
        impl Drop for ResetReboot {
            fn drop(&mut self) {
                crate::executor::reboot::reset_for_test();
            }
        }
        crate::executor::reboot::reset_for_test();
        let _reboot = ResetReboot;
        let root = crate::test_support::TempDir::new("regenclean-pending");
        let _layouts = test_layouts::Fixture::new(&root);
        let layout = root.join("Temp");
        let payload = layout.join("must-survive.bin");
        let sibling = root.subdir("not-a-layout").join("keep.bin");
        std::fs::write(&payload, b"pending payload").unwrap();
        std::fs::write(&sibling, b"unrelated payload").unwrap();
        let paths = [layout.to_string_lossy().into_owned()];

        crate::executor::reboot::mark_pending("source\u{0}key\u{0}Temp");
        let outcome = delete_regenerable(&paths);
        // Check the effect before the error: a missing reboot gate stays inside
        // this temp root and makes the preservation assertion red.
        assert_eq!(std::fs::read(&payload).unwrap(), b"pending payload");
        assert_eq!(
            outcome.unwrap_err(),
            crate::executor::reboot::PENDING_MESSAGE
        );
        assert_eq!(std::fs::read(&sibling).unwrap(), b"unrelated payload");

        // Same admitted fixture through the real entry point with the gate open:
        // prove the rejected call was not merely blocked by layout admission.
        crate::executor::reboot::reset_for_test();
        let report = delete_regenerable(&paths).unwrap();
        assert_eq!(report.cleaned, paths);
        assert!(report.skipped.is_empty());
        assert!(layout.is_dir());
        assert!(!payload.exists());
        assert_eq!(std::fs::read(&sibling).unwrap(), b"unrelated payload");
    }
}
