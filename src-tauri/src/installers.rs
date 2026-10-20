//! Installer / updater-cache scan (进阶) — packages and update blobs, not app leftover dirs.
//! Distinct from orphan leftovers: objects are installers and updater caches.

use crate::constants::{INSTALLER_PHASE_CAP, MIN_AMBIGUOUS_PACKAGE_BYTES, UPDATER_CACHE_LEAVES};
use crate::scan_allow::{self, AllowScope};
use crate::scanner::{
    fill_item_buckets, fill_item_sizes, CleanupItem, Confidence, Evidence, ItemKind, RiskLevel,
    ScanGaps, ScopedScanResult, SCORE_CONFIRMED, SCORE_SUSPECTED_MIN,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Extensions that are installer packages on their own. These are only ever
/// installers, so they need neither a name marker nor a size floor.
const PKG_EXTS: &[&str] = &[".msi", ".msix", ".appx", ".appxbundle", ".msp"];

/// Archive extensions. Personal archives share these, so an archive is only a
/// package candidate when the name also says so.
const ARCHIVE_EXTS: &[&str] = &[".zip", ".7z", ".rar", ".tar.gz", ".tgz"];

/// Bilingual markers that a filename is an installer. Chinese software commonly
/// ships as `xxx安装包.exe`, which no English-only list would catch.
const SETUP_NAME_MARKERS: &[&str] = &[
    "setup", "install", "update", "redist", "vcredist", "dotnet", "jdk", "node-v", "webview2",
    "安装", "安裝",
];

/// The user's real Downloads folder.
///
/// Relocating Downloads to another drive is common, so the shell Known Folder
/// wins; `%USERPROFILE%\Downloads` is only the fallback when it cannot be read.
fn user_downloads() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(p) = known_folder_downloads() {
        return Some(p);
    }
    let profile = std::env::var_os("USERPROFILE")?;
    Some(PathBuf::from(profile).join("Downloads"))
}

#[cfg(windows)]
fn known_folder_downloads() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let raw = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
        let text = raw.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(raw.0.cast()));
        let text = text?;
        if text.is_empty() {
            return None;
        }
        Some(PathBuf::from(text))
    }
}

fn looks_like_setup_name(name: &str) -> bool {
    looks_like_setup_name_lower(&name.to_lowercase())
}

/// The same predicate on a string the caller has already lowercased.
///
/// The guess in `guess_download` lowercases every Downloads entry anyway, so
/// lowercasing again here allocated a second copy of every filename in the
/// folder — 50k entries is a lot of garbage for a predicate that only reads.
fn looks_like_setup_name_lower(lower: &str) -> bool {
    // "uninstall.exe" contains "install" but is not a package.
    if lower.contains("uninstall") {
        return false;
    }
    SETUP_NAME_MARKERS.iter().any(|m| lower.contains(m))
}

fn user_local_appdata() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local))
}

/// `MZ` plus a PE signature — the real test that a downloaded `.exe` is a
/// Windows executable, independent of its filename or the language it ships in.
/// A renamed document fails this, which a name-keyword check cannot tell.
fn is_pe_executable(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 2];
    if f.read_exact(&mut head).is_err() || head != *b"MZ" {
        return false;
    }
    // e_lfanew at 0x3C points at the PE signature.
    let mut offset = [0u8; 4];
    if f.seek(SeekFrom::Start(0x3C)).is_err() || f.read_exact(&mut offset).is_err() {
        return false;
    }
    let pe = u32::from_le_bytes(offset) as u64;
    if pe == 0 || pe > 0x0800_0000 {
        return false;
    }
    let mut signature = [0u8; 4];
    if f.seek(SeekFrom::Start(pe)).is_err() || f.read_exact(&mut signature).is_err() {
        return false;
    }
    signature == *b"PE\0\0"
}

/// How strongly a Downloads entry is evidence of an installer package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadEvidence {
    /// A package extension, or an `.exe` whose name says it is an installer.
    /// Preselected by the UI, like every other confirmed leftover.
    Package,
    /// A real executable or an installer-named archive. Shown, but never
    /// preselected: nothing proves the whole file is disposable.
    MaybePackage,
}

/// Cheap pre-classification: what the name and size alone can decide.
///
/// Deliberately does not open the file. The PE check costs a `File::open` plus
/// reads — with Defender's real-time scanning hooked on `CreateFileW` that is
/// milliseconds per file — and it used to run for every candidate before the
/// result was truncated, so a Downloads folder with a thousand executables paid
/// for ~940 checks it then threw away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadGuess {
    /// Settled by extension or name. No file access needed.
    Definite(DownloadEvidence),
    /// An `.exe` with no installer name: needs the PE header to decide.
    NeedsPeCheck,
    /// Not an installer candidate.
    Rejected,
}

