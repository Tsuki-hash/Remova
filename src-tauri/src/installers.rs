//! Installer / updater-cache scan (进阶) — packages and update blobs, not app leftover dirs.
//! Distinct from orphan leftovers: objects are installers and updater caches.

use crate::constants::{INSTALLER_PHASE_CAP, MIN_AMBIGUOUS_PACKAGE_BYTES, UPDATER_CACHE_LEAVES};
use crate::scan_allow::{self, AllowScope};
use crate::scanner::{
    fill_item_buckets, fill_item_sizes, CleanupItem, Confidence, Evidence, ItemKind, RiskLevel,
    SCORE_CONFIRMED, SCORE_SUSPECTED_MIN,
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
    let n = name.to_lowercase();
    // "uninstall.exe" contains "install" but is not a package.
    if n.contains("uninstall") {
        return false;
    }
    SETUP_NAME_MARKERS.iter().any(|m| n.contains(m))
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

/// Classify one Downloads entry. `None` means "not an installer candidate".
fn classify_download(name: &str, path: &Path, len: u64) -> Option<DownloadEvidence> {
    let lower = name.to_lowercase();
    if PKG_EXTS.iter().any(|ext| lower.ends_with(ext)) {
        return Some(DownloadEvidence::Package);
    }
    let marker = looks_like_setup_name(&lower);
    if lower.ends_with(".exe") {
        if marker {
            return Some(DownloadEvidence::Package);
        }
        // Portable tools and unattended installers are named after the app
        // (`python-3.12.1-amd64.exe`), so the PE header carries the evidence.
        if len >= MIN_AMBIGUOUS_PACKAGE_BYTES && is_pe_executable(path) {
            return Some(DownloadEvidence::MaybePackage);
        }
        return None;
    }
    if ARCHIVE_EXTS.iter().any(|ext| lower.ends_with(ext))
        && marker
        && len >= MIN_AMBIGUOUS_PACKAGE_BYTES
    {
        return Some(DownloadEvidence::MaybePackage);
    }
    None
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
    let mut evidence = vec![Evidence {
        code: "installer_package".into(),
        label: "installer".into(),
        weight: 40,
        detail: bucket.into(),
    }];
    if looks_like_setup_name(&p) {
        evidence.push(Evidence {
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
        evidence,
        shared: false,
        user_data: crate::safety::is_user_data_path(&p),
        user_library: crate::safety::is_user_library_path(&p)
            && !crate::safety::is_user_data_path(&p),
        size_kb: None,
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
        bucket: Some(bucket.into()),
    });
    scanned.insert(p);
}

fn scan_downloads(out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>) {
    if !crate::scan_task::checkpoint("files", out.len()) {
        return;
    }
    let Some(dir) = user_downloads() else { return };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    // Collect everything first: truncating during enumeration would make the
    // result depend on filesystem order.
    let mut candidates: Vec<(PathBuf, DownloadEvidence)> = Vec::new();
    for ent in rd.flatten() {
        if !crate::scan_task::checkpoint("files", candidates.len()) {
            return;
        }
        let p = ent.path();
        if !p.is_file() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        let len = ent.metadata().map(|m| m.len()).unwrap_or(0);
        if let Some(evidence) = classify_download(&name, &p, len) {
            candidates.push((p, evidence));
        }
    }
    candidates.sort_by_cached_key(|(p, _)| p.to_string_lossy().to_lowercase());
    candidates.truncate(INSTALLER_PHASE_CAP);
    for (path, evidence) in candidates {
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

fn scan_updater_dirs(out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>) {
    let Some(local) = user_local_appdata() else {
        return;
    };
    scan_updater_dirs_from(&local, out, scanned);
}

fn scan_updater_dirs_from(local: &Path, out: &mut Vec<CleanupItem>, scanned: &mut HashSet<String>) {
    if !crate::scan_task::checkpoint("caches", out.len()) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(local) else {
        return;
    };
    // Every first-level entry is examined, then the whole set is ordered and
    // truncated. The old `take(200)` dropped whatever sorted past 200, which on a
    // machine with 214 entries here missed a real updater directory.
    let mut candidates: Vec<PathBuf> = Vec::new();
    for vendor in rd.flatten() {
        if !crate::scan_task::checkpoint("caches", candidates.len()) {
            return;
        }
        let vpath = vendor.path();
        if !vpath.is_dir() {
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
    for cand in crate::fsutil::order_candidates(candidates)
        .into_iter()
        .take(INSTALLER_PHASE_CAP)
    {
        if !crate::scan_task::checkpoint("caches", out.len()) {
            return;
        }
        push_dir_item(out, scanned, &cand, "updater_cache");
    }
}

/// Scan installer packages + updater caches. Results feed the installer allow-list.
pub fn scan_installer_caches() -> Vec<CleanupItem> {
    let mut out = Vec::new();
    let mut scanned = HashSet::new();
    scan_downloads(&mut out, &mut scanned);
    scan_updater_dirs(&mut out, &mut scanned);
    if !crate::scan_task::checkpoint("sizes", out.len()) {
        return out;
    }
    fill_item_sizes(&mut out);
    if !crate::scan_task::checkpoint("finalizing", out.len()) {
        return out;
    }
    fill_item_buckets(&mut out, "");
    scan_allow::remember(AllowScope::Installer, &scanned);
    out
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
    /// interesting cases are all filename- or size-driven rather than
    /// machine-specific.
    #[test]
    fn classify_download_separates_packages_from_maybe_packages() {
        let root = std::env::temp_dir().join(format!(
            "remova-downloads-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");

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
        assert_eq!(
            classify_download("7z.msi", &root.join("7z.msi"), 0),
            Some(DownloadEvidence::Package)
        );
        assert_eq!(
            classify_download("App.Setup.MSI", &root.join("App.Setup.MSI"), 0),
            Some(DownloadEvidence::Package)
        );
        assert_eq!(
            classify_download("node-v20.msi", &root.join("node-v20.msi"), 0),
            Some(DownloadEvidence::Package)
        );
        // `.exe` with an installer name is a package even when small.
        assert_eq!(
            classify_download("App-Setup.exe", &root.join("App-Setup.exe"), 1024),
            Some(DownloadEvidence::Package)
        );
        // A real executable with no installer name: shown, never preselected.
        assert_eq!(
            classify_download("python-3.12.1-amd64.exe", &pe, len(&pe)),
            Some(DownloadEvidence::MaybePackage)
        );
        // An installer-named archive is a candidate, not a certainty.
        assert_eq!(
            classify_download("微信安装包.zip", &root.join("微信安装包.zip"), 4 << 20),
            Some(DownloadEvidence::MaybePackage)
        );
        // Personal files stay out entirely.
        for name in [
            "holiday-photos.zip",
            "invoice.pdf",
            "notes.txt",
            "uninstall.exe",
            "stub.exe",
            "invoice.exe",
        ] {
            let p = if name == "stub.exe" {
                tiny_pe.clone()
            } else if name == "invoice.exe" {
                fake.clone()
            } else {
                root.join(name)
            };
            assert_eq!(
                classify_download(name, &p, len(&p)),
                None,
                "{name} must not be an installer candidate"
            );
        }

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    /// Naming conventions differ per vendor, ecosystem and language, so the
    /// classifier is checked against realistic filenames from outside this
    /// machine. Every one of them must at least be *shown* — the previous
    /// name-keyword rule missed most of them.
    #[test]
    fn realistic_global_installer_names_are_all_shown() {
        let root = std::env::temp_dir().join(format!(
            "remova-downloads-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
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
            assert_eq!(
                classify_download(name, &pe, std::fs::metadata(&pe).unwrap().len()),
                Some(DownloadEvidence::MaybePackage),
                "{name}"
            );
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
                classify_download(name, &root.join(name), 4 << 20),
                Some(DownloadEvidence::Package),
                "{name}"
            );
        }
        // An installer-named archive is one tier lower: portable apps are
        // commonly shipped as zip, so the name alone does not prove the file is
        // disposable rather than the app itself.
        for name in ["WPS安裝程式.zip", "微信安装包.7z"] {
            assert_eq!(
                classify_download(name, &root.join(name), 4 << 20),
                Some(DownloadEvidence::MaybePackage),
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
                classify_download(name, &root.join(name), 4 << 20),
                None,
                "{name}"
            );
        }

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    /// The proven regression: a machine with more first-level entries than the
    /// old `take(200)` silently lost updater directories that sorted past it.
    #[test]
    fn updater_scan_examines_every_first_level_entry() {
        let root = std::env::temp_dir().join(format!(
            "remova-updater-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        // 214 filler directories, then the target — so the target sits far past
        // where a 200-entry cap would have stopped looking.
        for i in 0..214 {
            std::fs::create_dir(root.join(format!("filler-{i:03}"))).expect("create filler");
        }
        std::fs::create_dir(root.join("vxe-drive-updater")).expect("create target");

        let mut out = Vec::new();
        let mut scanned = HashSet::new();
        scan_updater_dirs_from(&root, &mut out, &mut scanned);

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

        std::fs::remove_dir_all(root).expect("remove fixture root");
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
        let root = std::env::temp_dir().join(format!(
            "remova-installer-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
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
        scan_updater_dirs_from(&root, &mut out, &mut scanned);

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

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }

    #[test]
    fn scan_runs_without_panic() {
        let _allow = crate::scan_allow::test_lock();
        let items = scan_installer_caches();
        // Two independent phases, each bounded on its own.
        assert!(items.len() <= INSTALLER_PHASE_CAP * 2);
    }
}