fn guess_download(name: &str, len: u64) -> DownloadGuess {
    let lower = name.to_lowercase();
    // An uninstaller is the opposite of what this panel offers, and the name
    // says so — no need to open the file to find out.
    if lower.contains("uninstall") {
        return DownloadGuess::Rejected;
    }
    if PKG_EXTS.iter().any(|ext| lower.ends_with(ext)) {
        return DownloadGuess::Definite(DownloadEvidence::Package);
    }
    let marker = looks_like_setup_name_lower(&lower);
    if lower.ends_with(".exe") {
        if marker {
            return DownloadGuess::Definite(DownloadEvidence::Package);
        }
        // Portable tools and unattended installers are named after the app
        // (`python-3.12.1-amd64.exe`), so the PE header carries the evidence.
        if len >= MIN_AMBIGUOUS_PACKAGE_BYTES {
            return DownloadGuess::NeedsPeCheck;
        }
        return DownloadGuess::Rejected;
    }
    if ARCHIVE_EXTS.iter().any(|ext| lower.ends_with(ext))
        && looks_like_setup_name_lower(&lower)
        && len >= MIN_AMBIGUOUS_PACKAGE_BYTES
    {
        return DownloadGuess::Definite(DownloadEvidence::MaybePackage);
    }
    DownloadGuess::Rejected
}

fn push_file_item(
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    path: &Path,
    bucket: &str,
    evidence: DownloadEvidence,
) {
    // Non-UTF-8 never becomes a delete candidate (fail-closed).
    let Some(p) = crate::fsutil::path_utf8(path) else {
        return;
    };
    let confirmed = evidence == DownloadEvidence::Package;
    let risk = if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("msi"))
        .unwrap_or(false)
    {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };
    let mut evidence_list = vec![Evidence {
        code: "installer_package".into(),
        label: "installer".into(),
        weight: 40,
        detail: bucket.into(),
    }];
    // The file NAME carries the setup marker. Running this over the whole path
    // let a parent directory named `...uninstall...` silently drop the evidence
    // (and a parent named `...setup...` add it to a file that says nothing).
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(looks_like_setup_name)
    {
        evidence_list.push(Evidence {
            code: "installer_setup_name".into(),
            label: "setup-like".into(),
            weight: 10,
            detail: String::new(),
        });
    }
    out.push(CleanupItem {
        path: p.clone(),
        kind: ItemKind::File,
        score: if confirmed {
            SCORE_CONFIRMED
        } else {
            SCORE_SUSPECTED_MIN + 20
        },
        confidence: if confirmed {
            Confidence::Confirmed
        } else {
            Confidence::Suspected
        },
        risk,
        reason: if confirmed {
            "installer package".into()
        } else {
            "possible installer package".into()
        },
        evidence: evidence_list,
        shared: false,
        user_data: crate::safety::is_user_data_path(&p),
        // Never `user_library`: an installer package is the object of this scan,
        // not personal data that happens to live under a library folder. The
        // flag is also `\users\`-shaped, so a relocated Downloads folder behaved
        // differently from a default one — the same package was preselected on
        // one drive and never preselected on another.
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: Some(bucket.into()),
    });
    scanned.insert(p);
}

fn push_dir_item(
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    path: &Path,
    bucket: &str,
) {
    let Some(p) = crate::fsutil::path_utf8(path) else {
        return;
    };
    out.push(CleanupItem {
        path: p.clone(),
        kind: ItemKind::Dir,
        score: SCORE_SUSPECTED_MIN + 20,
        confidence: Confidence::Suspected,
        risk: RiskLevel::Medium,
        reason: "updater cache".into(),
        evidence: vec![Evidence {
            code: "updater_cache".into(),
            label: "updater".into(),
            weight: 35,
            detail: bucket.into(),
        }],
        shared: false,
        user_data: crate::safety::is_user_data_path(&p),
        user_library: crate::safety::is_user_library_path(&p)
            && !crate::safety::is_user_data_path(&p),
        size_kb: None,
        size_capped: false,
        bucket: Some(bucket.into()),
    });
    scanned.insert(p);
}

fn scan_downloads(
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    report: &mut ScanGaps,
) {
    let Some(dir) = user_downloads() else {
        report.degrade("downloads_root_missing");
        return;
    };
    scan_downloads_from(&dir, out, scanned, report);
}

fn scan_downloads_from(
    dir: &Path,
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    report: &mut ScanGaps,
) {
    if !crate::scan_task::checkpoint("files", out.len()) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        report.degrade(crate::scanner::unreadable_root_code(dir));
        return;
    };
    // Collect everything first: truncating during enumeration would make the
    // result depend on filesystem order.
    let mut definite: Vec<(PathBuf, DownloadEvidence)> = Vec::new();
    let mut ambiguous: Vec<PathBuf> = Vec::new();
    for ent in rd.flatten() {
        if !crate::scan_task::checkpoint("files", definite.len() + ambiguous.len()) {
            return;
        }
        // `file_type()` comes out of the `FindNextFileW` buffer, so the
        // directory test costs no extra syscall; `Path::is_file()` did a second
        // stat on top of the reparse check and the metadata read below.
        if !ent.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        // Junctions/mounts resolve elsewhere; listing them would offer a path
        // whose string location and real location disagree. The attribute is in
        // the metadata we already need for the size.
        let Ok(meta) = ent.metadata() else {
            continue;
        };
        if crate::dirsize::is_reparse_metadata(&meta) {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        let len = meta.len();
        match guess_download(&name, len) {
            DownloadGuess::Definite(evidence) => definite.push((ent.path(), evidence)),
            DownloadGuess::NeedsPeCheck => ambiguous.push(ent.path()),
            DownloadGuess::Rejected => {}
        }
    }
    definite.sort_by_cached_key(|(p, _)| p.to_string_lossy().to_lowercase());
    let definite_total = definite.len();
    definite.truncate(INSTALLER_PHASE_CAP);
    report.omit(definite_total.saturating_sub(INSTALLER_PHASE_CAP));
    // The expensive PE check runs only for what the definite group left room
    // for, and in sorted order, so the number of file opens is bounded by the
    // result cap rather than by the size of the Downloads folder.
    let mut examined = 0;
    if definite.len() < INSTALLER_PHASE_CAP {
        ambiguous.sort_by_cached_key(|p| p.to_string_lossy().to_lowercase());
        while definite.len() < INSTALLER_PHASE_CAP && examined < ambiguous.len() {
            if crate::scan_task::cancelled() {
                return;
            }
            let p = &ambiguous[examined];
            examined += 1;
            if is_pe_executable(p) {
                definite.push((p.clone(), DownloadEvidence::MaybePackage));
            }
        }
    }
    // Executables the PE budget never reached are NOT rows the list dropped —
    // most are not installers at all. Report them as a degraded phase instead
    // of inflating "N more not shown".
    let unchecked = ambiguous.len().saturating_sub(examined);
    if unchecked > 0 {
        report.degrade(format!(
            "{}:{unchecked}",
            crate::scanner::DEGRADED_PE_BUDGET_EXHAUSTED
        ));
    }
    for (path, evidence) in definite {
        if !crate::scan_task::checkpoint("files", out.len()) {
            return;
        }
        push_file_item(out, scanned, &path, "download_pkg", evidence);
    }
}

fn is_updater_dir_name(name: &str) -> bool {
    name.to_ascii_lowercase()
        .strip_suffix("-updater")
        .is_some_and(|prefix| !prefix.is_empty())
}

/// Drop candidates that sit under one already kept.
///
/// `*-updater` and the cache leaves inside it (`*-updater\updates`, …) are both
/// candidates, so the same bytes were reported twice. With both rows selected the
/// parent deletes first and the child then reports `skipped: path missing` — a
/// row the user cannot act on. `order_candidates` sorts an ancestor ahead of its
/// descendants, so one forward pass is enough.
fn drop_nested(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::with_capacity(candidates.len());
    for cand in candidates {
        let nested = kept
            .iter()
            .any(|ancestor| cand != *ancestor && cand.starts_with(ancestor));
        if !nested {
            kept.push(cand);
        }
    }
    kept
}

fn scan_updater_dirs(
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    report: &mut ScanGaps,
) {
    let Some(local) = user_local_appdata() else {
        report
            .degraded
            .push("LOCALAPPDATA is not set; updater caches under it were not scanned".into());
        return;
    };
    scan_updater_dirs_from(&local, out, scanned, report);
}

fn scan_updater_dirs_from(
    local: &Path,
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    report: &mut ScanGaps,
) {
    if !crate::scan_task::checkpoint("caches", out.len()) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(local) else {
        report
            .degraded
            .push(crate::scanner::unreadable_root_code(local));
        return;
    };
    // Every first-level entry is examined, then the whole set is ordered,
    // de-nested and truncated. The old `take(200)` dropped whatever sorted past
    // 200, which on a machine with 214 entries here missed a real updater
    // directory.
    let mut candidates: Vec<PathBuf> = Vec::new();
    for vendor in rd.flatten() {
        if !crate::scan_task::checkpoint("caches", candidates.len()) {
            return;
        }
        let vpath = vendor.path();
        // PERF-C: one buffered metadata read for the directory bit and the
        // reparse bit, instead of `is_dir()` (which follows links) + a stat.
        let Ok(meta) = vendor.metadata() else {
            continue;
        };
        if !meta.is_dir() || crate::dirsize::is_reparse_metadata(&meta) {
            continue;
        }
        if vendor.file_name().to_str().is_some_and(is_updater_dir_name) {
            candidates.push(vpath.clone());
        }
        for leaf in UPDATER_CACHE_LEAVES {
            if crate::scan_task::cancelled() {
                return;
            }
            let cand = vpath.join(leaf);
            if cand.is_dir() {
                candidates.push(cand);
            }
        }
    }
    // Order first, then drop the nested leaves, then truncate: a child dropped as
    // nested must not consume a slot the cap could have given a real candidate.
    let candidates = crate::fsutil::order_candidates(candidates);
    let candidates = drop_nested(candidates);
    report.omit(candidates.len().saturating_sub(INSTALLER_PHASE_CAP));
    for cand in candidates.into_iter().take(INSTALLER_PHASE_CAP) {
        if !crate::scan_task::checkpoint("caches", out.len()) {
            return;
        }
        push_dir_item(out, scanned, &cand, "updater_cache");
    }
}

/// Scan installer packages + updater caches. Results feed the installer allow-list.
///
/// Returns `Result` so the caller can tell a scan that ran from one that never
/// started; rows a phase cap dropped are reported as `omitted`, and roots that
/// could not be read as `degraded`.
pub fn scan_installer_caches() -> Result<ScopedScanResult, String> {
    let mut out = Vec::new();
    // Each phase owns its allow-list set: Downloads packages and updater
    // directories are independent result sets with independent caps, and sharing
    // one accumulator across them coupled the two phases for no benefit.
    let mut downloads_scanned = HashSet::new();
    let mut updater_scanned = HashSet::new();
    let mut report = ScanGaps::default();
    scan_downloads(&mut out, &mut downloads_scanned, &mut report);
    scan_updater_dirs(&mut out, &mut updater_scanned, &mut report);
    if !crate::scan_task::checkpoint("sizes", out.len()) {
        return Ok(ScopedScanResult::empty());
    }
    fill_item_sizes(&mut out);
    if !crate::scan_task::checkpoint("finalizing", out.len()) {
        return Ok(ScopedScanResult::empty());
    }
    fill_item_buckets(&mut out, "");
    let mut scanned = downloads_scanned;
    scanned.extend(updater_scanned);
    // Armed from exactly what is reported: a truncated or cancelled candidate is
    // never authorized.
    scan_allow::remember(AllowScope::Installer, &scanned);
    Ok(ScopedScanResult {
        items: out,
        omitted: report.omitted,
        degraded: report.degraded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_name_markers_are_bilingual_and_reject_uninstallers() {
        for name in [
            "App-Setup.exe",
            "vcredist_x64.exe",
            "node-v20.11.0-x64.msi",
            "python-3.12.1-amd64-installer.exe",
            "微信安装包.exe",
            "Chrome安裝程式.zip",
            "WebView2Runtime.exe",
        ] {
            assert!(looks_like_setup_name(&name.to_lowercase()), "{name}");
        }
        for name in [
            "uninstall.exe",
            "Uninstaller.exe",
            "report.pdf",
            "holiday-photos.zip",
            "notes.txt",
        ] {
            assert!(!looks_like_setup_name(&name.to_lowercase()), "{name}");
        }
    }

    /// Downloads classification is what the whole panel rests on, and the
    /// Downloads classification is what the whole panel rests on, and the
    /// interesting cases are all filename- or size-driven rather than
    /// machine-specific. The cheap guess decides everything except the PE check,
    /// which is asserted separately against a real header.
    #[test]
    fn guess_download_separates_packages_from_maybe_packages() {
        let root = crate::test_support::TempDir::new("remova-downloads");

        // A real PE header, so the classifier can recognise an executable that
        // carries no installer name at all.
        let pe = root.join("python-3.12.1-amd64.exe");
        let mut bytes = b"MZ".to_vec();
        bytes.extend_from_slice(&[0u8; 0x3A]);
        bytes.extend_from_slice(&0x40u32.to_le_bytes());
        bytes.extend_from_slice(b"PE\0\0");
        bytes.extend(std::iter::repeat(0u8).take(MIN_AMBIGUOUS_PACKAGE_BYTES as usize));
        std::fs::write(&pe, &bytes).expect("write PE fixture");
        // Same layout, but far too small to be an installer.
        let tiny_pe = root.join("stub.exe");
        std::fs::write(&tiny_pe, &bytes[..0x46]).expect("write tiny PE fixture");
        // A renamed document: `.exe` by name, not an executable.
        let fake = root.join("invoice.exe");
        std::fs::write(&fake, b"not a binary at all").expect("write fake exe");

        let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);

        // Package extensions need neither a marker nor a size floor — a small
        // `.msi` is still an installer.
        for name in ["7z.msi", "App.Setup.MSI", "node-v20.msi"] {
            assert_eq!(
                guess_download(name, 0),
                DownloadGuess::Definite(DownloadEvidence::Package),
                "{name}"
            );
        }
        // `.exe` with an installer name is a package even when small.
        assert_eq!(
            guess_download("App-Setup.exe", 1024),
            DownloadGuess::Definite(DownloadEvidence::Package)
        );
        // An installer-named archive is a candidate, not a certainty.
        assert_eq!(
            guess_download("微信安装包.zip", 4 << 20),
            DownloadGuess::Definite(DownloadEvidence::MaybePackage)
        );
        // A real executable with no installer name needs the PE header.
        assert_eq!(
            guess_download("python-3.12.1-amd64.exe", len(&pe)),
            DownloadGuess::NeedsPeCheck
        );
        assert!(is_pe_executable(&pe));
        // Personal files stay out entirely.
        for name in [
            "holiday-photos.zip",
            "invoice.pdf",
            "notes.txt",
            "uninstall.exe",
            "stub.exe",
            "invoice.exe",
        ] {
            assert_eq!(
                guess_download(name, len(&root.join(name))),
                DownloadGuess::Rejected,
                "{name} must not be an installer candidate"
            );
        }
        // A renamed document is not an executable at all.
        assert!(!is_pe_executable(&fake));
        // The undersized stub *does* carry a valid PE header; what rejects it is
        // the size floor in the guess, not the header check.
        assert!(is_pe_executable(&tiny_pe));
        assert_eq!(
            guess_download("stub.exe", len(&tiny_pe)),
            DownloadGuess::Rejected,
            "an undersized executable must not reach the PE tier"
        );
    }

    /// Naming conventions differ per vendor, ecosystem and language, so the
    /// classifier is checked against realistic filenames from outside this
    /// machine. Every one of them must at least be *shown* — the previous
    /// name-keyword rule missed most of them.
    #[test]
    fn realistic_global_installer_names_are_all_shown() {
        let root = crate::test_support::TempDir::new("remova-downloads-global");
        // A real PE header, so an app-named `.exe` is recognised without a marker.
        let pe = root.join("app-named.exe");
        let mut bytes = b"MZ".to_vec();
        bytes.extend_from_slice(&[0u8; 0x3A]);
        bytes.extend_from_slice(&0x40u32.to_le_bytes());
        bytes.extend_from_slice(b"PE\0\0");
        bytes.extend(std::iter::repeat(0u8).take(2 << 20));
        std::fs::write(&pe, &bytes).expect("write PE fixture");

        // Named after the app with no installer keyword — the PE header carries it.
        let app_named = [
            "python-3.12.1-amd64.exe",
            "Git-2.43.0-64-bit.exe",
            "obsidian-1.5.3.exe",
            "WPSOffice-11.1.0.11704.exe",
            "postgresql-16.1-1-windows-x64.exe",
            "android-studio-2023.1.1.26-windows.exe",
        ];
        for name in app_named {
            // Cheap guess defers these to the PE check, which the fixture header
            // satisfies.
            assert_eq!(
                guess_download(name, std::fs::metadata(&pe).unwrap().len()),
                DownloadGuess::NeedsPeCheck,
                "{name}"
            );
            assert!(is_pe_executable(&pe), "{name}");
        }
        // Marker-named or dedicated package formats, across languages.
        let packages = [
            "Docker Desktop Installer.exe",
            "VSCodeUserSetup-x64-1.85.1.exe",
            "ChromeSetup.exe",
            "Firefox Installer.exe",
            "jdk-17_windows-x64_bin.exe",
            "dotnet-sdk-8.0.100-win-x64.exe",
            "rust-1.75.0-x86_64-pc-windows-msvc.msi",
            "7z2401-x64.msi",
            "微信安装包.exe",
            "TeamViewer_Setup_x64.exe",
            "Notepad++-11.0-Installer.exe",
            "Node.js-20.11.0-x64.msi",
            "MicrosoftEdgeEnterpriseX64.msi",
        ];
        for name in packages {
            // These files are not written to disk; a plausible installer size is
            // what the archive tier needs on top of its name marker.
            assert_eq!(
                guess_download(name, 4 << 20),
                DownloadGuess::Definite(DownloadEvidence::Package),
                "{name}"
            );
        }
        // An installer-named archive is one tier lower: portable apps are
        // commonly shipped as zip, so the name alone does not prove the file is
        // disposable rather than the app itself.
        for name in ["WPS安裝程式.zip", "微信安装包.7z"] {
            assert_eq!(
                guess_download(name, 4 << 20),
                DownloadGuess::Definite(DownloadEvidence::MaybePackage),
                "{name}"
            );
        }
        // Still excluded: personal files and portable archives, whatever the language.
        for name in [
            "holiday-photos.zip",
            "简历.pdf",
            "portable-vlc.zip",
            "data-2024.7z",
            "uninstall.exe",
        ] {
            assert_eq!(
                guess_download(name, 4 << 20),
                DownloadGuess::Rejected,
                "{name}"
            );
        }
    }

    /// The PE check costs a file open per candidate. It must run only for what
    /// the definite group left room for, so a Downloads folder holding hundreds
    /// of app-named executables does not open all of them to report 60 rows.
    #[test]
    fn pe_check_runs_only_for_what_the_budget_allows() {
        let root = crate::test_support::TempDir::new("remova-pe-budget");
        // A valid PE header, reused for every app-named executable.
        let mut pe_bytes = b"MZ".to_vec();
        pe_bytes.extend_from_slice(&[0u8; 0x3A]);
        pe_bytes.extend_from_slice(&0x40u32.to_le_bytes());
        pe_bytes.extend_from_slice(b"PE\0\0");
        pe_bytes.extend(std::iter::repeat(0u8).take(2 << 20));
        // Far more app-named executables than the phase cap allows.
        let ambiguous_count = INSTALLER_PHASE_CAP * 3;
        for i in 0..ambiguous_count {
            std::fs::write(root.join(format!("app-{i:04}-1.0.0.exe")), &pe_bytes)
                .expect("write PE fixture");
        }
        // And enough definite packages to fill the budget on their own.
        for i in 0..INSTALLER_PHASE_CAP {
            std::fs::write(root.join(format!("setup-{i:04}.msi")), b"msi").expect("write msi");
        }

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_downloads_from(&root, &mut out, &mut scanned, &mut gaps);

        assert_eq!(out.len(), INSTALLER_PHASE_CAP);
        // Every reported row is a definite package; the executables never got a
        // PE check because the budget was already full.
        assert!(out.iter().all(|it| it.reason == "installer package"));
        assert!(out.iter().all(|it| it.path.contains("setup-")));
        assert!(scanned.len() == out.len());
        // Executables the budget never reached are a degraded phase, not rows
        // the list dropped — most of them are not installers at all.
        assert_eq!(
            gaps.omitted, 0,
            "unchecked executables are not dropped rows"
        );
        assert!(
            gaps.degraded.contains(&format!(
                "{}:{ambiguous_count}",
                crate::scanner::DEGRADED_PE_BUDGET_EXHAUSTED
            )),
            "{:?}",
            gaps.degraded
        );
    }

    /// CQ-2: the over-cap Downloads fixture used to write exactly
    /// `INSTALLER_PHASE_CAP` definite packages, so `definite_total - cap` was 0
    /// and deleting the `omitted` accumulation left the whole suite green. This
    /// fixture writes more definite packages than the cap and no ambiguous files
    /// at all, so the excess is visible only through `omitted`.
    #[test]
    fn downloads_over_the_cap_report_the_dropped_packages_as_omitted() {
        let root = crate::test_support::TempDir::new("downloads_over_cap");
        let definite_count = INSTALLER_PHASE_CAP + 7;
        for i in 0..definite_count {
            std::fs::write(root.join(format!("setup-{i:04}.msi")), b"msi").expect("write msi");
        }

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_downloads_from(root.path(), &mut out, &mut scanned, &mut gaps);

        assert_eq!(out.len(), INSTALLER_PHASE_CAP);
        assert_eq!(
            gaps.omitted as usize,
            definite_count - INSTALLER_PHASE_CAP,
            "every definite package the cap dropped must be counted"
        );
        // Ordering decides *which* packages are dropped, so the kept subset is
        // the alphabetically first cap-sized one.
        let paths: Vec<&str> = out.iter().map(|it| it.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_by_key(|p| p.to_lowercase());
        assert_eq!(paths, sorted, "packages must be reported in sorted order");
        assert!(paths.iter().any(|p| p.ends_with("setup-0000.msi")));
        let last = format!("setup-{:04}.msi", definite_count - 1);
        assert!(
            !paths.iter().any(|p| p.ends_with(&last)),
            "the last-sorting package is the one the cap drops"
        );
        assert_eq!(
            scanned.len(),
            out.len(),
            "only reported rows are authorized"
        );
    }

    /// The proven regression: a machine with more first-level entries than the
    /// old `take(200)` silently lost updater directories that sorted past it.
    #[test]
    fn updater_scan_examines_every_first_level_entry() {
        let root = crate::test_support::TempDir::new("remova-updater-order");
        // 214 filler directories, then the target — so the target sits far past
        // where a 200-entry cap would have stopped looking.
        for i in 0..214 {
            std::fs::create_dir(root.join(format!("filler-{i:03}"))).expect("create filler");
        }
        std::fs::create_dir(root.join("vxe-drive-updater")).expect("create target");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_updater_dirs_from(&root, &mut out, &mut scanned, &mut gaps);

        let paths: Vec<&str> = out.iter().map(|it| it.path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("vxe-drive-updater")),
            "an updater directory past a 200-entry cap must still be found"
        );
        // Ordered, so the subset chosen under the phase cap is reproducible.
        let mut sorted = paths.clone();
        sorted.sort_by_key(|p| p.to_lowercase());
        assert_eq!(paths, sorted);
        assert!(out.len() <= INSTALLER_PHASE_CAP);
        assert_eq!(
            gaps.omitted, 0,
            "214 fillers plus one target fits under the cap"
        );
    }

    /// `%LOCALAPPDATA%\Application Data` is a junction back to
    /// `%LOCALAPPDATA%`, so probing it for a `packages` leaf used to resolve to
    /// the real `%LOCALAPPDATA%\Packages` — the LocalState root of every Store
    /// app — and offer it for deletion. Creating a junction needs a privilege
    /// the test suite must not require, so the guard is asserted directly and
    /// the real-machine layout is covered by the read-only scan check below.
    #[test]
    fn reparse_entries_are_never_enumeration_candidates() {
        let root = crate::test_support::TempDir::new("remova-reparse");
        // The shape that matters: a leaf directory that is reachable both
        // directly and through an alias.
        std::fs::create_dir_all(root.join("Vendor").join("packages")).expect("create leaf");
        std::fs::create_dir_all(root.join("plain-updater")).expect("create updater");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_updater_dirs_from(&root, &mut out, &mut scanned, &mut gaps);

        // Ordinary directories are still found — the guard must not over-block.
        assert!(out.iter().any(|it| it.path.ends_with("plain-updater")));
        assert!(out.iter().any(|it| it.path.ends_with("packages")));
        // Every reported path must be a real path, not an alias of one.
        for it in &out {
            assert!(
                !crate::fsutil::is_reparse_point(std::path::Path::new(&it.path)),
                "reported path is a reparse point: {}",
                it.path
            );
        }
    }

    #[test]
    fn updater_dir_name_matches_known_suffix_only() {
        for name in [
            "perfectworldarena-updater",
            "@deepseek-aidsh-desktop-updater",
            "obsidian-UPDATER",
            "token-monitor-UpDaTeR",
        ] {
            assert!(is_updater_dir_name(name), "{name}");
        }
        for name in [
            "updater",
            "-updater",
            "ordinary-directory",
            "updater-backup",
            "product-updater-extra",
        ] {
            assert!(!is_updater_dir_name(name), "{name}");
        }
    }

    #[test]
    fn updater_scan_finds_suffix_roots_and_existing_cache_leaves() {
        let _allow = crate::scan_allow::test_lock();
        let root = crate::test_support::TempDir::new("remova-installer-scan");
        for name in [
            "perfectworldarena-updater",
            "@deepseek-aidsh-desktop-updater",
            "obsidian-UPDATER",
            "token-monitor-UpDaTeR",
        ] {
            let dir = root.join(name);
            std::fs::create_dir_all(dir.join("payload")).expect("create updater fixture");
            std::fs::write(dir.join("payload").join("package.nupkg"), b"fixture")
                .expect("write updater fixture");
        }
        let vendor = root.join("Application Data");
        for leaf in [
            "updater",
            "updates",
            "packages",
            "installcache",
            "squirreltemp",
        ] {
            std::fs::create_dir_all(vendor.join(leaf)).expect("create cache leaf");
        }
        std::fs::create_dir(root.join("ordinary-directory")).expect("create ordinary fixture");
        std::fs::write(root.join("token-monitor-updater.txt"), b"not a directory")
            .expect("create non-directory fixture");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_updater_dirs_from(&root, &mut out, &mut scanned, &mut gaps);

        let paths = out
            .iter()
            .map(|item| item.path.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(out.len(), 9);
        assert_eq!(paths.len(), out.len());
        for name in [
            "perfectworldarena-updater",
            "@deepseek-aidsh-desktop-updater",
            "obsidian-UPDATER",
            "token-monitor-UpDaTeR",
        ] {
            let path = crate::fsutil::path_utf8(&root.join(name)).expect("utf8 fixture path");
            let item = out
                .iter()
                .find(|item| item.path == path)
                .expect("suffix root");
            assert_eq!(item.kind, ItemKind::Dir);
            assert_eq!(item.bucket.as_deref(), Some("updater_cache"));
            assert_eq!(item.confidence, Confidence::Suspected);
            assert_eq!(item.risk, RiskLevel::Medium);
            assert_eq!(item.evidence[0].code, "updater_cache");
        }
        assert!(out.iter().all(|item| !item.path.ends_with("package.nupkg")));
        assert!(out
            .iter()
            .all(|item| !item.path.ends_with("ordinary-directory")));
    }

    /// Downloads packages and updater directories are independent result sets
    /// with independent caps, so a large Downloads folder cannot crowd updater
    /// directories out (or the reverse). The scan also reports up to twice the
    /// phase cap in total, never more.
    #[test]
    fn scan_stays_within_phase_caps() {
        let _allow = crate::scan_allow::test_lock();
        let result = scan_installer_caches().expect("installer scan");
        let items = &result.items;
        assert!(
            items.len() <= INSTALLER_PHASE_CAP * 2,
            "reported {} items, cap is {} per phase",
            items.len(),
            INSTALLER_PHASE_CAP
        );
        for bucket in ["download_pkg", "updater_cache"] {
            let reported = items
                .iter()
                .filter(|it| it.bucket.as_deref() == Some(bucket))
                .count();
            assert!(
                reported <= INSTALLER_PHASE_CAP,
                "{bucket} reported {reported} items, cap is {INSTALLER_PHASE_CAP}"
            );
        }
        // Every reported row is authorized; a truncated or cancelled candidate
        // never is.
        for it in items {
            assert!(crate::scan_allow::was_recent(
                AllowScope::Installer,
                &it.path
            ));
        }
    }

    /// A `*-updater` directory and the cache leaves inside it are both
    /// candidates, so the same bytes used to be reported twice. With both rows
    /// selected the parent deletes first and the child then reports
    /// `skipped: path missing` — a row the user can do nothing about. The nested
    /// leaf must be dropped, and dropped candidates must not be authorized.
    ///
    /// CQ-2: the fixture crosses the phase cap — exactly `cap` real candidates
    /// survive the de-nesting, and the five nested leaves sort into the first
    /// six pre-drop positions. If `drop_nested` ran after the truncation, those
    /// five dead leaves would consume cap slots: only 55 rows would be
    /// reported, the last real candidate would be cut, and 5 would be counted
    /// omitted. The old fixture had 11 candidates against a cap of 60, so it
    /// stayed green in that shape.
    #[test]
    fn nested_updater_leaves_are_not_reported_twice() {
        let root = crate::test_support::TempDir::new("nested");
        // The exact shape from the report: an updater directory that also holds
        // the recognized cache leaves. It sorts first, so its leaves land at the
        // very front of the pre-drop candidate list.
        let updater = root.join("aaa-updater");
        for leaf in UPDATER_CACHE_LEAVES {
            std::fs::create_dir_all(updater.join(leaf)).expect("create cache leaf");
        }
        // A vendor directory that is not an updater dir keeps its leaves: there
        // is no parent row to fold them into.
        let vendor = root.join("Application Data");
        for leaf in UPDATER_CACHE_LEAVES {
            std::fs::create_dir_all(vendor.join(leaf)).expect("create cache leaf");
        }
        // One-candidate vendors, enough that the de-nested set lands exactly on
        // the cap: 1 updater parent + 5 standalone leaves + fillers == cap.
        let filler_count = INSTALLER_PHASE_CAP - 1 - UPDATER_CACHE_LEAVES.len();
        for i in 0..filler_count {
            std::fs::create_dir_all(root.join(format!("filler-{i:03}")).join("updates"))
                .expect("create filler cache leaf");
        }

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_updater_dirs_from(&root, &mut out, &mut scanned, &mut gaps);

        let updater_path = crate::fsutil::path_utf8(&updater).expect("utf8 fixture path");
        assert!(
            out.iter().any(|it| it.path == updater_path),
            "the updater directory itself must be reported"
        );
        for leaf in UPDATER_CACHE_LEAVES {
            let nested = crate::fsutil::path_utf8(&updater.join(leaf)).expect("utf8 fixture path");
            assert!(
                !out.iter().any(|it| it.path == nested),
                "nested leaf {leaf} must not be reported alongside its parent"
            );
            assert!(
                !scanned.contains(&nested),
                "nested leaf {leaf} must not be authorized for deletion"
            );
            // The same leaf under a non-updater parent is still its own row.
            let standalone =
                crate::fsutil::path_utf8(&vendor.join(leaf)).expect("utf8 fixture path");
            assert!(
                out.iter().any(|it| it.path == standalone),
                "{leaf} under a plain vendor directory must still be reported"
            );
        }
        // Dropping first leaves exactly `cap` real candidates, so none may be
        // cut and none may be reported as omitted.
        assert_eq!(out.len(), INSTALLER_PHASE_CAP);
        assert_eq!(gaps.omitted, 0, "de-nesting must free the cap slots");
        // The last filler sorts after everything else; it survives only when the
        // nested leaves never consumed cap slots.
        let last = crate::fsutil::path_utf8(
            &root
                .join(format!("filler-{:03}", filler_count - 1))
                .join("updates"),
        )
        .expect("utf8 fixture path");
        assert!(
            out.iter().any(|it| it.path == last),
            "the nested leaves must not crowd a real candidate out of the cap"
        );
    }

    /// An installer package is what this panel offers, so it must be `Confirmed`
    /// and preselected wherever the user's Downloads folder actually lives.
    /// `is_user_library_path` keys on the literal `\users\` segment, so a
    /// relocated Downloads folder used to behave differently from a default one:
    /// the same `.msi` was never preselected on `C:\Users\...` and preselected on
    /// `D:\...`. The flag must not depend on the drive at all.
    #[test]
    fn installer_packages_are_preselected_whatever_drive_downloads_lives_on() {
        let root = crate::test_support::TempDir::new("remova-library");
        // Both shapes are synthetic: a default-profile Downloads and a
        // relocated one on another drive.
        let default_profile = root.join("users").join("someone").join("Downloads");
        let relocated = root.join("d-drive").join("Downloads");
        for dir in [&default_profile, &relocated] {
            std::fs::create_dir_all(dir).expect("create downloads fixture");
            std::fs::write(dir.join("App-Setup.msi"), b"msi").expect("write msi fixture");
            std::fs::write(dir.join("ChromeSetup.exe"), b"exe").expect("write exe fixture");
        }

        for dir in [&default_profile, &relocated] {
            let mut out = Vec::new();
            let mut scanned = HashSet::new();
            let mut gaps = ScanGaps::default();
            scan_downloads_from(dir, &mut out, &mut scanned, &mut gaps);

            assert_eq!(out.len(), 2, "both packages in {}", dir.display());
            for it in &out {
                assert!(
                    !it.user_library,
                    "{} must not be flagged as a user library",
                    it.path
                );
                assert!(!it.user_data, "{} is not a library root", it.path);
                assert_eq!(
                    it.confidence,
                    Confidence::Confirmed,
                    "{} must stay preselected",
                    it.path
                );
                assert_eq!(it.reason, "installer package");
            }
            assert_eq!(gaps.degraded.len(), 0);
        }
    }

    /// The setup-name evidence is read off the file name. Running it over the
    /// whole path let a parent directory named `...uninstall...` silently drop
    /// the evidence weight for a perfectly good package.
    #[test]
    fn setup_name_evidence_uses_the_file_name_not_the_parent_path() {
        let root = crate::test_support::TempDir::new("remova-parent");
        // A parent directory that contains "uninstall" and "setup".
        let downloads = root.join("uninstall").join("setup-tools");
        std::fs::create_dir_all(&downloads).expect("create downloads fixture");
        std::fs::write(downloads.join("App-Setup.msi"), b"msi").expect("write msi fixture");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_downloads_from(&downloads, &mut out, &mut scanned, &mut gaps);

        assert_eq!(out.len(), 1);
        assert!(
            out[0]
                .evidence
                .iter()
                .any(|e| e.code == "installer_setup_name"),
            "the file name says setup, so the evidence must be there: {:?}",
            out[0].evidence
        );
    }

    /// A root that exists but cannot be read must say so. An empty list with no
    /// `degraded` entry reads as "there is nothing here", which is how a
    /// permission-restricted user concludes the machine is clean.
    #[test]
    fn unreadable_scan_roots_are_reported_as_degraded() {
        let root = crate::test_support::TempDir::new("remova-degraded");
        let blocked = root.join("blocked");
        std::fs::write(&blocked, b"not a directory").expect("create blocked root");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        let mut gaps = ScanGaps::default();
        scan_downloads_from(&blocked, &mut out, &mut scanned, &mut gaps);
        assert!(out.is_empty());
        assert_eq!(gaps.degraded.len(), 1, "{:?}", gaps.degraded);
        assert!(gaps.degraded[0].starts_with("unreadable_root:"));

        scan_updater_dirs_from(&blocked, &mut out, &mut scanned, &mut gaps);
        assert!(out.is_empty());
        assert_eq!(gaps.degraded.len(), 2, "{:?}", gaps.degraded);
    }
}
