//! Remova core library (Tauri backend).

pub mod ai;
pub mod apps;
pub mod association;
pub mod backup;
pub mod bounded_cmd;
pub mod cliglobals;
pub mod cliresidue;
pub mod commands;
pub mod constants;
pub mod deep_reg;
pub mod dirsize;
pub mod diskradar;
pub mod error;
pub mod executor;
pub mod firewall;
mod folder_references;
pub mod fsutil;
pub mod history;
pub mod icon;
pub mod idle;
pub mod ignore;
pub mod installers;
pub mod installmon;
pub mod junction;
pub mod locks;
pub mod manage;
pub mod migrate;
pub mod msiconsistency;
pub mod npmglobals;
pub mod orphans;
pub mod path_seal;
pub mod policy;
pub mod portable;
pub mod regenclean;
pub mod regops;
pub mod regscan;
pub mod restore;
pub mod safety;
pub mod scan_allow;
pub mod scan_contract;
pub mod scan_task;
pub mod scanner;
pub mod shared;
pub mod shellassoc;
pub mod storeapps;
pub mod sysops;
pub mod toolcache;
pub mod toolchain_audit;
pub mod traces;
pub mod triage;
pub mod vectors;
pub mod winsxs;

// End-to-end smokes (temp dirs / path mock only) — kept in the module tree so
// `cargo test` actually compiles and runs them.
#[cfg(test)]
mod path_value_smoke;
#[cfg(test)]
mod pipeline_smoke;
// Opt-in performance baseline (every case #[ignore]d) — see the module docs.
#[cfg(test)]
mod perf_baseline;
// Command-layer wire tests: real JSON payloads through the real handlers.
#[cfg(test)]
mod command_wire_tests;
// Shared fixtures. Test-only, so it must never reach the shipped binary.
#[cfg(test)]
mod test_support;

use apps::InstalledApp;
use executor::{CleanupReport, FullCleanupOptions, FullCleanupReport};
use scanner::{CleanupItem, ScanResult};
use tauri::{Emitter, Manager};

#[tauri::command]
async fn list_installed_apps() -> Result<apps::InstalledAppsSnapshot, String> {
    tauri::async_runtime::spawn_blocking(apps::scan_installed_apps_snapshot)
        .await
        .map_err(|e| e.to_string())
}

/// Clear cancel flag before a new estimate batch.
#[tauri::command]
fn begin_size_estimate() {
    dirsize::clear_cancel();
}

/// Estimate on-disk size of an install location (KB).
/// Runs on the blocking pool so large trees do not freeze the webview.
/// Result is zeroed if the estimate batch was cancelled / superseded.
/// `capped` marks a file-cap partial (floor, not total).
#[derive(serde::Serialize)]
struct SizeEstimate {
    kb: i64,
    capped: bool,
}

#[tauri::command]
async fn estimate_dir_size_kb(path: String) -> Result<SizeEstimate, String> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Ok(SizeEstimate {
            kb: 0,
            capped: false,
        });
    }
    let gen = dirsize::current_batch();
    tauri::async_runtime::spawn_blocking(move || {
        // Honors the global cancel flag (batch cancel must zero in-flight walks).
        if !crate::fsutil::is_local_drive_path(&path) {
            return SizeEstimate {
                kb: 0,
                capped: false,
            };
        }
        let (kb, capped) = dirsize::walk_size_kb_capped(std::path::Path::new(&path));
        if dirsize::batch_stale(gen) {
            SizeEstimate {
                kb: 0,
                capped: false,
            }
        } else {
            SizeEstimate { kb, capped }
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Cancel in-flight directory size walks (they return 0).
#[tauri::command]
fn cancel_size_estimate() {
    dirsize::request_cancel();
}

/// Strict URL shape for `open_path` (scheme + host, no control/quote/space).
/// plain `http://` only for loopback (local Ollama etc.); remote must be https.
fn is_safe_http_url(url: &str) -> bool {
    let is_https = url.starts_with("https://");
    let is_http = url.starts_with("http://");
    if !is_https && !is_http {
        return false;
    }
    if url
        .chars()
        .any(|c| c.is_control() || c == '"' || c == '\'' || c == ' ' || c == '\t')
    {
        return false;
    }
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or("");
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains(['@', '\\']) {
        return false;
    }
    // Bracketed IPv6 (`[::1]:8080`) — a plain colon split would truncate to "[".
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        match rest.split_once(']') {
            Some((h, tail)) => {
                if h.parse::<std::net::Ipv6Addr>().is_err() {
                    return false;
                }
                let port = if tail.is_empty() {
                    None
                } else {
                    match tail.strip_prefix(':') {
                        Some(p) => Some(p),
                        None => return false,
                    }
                };
                (format!("[{h}]"), port)
            }
            None => return false,
        }
    } else {
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(h, p)| (h, Some(p)));
        (host.to_string(), port)
    };
    if port.is_some_and(|p| {
        p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()) || p.parse::<u16>().is_err()
    }) {
        return false;
    }
    if host.is_empty()
        || !host.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || c == '.'
                || c == '-'
                || c == '['
                || c == ']'
                || (c == ':' && host.starts_with('[') && host.ends_with(']'))
        })
    {
        return false;
    }
    if is_http {
        // loopback only (IPv4 / localhost / [::1])
        let low = host.to_ascii_lowercase();
        return low == "localhost"
            || low == "[::1]"
            || low
                .parse::<std::net::Ipv4Addr>()
                .is_ok_and(|ip| ip.is_loopback());
    }
    true
}

/// Launch a validated http(s) URL via ShellExecuteW "open" (no shell metacharacter parsing).
#[cfg(windows)]
fn open_url_shell(url: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let url_w: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let empty: Vec<u16> = vec![0];
    unsafe {
        let rc = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(url_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR(empty.as_ptr()),
            SW_SHOWNORMAL,
        );
        if (rc.0 as isize) > 32 {
            Ok(())
        } else {
            Err("open_path:failed".into())
        }
    }
}

#[cfg(not(windows))]
fn open_url_shell(_url: &str) -> Result<(), String> {
    Err("open_path:failed".into())
}

/// Stable errors: `open_path:empty` | `open_path:not_found` | `open_path:failed`.
#[tauri::command]
fn open_path_in_explorer(path: String) -> Result<(), String> {
    let path = path.trim().trim_matches('"').trim();
    if path.is_empty() {
        return Err("open_path:empty".into());
    }
    // Windows Settings deep link: exact literal only — no caller-controlled
    // protocol string reaches ShellExecuteW.
    if path == "ms-settings:privacy" {
        return open_url_shell(path);
    }
    // HTTP(S): ShellExecuteW + strict URL shape (-01 — no `cmd /C start` injection surface).
    if path.starts_with("http://") || path.starts_with("https://") {
        if !is_safe_http_url(path) {
            return Err("open_path:failed".into());
        }
        return open_url_shell(path);
    }
    if !crate::fsutil::is_local_drive_path(path) {
        return Err("open_path:unsafe".into());
    }
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err("open_path:not_found".into());
    }
    use std::process::Command;
    let is_file = p.is_file();
    let explorer = regops::explorer_exe();
    if explorer.is_empty() {
        return Err("open_path:failed".into());
    }
    let mut cmd = Command::new(&explorer);
    if is_file {
        // Single argument form so Explorer selects the file in its parent.
        cmd.arg(format!("/select,{path}"));
    } else {
        cmd.arg(path);
    }
    regops::hide_console(&mut cmd);
    cmd.spawn().map(|_| ()).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "open_path:not_found".into(),
        _ => "open_path:failed".into(),
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelatedIcon {
    path: String,
    app_user_model_id: String,
}

/// Return an image data URL for an app or a related local item, or null.
#[tauri::command]
async fn app_icon_data(
    display_icon: Option<String>,
    embedded_only: Option<bool>,
    installed_app_key: Option<String>,
    related: Option<RelatedIcon>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        if let Some(key) = installed_app_key.as_deref() {
            let (mime, bytes) = icon::installed_icon(key)?;
            return Some(format!(
                "data:{mime};base64,{}",
                base64_light::b64_encode(&bytes)
            ));
        }
        #[cfg(windows)]
        if let Some(related) = related {
            let (mime, bytes) = icon::related_icon(
                display_icon.as_deref().unwrap_or_default(),
                &related.path,
                &related.app_user_model_id,
            )?;
            return Some(format!(
                "data:{mime};base64,{}",
                base64_light::b64_encode(&bytes)
            ));
        }
        #[cfg(not(windows))]
        let _ = (installed_app_key, related);
        let raw = display_icon?;
        if raw.trim().is_empty() || !crate::fsutil::is_local_drive_path(&raw) {
            return None;
        }
        let png = icon::extract_icon_png(&raw, embedded_only.unwrap_or(false))?;
        use base64_light::*;
        Some(format!("data:image/png;base64,{}", b64_encode(&png)))
    })
    .await
    .map_err(|e| e.to_string())
}

mod base64_light {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn b64_encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(ALPHABET[(n >> 6) as usize & 63] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(ALPHABET[n as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::b64_encode;

        #[test]
        fn encode_known() {
            assert_eq!(b64_encode(b"Man"), "TWFu");
            assert_eq!(b64_encode(b"Ma"), "TWE=");
            assert_eq!(b64_encode(b"M"), "TQ==");
        }
    }
}

#[cfg(test)]
mod open_path_url_tests {
    // Strict URL whitelist for open_path (no cmd /C start).
    #[test]
    fn http_url_whitelist() {
        assert!(super::is_safe_http_url(
            "https://github.com/Tsuki-hash/Remova/releases"
        ));
        assert!(super::is_safe_http_url("http://127.0.0.1:11434/v1"));
        assert!(super::is_safe_http_url("http://localhost:11434/v1"));
        assert!(super::is_safe_http_url("http://127.9.8.7:11434/v1"));
        assert!(super::is_safe_http_url("http://[::1]:11434/v1"));
        for url in [
            "http://127.0.0.1.evil.com",
            "http://127.999.0.1",
            "http://127.1",
            "http://128.0.0.1",
            "http://127.0.0.1@evil.com",
            "http://127.0.0.1:80@evil.com",
            "http://[::1]:80@evil.com",
            "http://[::1]evil.com",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:",
            "http://127.0.0.1:abc",
        ] {
            assert!(!super::is_safe_http_url(url), "must reject {url}");
        }
        // remote cleartext http is refused
        assert!(!super::is_safe_http_url("http://example.com/x"));
        assert!(!super::is_safe_http_url("https://"));
        assert!(!super::is_safe_http_url("ftp://example.com/x"));
        assert!(!super::is_safe_http_url("https://evil.com/\"&calc.exe"));
        assert!(!super::is_safe_http_url("https://evil.com/ path"));
        assert!(!super::is_safe_http_url("javascript:alert(1)"));
    }
}

#[cfg(test)]
mod cleanup_trust_tests {
    use super::*;
    use crate::policy::CleanupSource;
    use crate::scanner::{CleanupItem, Confidence, ItemKind, RiskLevel};

    fn item(path: &str) -> CleanupItem {
        CleanupItem {
            path: path.into(),
            kind: ItemKind::Dir,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "fixture".into(),
            evidence: vec![],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        }
    }

    fn app(name: &str, source: &str, install: &str) -> InstalledApp {
        InstalledApp {
            name: name.into(),
            version: "1".into(),
            publisher: "Vendor".into(),
            install_location: install.into(),
            uninstall_string: r"C:\Program Files\Demo\unins000.exe".into(),
            quiet_uninstall_string: String::new(),
            source: source.into(),
            registry_key: format!(
                r"{source}\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{name}"
            ),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        }
    }

    #[test]
    fn forged_cleanup_items_are_rejected_even_with_a_valid_app_identity() {
        let _lock = crate::scan_allow::test_lock();
        let scanned = app("TrustedDemo", "HKLM64", r"C:\Program Files\TrustedDemo");
        let key = apps::app_key(&scanned);
        scan_allow::remember_uninstall_items(&key, &[item(r"C:\Program Files\TrustedDemo")]);
        assert!(authorize_cleanup_items(
            &scanned,
            &[item(r"C:\Program Files\TrustedDemo")],
            CleanupSource::Uninstall
        )
        .is_ok());
        let mut tampered_kind = item(r"C:\Program Files\TrustedDemo");
        tampered_kind.kind = ItemKind::Path;
        assert!(
            authorize_cleanup_items(&scanned, &[tampered_kind], CleanupSource::Uninstall).is_err(),
            "the item kind must match the server analysis"
        );
        let mut tampered_bound = item(r"C:\Program Files\TrustedDemo");
        tampered_bound.service_bound = true;
        assert!(
            authorize_cleanup_items(&scanned, &[tampered_bound], CleanupSource::Uninstall).is_err(),
            "the service-bound flag must match the server analysis"
        );
        assert!(authorize_cleanup_items(
            &scanned,
            &[item(r"D:\Data\Victim")],
            CleanupSource::Uninstall
        )
        .is_err());
        assert!(
            authorize_cleanup_items(
                &scanned,
                &[item(r"C:\Program Files\TrustedDemo")],
                CleanupSource::Orphan
            )
            .is_err(),
            "an orphan request must match the server snapshot, not another scope"
        );
    }

    #[test]
    #[cfg(windows)]
    fn orphan_commands_recheck_owners_with_the_original_tree_held() {
        use crate::scan_contract::{ScanCompleteness, ScanStatus};
        use std::sync::{Arc, Mutex};

        #[derive(Default)]
        struct State {
            changed: bool,
            armed: bool,
            reads: usize,
            held_check: bool,
        }

        let _backup = crate::backup::lock_backup_env();
        let _allow = crate::scan_allow::test_lock();
        let fixture = crate::test_support::TempDir::new("orphan-live-owners");
        let _env = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", fixture.join("Backup"));
        let _seals = crate::path_seal::TestStore::new(fixture.join("seals"));
        let _history = crate::history::use_history_dir(fixture.path());
        let target = app("Orphan fixture", "Orphan", "");

        for backup in [false, true] {
            for case in [
                "HKLM64",
                "HKLM32",
                "HKCU",
                "parent",
                "child",
                "unnamed",
                "name-only",
                "partial",
                "failed",
                "cancelled",
            ] {
                for moment in ["before-preview", "after-preview", "held-tree"] {
                    let root = fixture.subdir(&format!("scan-{backup}-{case}-{moment}"));
                    let directory = root.join("CandidateOwnerFixture");
                    std::fs::create_dir(&directory).unwrap();
                    let old_file = directory.join("app.exe");
                    std::fs::write(&old_file, b"old residue").unwrap();
                    let original_id = crate::fsutil::local_directory_id(&directory).unwrap();
                    let _roots = orphans::use_test_roots(vec![root.clone()]);
                    let state = Arc::new(Mutex::new(State::default()));
                    let read_state = Arc::clone(&state);
                    let read_directory = directory.clone();
                    let _inventory = apps::inventory_test_support::use_source(move || {
                        let mut state = read_state.lock().unwrap();
                        if state.armed {
                            state.reads += 1;
                            if state.reads == 2 {
                                // First read is the plan. The second must run
                                // after the final source tree is already held.
                                assert!(std::fs::rename(
                                    &read_directory,
                                    read_directory.with_extension("moved")
                                )
                                .is_err());
                                state.held_check = true;
                                state.changed = true;
                            }
                        }
                        if !state.changed {
                            return (vec![], ScanCompleteness::complete());
                        }
                        let completeness = match case {
                            "partial" => ScanCompleteness::partial(3, vec!["HKLM32".into()]),
                            "failed" => ScanCompleteness::failed("HKCU".into()),
                            "cancelled" => ScanCompleteness::cancelled(1),
                            _ => ScanCompleteness::complete(),
                        };
                        if completeness.status != ScanStatus::Complete {
                            return (vec![], completeness);
                        }
                        let install = match case {
                            "parent" => read_directory.parent().unwrap().to_path_buf(),
                            "child" => read_directory.join("NewProduct"),
                            "name-only" => std::path::PathBuf::new(),
                            _ => read_directory.clone(),
                        };
                        let name = match case {
                            "unnamed" => "",
                            "name-only" => "CandidateOwnerFixture",
                            _ => "Replacement product",
                        };
                        let source = if matches!(case, "HKLM32" | "HKCU") {
                            case
                        } else {
                            "HKLM64"
                        };
                        (
                            vec![app(name, source, &install.to_string_lossy())],
                            completeness,
                        )
                    });
                    let scan =
                        tauri::async_runtime::block_on(scan_orphan_leftovers(None, Some(false)))
                            .unwrap();
                    assert!(scan.cleanup_allowed, "{backup}/{case}/{moment}");
                    assert_eq!(scan.items.len(), 1);
                    let options = FullCleanupOptions {
                        dry_run: false,
                        skip_official_uninstall: true,
                        backup_enabled: backup,
                        restore_point: false,
                        cleanup_source: Some("orphan".into()),
                        scope_revision: scan.scope_revision,
                        uninstall_review: None,
                    };
                    if moment == "before-preview" {
                        state.lock().unwrap().changed = true;
                    }
                    let preview = tauri::async_runtime::block_on(run_cleanup_dry_run(
                        target.clone(),
                        scan.items.clone(),
                        Some("orphan".into()),
                        scan.scope_revision,
                        None,
                    ))
                    .unwrap();
                    let refused = moment == "before-preview";
                    assert_eq!(preview.deleted_planned, u32::from(!refused));
                    assert_eq!(preview.skipped, u32::from(refused));
                    // The installer reuses the existing root, without renaming
                    // it, and adds data before confirmation/execution.
                    let new_file = directory.join("new-installation.bin");
                    std::fs::write(&new_file, b"new installed data").unwrap();
                    assert_eq!(
                        crate::fsutil::local_directory_id(&directory).unwrap(),
                        original_id
                    );
                    {
                        let mut state = state.lock().unwrap();
                        if moment == "held-tree" {
                            state.armed = true;
                            state.reads = 0;
                        } else {
                            state.changed = true;
                        }
                    }
                    let report = tauri::async_runtime::block_on(run_full_cleanup(
                        target.clone(),
                        scan.items,
                        options,
                        None,
                    ))
                    .unwrap();
                    assert_eq!(
                        (
                            report.deleted,
                            report.failed,
                            report.skipped,
                            report.delayed
                        ),
                        (0, 0, 1, 0),
                        "{backup}/{case}/{moment}: {report:?}"
                    );
                    let reason = if matches!(case, "partial" | "failed" | "cancelled") {
                        orphans::OWNER_UNVERIFIED
                    } else {
                        orphans::OWNER_CHANGED
                    };
                    assert_eq!(report.item_details[0].message, reason);
                    assert_eq!(std::fs::read(&old_file).unwrap(), b"old residue");
                    assert_eq!(std::fs::read(&new_file).unwrap(), b"new installed data");
                    assert_eq!(
                        crate::fsutil::local_directory_id(&directory).unwrap(),
                        original_id
                    );
                    if moment == "held-tree" {
                        assert!(state.lock().unwrap().held_check);
                        assert_eq!(report.backup_verified, backup);
                    }
                    let fresh =
                        tauri::async_runtime::block_on(scan_orphan_leftovers(None, Some(false)))
                            .unwrap();
                    if reason == orphans::OWNER_CHANGED {
                        assert!(
                            fresh.items.is_empty(),
                            "new owners are excluded on a fresh scan"
                        );
                    } else {
                        assert!(!fresh.cleanup_allowed);
                    }
                }
            }
        }
    }

    #[test]
    #[cfg(windows)]
    fn orphan_commands_keep_unowned_cleanup_and_accepted_cancellation() {
        use crate::scan_contract::ScanCompleteness;
        use std::sync::{Arc, Mutex};

        let _backup = crate::backup::lock_backup_env();
        let _allow = crate::scan_allow::test_lock();
        let fixture = crate::test_support::TempDir::new("orphan-live-positive");
        let _env = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", fixture.join("Backup"));
        let _seals = crate::path_seal::TestStore::new(fixture.join("seals"));
        let _history = crate::history::use_history_dir(fixture.path());
        let target = app("Orphan fixture", "Orphan", "");
        for backup in [false, true] {
            for cancel in [false, true] {
                let root = fixture.subdir(&format!("scan-{backup}-{cancel}"));
                let directory = root.join("UnownedCandidateFixture");
                std::fs::create_dir(&directory).unwrap();
                let sentinel = directory.join("app.exe");
                std::fs::write(&sentinel, b"unowned fixture").unwrap();
                let _roots = orphans::use_test_roots(vec![root]);
                let cancel_id = Arc::new(Mutex::new(None));
                let read_cancel_id = Arc::clone(&cancel_id);
                let _inventory = apps::inventory_test_support::use_source(move || {
                    if let Some(id) = read_cancel_id.lock().unwrap().take() {
                        assert!(executor::cleanup_request::cancel(id));
                    }
                    (vec![], ScanCompleteness::complete())
                });
                let scan = tauri::async_runtime::block_on(scan_orphan_leftovers(None, Some(false)))
                    .unwrap();
                assert_eq!(scan.items.len(), 1);
                let preview = tauri::async_runtime::block_on(run_cleanup_dry_run(
                    target.clone(),
                    scan.items.clone(),
                    Some("orphan".into()),
                    scan.scope_revision,
                    None,
                ))
                .unwrap();
                assert_eq!(preview.deleted_planned, 1);
                let operation = executor::cleanup_request::begin().unwrap();
                if cancel {
                    *cancel_id.lock().unwrap() = Some(operation);
                }
                let report = tauri::async_runtime::block_on(run_full_cleanup(
                    target.clone(),
                    scan.items,
                    FullCleanupOptions {
                        dry_run: false,
                        skip_official_uninstall: true,
                        backup_enabled: backup,
                        restore_point: false,
                        cleanup_source: Some("orphan".into()),
                        scope_revision: scan.scope_revision,
                        uninstall_review: None,
                    },
                    Some(operation),
                ))
                .unwrap();
                assert_eq!(report.failed, 0, "{report:?}");
                if cancel {
                    assert!(report.aborted, "{report:?}");
                    assert_eq!((report.deleted, report.delayed), (0, 0));
                    assert_eq!(std::fs::read(sentinel).unwrap(), b"unowned fixture");
                } else {
                    assert_eq!((report.deleted, report.skipped), (1, 0), "{report:?}");
                    assert!(!directory.exists());
                }
            }
        }
    }

    #[test]
    #[cfg(windows)]
    fn orphan_commands_preserve_replacements_and_require_the_latest_scan() {
        let _inventory = crate::apps::inventory_test_support::use_source(|| {
            (vec![], crate::scan_contract::ScanCompleteness::complete())
        });
        let _backup = crate::backup::lock_backup_env();
        let _allow = crate::scan_allow::test_lock();
        let fixture = crate::test_support::TempDir::new("orphan-ipc-identity");
        let _env = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", fixture.join("Backup"));
        let _seals = crate::path_seal::TestStore::new(fixture.join("seals"));
        let _history = crate::history::use_history_dir(fixture.path());
        let target = app("Orphan fixture", "Orphan", "");
        for backup in [false, true] {
            for directory in [false, true] {
                for replacement_directory in [false, true] {
                    let path = fixture.join(format!(
                        "candidate-{backup}-{directory}-{replacement_directory}"
                    ));
                    if directory {
                        std::fs::create_dir(&path).unwrap();
                    } else {
                        std::fs::write(&path, b"original").unwrap();
                    }
                    let candidate = CleanupItem {
                        kind: if directory {
                            ItemKind::Dir
                        } else {
                            ItemKind::File
                        },
                        ..item(&path.to_string_lossy())
                    };
                    let items = vec![candidate];
                    let revision = orphans::remember_orphan_items(&items);
                    let options = FullCleanupOptions {
                        dry_run: false,
                        skip_official_uninstall: true,
                        backup_enabled: backup,
                        restore_point: false,
                        cleanup_source: Some("orphan".into()),
                        scope_revision: revision,
                        uninstall_review: None,
                    };
                    let preview = tauri::async_runtime::block_on(run_cleanup_dry_run(
                        target.clone(),
                        items.clone(),
                        Some("orphan".into()),
                        revision,
                        None,
                    ))
                    .unwrap();
                    assert_eq!(preview.deleted_planned, 1);
                    std::fs::rename(&path, path.with_extension("original")).unwrap();
                    let sentinel = if replacement_directory {
                        std::fs::create_dir(&path).unwrap();
                        path.join("new-data")
                    } else {
                        path.clone()
                    };
                    std::fs::write(&sentinel, b"keep replacement").unwrap();
                    let preview = tauri::async_runtime::block_on(run_cleanup_dry_run(
                        target.clone(),
                        items.clone(),
                        Some("orphan".into()),
                        revision,
                        None,
                    ))
                    .unwrap();
                    assert_eq!((preview.deleted_planned, preview.skipped), (0, 1));
                    let report = tauri::async_runtime::block_on(run_full_cleanup(
                        target.clone(),
                        items.clone(),
                        options.clone(),
                        None,
                    ))
                    .unwrap();
                    assert_eq!((report.deleted, report.skipped), (0, 1), "{report:?}");
                    assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep replacement");
                    let current_item = CleanupItem {
                        kind: if replacement_directory {
                            ItemKind::Dir
                        } else {
                            ItemKind::File
                        },
                        ..items[0].clone()
                    };
                    let current = vec![current_item];
                    let fresh = orphans::remember_orphan_items(&current);
                    assert!(tauri::async_runtime::block_on(run_full_cleanup(
                        target.clone(),
                        items,
                        options.clone(),
                        None
                    ))
                    .is_err());
                    assert!(tauri::async_runtime::block_on(run_full_cleanup(
                        target.clone(),
                        current.clone(),
                        FullCleanupOptions {
                            scope_revision: None,
                            ..options.clone()
                        },
                        None
                    ))
                    .is_err());
                    let report = tauri::async_runtime::block_on(run_full_cleanup(
                        target.clone(),
                        current,
                        FullCleanupOptions {
                            scope_revision: fresh,
                            ..options
                        },
                        None,
                    ))
                    .unwrap();
                    assert_eq!((report.deleted, report.failed), (1, 0), "{report:?}");
                    assert!(!path.exists());
                }
            }
        }
    }

    #[test]
    fn orphan_ipc_rejects_cleared_guards_before_executor_dispatch() {
        let _lock = crate::scan_allow::test_lock();
        let app = app("Fixture", "orphan", "");
        let mut original = item("C:/Fixture/Orphan/Child");
        for service in [false, true] {
            original.shared = !service;
            original.service_bound = service;
            orphans::remember_orphan_items(&[original.clone()]);
            assert!(
                authorize_cleanup_items(&app, &[original.clone()], CleanupSource::Orphan).is_ok()
            );
            let mut forged = original.clone();
            forged.shared = false;
            forged.service_bound = false;
            assert!(authorize_cleanup_items(&app, &[forged], CleanupSource::Orphan).is_err());
        }
        let mut forged_kind = original.clone();
        forged_kind.kind = ItemKind::Registry;
        assert!(authorize_cleanup_items(&app, &[forged_kind], CleanupSource::Orphan).is_err());
        orphans::remember_orphan_items(&[]);
        assert!(authorize_cleanup_items(&app, &[original], CleanupSource::Orphan).is_err());
    }

    #[test]
    fn orphan_forgery_reaches_preview_and_full_gates_without_deleting_sentinel() {
        // TempDir and the safety/ignore readers share the process environment
        // with redirected-TEMP fixtures in other test threads.
        let _env = crate::test_support::EnvLock::new();
        let _inventory = crate::apps::inventory_test_support::use_source(|| {
            (vec![], crate::scan_contract::ScanCompleteness::complete())
        });
        let _lock = crate::scan_allow::test_lock();
        let root = crate::test_support::TempDir::new("orphan_guard");
        let directory = root.subdir("Candidate");
        let sentinel = directory.join("sentinel.bin");
        std::fs::write(&sentinel, b"unchanged fixture").unwrap();
        let app = app("Fixture", "orphan", "");
        let mut trusted = item(&directory.to_string_lossy());
        let options = FullCleanupOptions {
            dry_run: false,
            skip_official_uninstall: true,
            backup_enabled: false,
            restore_point: false,
            cleanup_source: Some("orphan".into()),
            scope_revision: None,
            uninstall_review: None,
        };
        for service in [false, true] {
            trusted.shared = !service;
            trusted.service_bound = service;
            orphans::remember_orphan_items(&[trusted.clone()]);
            let mut forged = trusted.clone();
            forged.shared = false;
            forged.service_bound = false;
            let preview = executor::run_cleanup_dry_for_app_source(
                &app,
                &[forged.clone()],
                CleanupSource::Orphan,
            );
            assert_eq!(preview.deleted_planned, 0);
            assert_eq!(preview.skipped, 1);
            assert_eq!(
                preview.item_details[0].message,
                "path not associated with app"
            );
            let report = executor::run_full_cleanup(&app, &[forged], &options);
            assert!(
                !report.aborted,
                "must reach the item gate, not an earlier abort"
            );
            assert_eq!(report.deleted, 0);
            assert_eq!(report.skipped, 1);
            assert_eq!(
                report.item_details[0].message,
                "path not associated with app"
            );
            assert!(report.backup_dir.is_empty());
            assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged fixture");
        }
        trusted.shared = false;
        trusted.service_bound = false;
        let revision = orphans::remember_orphan_items(&[trusted.clone()]);
        let preview = executor::run_cleanup_dry_for_app_source(
            &app,
            &[trusted.clone()],
            CleanupSource::Orphan,
        );
        assert_eq!(
            preview.deleted_planned, 1,
            "the ordinary fixture must be eligible"
        );
        let scan = crate::scan_allow::resolve_app_files(
            crate::scan_allow::ORPHAN_SCAN_KEY,
            revision,
            &[trusted.clone()],
        )
        .unwrap();
        let report = executor::run_full_cleanup_for_scan(
            &app,
            &[trusted],
            &FullCleanupOptions {
                scope_revision: revision,
                ..options
            },
            Some(&scan),
        );
        assert_eq!(
            report.deleted, 1,
            "ordinary authorized cleanup still works: {report:?}"
        );
        assert!(!directory.exists());
    }

    #[test]
    fn a_cleanup_without_analysis_is_refused() {
        let app = app("NeverAnalyzed", "HKLM64", r"C:\Program Files\NeverAnalyzed");
        assert!(authorize_cleanup_items(&app, &[], CleanupSource::Uninstall).is_err());
    }

    #[test]
    fn elevated_hkcu_analysis_and_cleanup_are_refused() {
        assert!(hkcu_refused_when_elevated("HKCU", true));
        assert!(hkcu_refused_when_elevated("hkcu", true));
        assert!(!hkcu_refused_when_elevated("HKLM64", true));
        assert!(!hkcu_refused_when_elevated("HKCU", false));
    }

    #[test]
    fn analysis_refusals_release_the_reserved_scan_task() {
        let _g = apps::trust_test_support::guard();
        let forged = app("ForgedDemo", "HKLM64", r"C:\Program Files\ForgedDemo");
        let id = scan_task::begin().expect("reserve a task");
        let err = resolve_analysis_app(forged, Some(id), false).unwrap_err();
        assert!(err.contains("latest scan"));
        assert!(
            scan_task::progress(id).is_none(),
            "a rejected analysis must not leak its scan reservation"
        );

        let scanned = app(
            "PerUserDemo",
            "HKCU",
            r"C:\Users\demo\AppData\Local\PerUserDemo",
        );
        apps::remember_uninstall_commands(std::slice::from_ref(&scanned));
        let id = scan_task::begin().expect("reserve a task");
        let err = resolve_analysis_app(scanned, Some(id), true).unwrap_err();
        assert!(err.contains("elevated HKCU"));
        assert!(
            scan_task::progress(id).is_none(),
            "an elevated-HKCU refusal must not leak its scan reservation"
        );
    }

    #[test]
    fn elevated_hkcu_cleanup_is_refused() {
        let _g = apps::trust_test_support::guard();
        let scanned = app(
            "PerUserCleanup",
            "HKCU",
            r"C:\Users\demo\AppData\Local\PerUserCleanup",
        );
        apps::remember_uninstall_commands(std::slice::from_ref(&scanned));
        let err = resolve_cleanup_app(scanned, CleanupSource::Uninstall, true).unwrap_err();
        assert!(err.contains("elevated HKCU cleanup"));
    }

    #[test]
    fn cleanup_resolution_uses_the_server_install_root() {
        let _g = apps::trust_test_support::guard();
        let scanned = app("ResolvedDemo", "HKLM64", r"C:\Program Files\ResolvedDemo");
        apps::remember_uninstall_commands(std::slice::from_ref(&scanned));
        let mut forged = scanned.clone();
        forged.install_location = r"D:\Data\Victim".into();
        let resolved =
            resolve_cleanup_app(forged, CleanupSource::Uninstall, false).expect("server identity");
        assert_eq!(resolved.install_location, r"C:\Program Files\ResolvedDemo");
    }
}

#[tauri::command]
fn begin_association_scan() -> Result<u32, String> {
    scan_task::begin()
}

#[tauri::command]
fn cancel_association_scan(scan_id: u32) -> Result<bool, String> {
    scan_task::cancel(scan_id)
}

#[tauri::command]
fn association_scan_progress(scan_id: u32) -> Option<scan_task::Progress> {
    scan_task::progress(scan_id)
}

fn cleanup_source_from_arg(source: Option<&str>) -> policy::CleanupSource {
    match source {
        Some("orphan") => policy::CleanupSource::Orphan,
        Some("monitor") => policy::CleanupSource::Monitor,
        Some("copilot") => policy::CleanupSource::Copilot,
        Some("installer") => policy::CleanupSource::Installer,
        Some("toolcache") => policy::CleanupSource::ToolCache,
        _ => policy::CleanupSource::Uninstall,
    }
}

/// App-scoped cleanup must resolve the client's identity to the last
/// server-side scan. Scoped scans (orphan/monitor/installer/toolcache) carry
/// their own server allow-lists and use a synthetic app.
fn resolve_cleanup_app(
    app: InstalledApp,
    source: policy::CleanupSource,
    elevated: bool,
) -> Result<InstalledApp, String> {
    if source.is_scoped_scan() {
        return Ok(app);
    }
    let app =
        apps::trusted_app(&app).ok_or_else(|| "app is not from the latest scan".to_string())?;
    if hkcu_refused_when_elevated(&app.source, elevated) {
        // Name-based associations can still point at machine directories, so
        // elevated deletion based on a user-writable HKCU record is refused
        // entirely. The user can run Remova without elevation to clean it.
        return Err("elevated HKCU cleanup refused".into());
    }
    Ok(app)
}

/// Release the scan task a client reserved before an analysis refusal.
///
/// The reservation is only removed by `scan_task::run` or `cancel`; a refusal
/// that returns before `run` would otherwise leave an unstarted task in the
/// process-wide table forever. Eight such refusals exhaust the task cap and
/// every scan entry point returns `scan:busy` until the app restarts.
fn release_scan_reservation(scan_id: Option<u32>) {
    if let Some(id) = scan_id {
        let _ = scan_task::cancel(id);
    }
}

fn resolve_analysis_app(
    app: InstalledApp,
    scan_id: Option<u32>,
    elevated: bool,
) -> Result<InstalledApp, String> {
    let Some(app) = apps::trusted_app(&app) else {
        release_scan_reservation(scan_id);
        return Err("app is not from the latest scan".into());
    };
    if hkcu_refused_when_elevated(&app.source, elevated) {
        release_scan_reservation(scan_id);
        return Err("elevated HKCU analysis refused".into());
    }
    Ok(app)
}

fn hkcu_refused_when_elevated(source: &str, elevated: bool) -> bool {
    elevated && source.to_ascii_uppercase().starts_with("HKCU")
}

/// Every app-scoped cleanup item must come from the latest server analysis of
/// that exact identity. This is the IPC boundary check; `policy` still applies
/// the path-level safety gates afterwards.
#[cfg(test)]
fn authorize_cleanup_items(
    app: &InstalledApp,
    items: &[CleanupItem],
    source: policy::CleanupSource,
) -> Result<(), String> {
    let key = if source == policy::CleanupSource::Orphan {
        scan_allow::ORPHAN_SCAN_KEY.to_owned()
    } else {
        executor::uninstall_review::analysis_key(app, None)
    };
    let revision = scan_allow::app_revision(&key);
    authorize_cleanup_items_for_review(app, items, source, None, revision).map(|_| ())
}

fn authorize_cleanup_items_for_review(
    app: &InstalledApp,
    items: &[CleanupItem],
    source: policy::CleanupSource,
    uninstall_review: Option<u64>,
    scope_revision: Option<u64>,
) -> Result<Option<scan_allow::AppFileSnapshot>, String> {
    if source == policy::CleanupSource::Orphan {
        return scan_allow::resolve_app_files(scan_allow::ORPHAN_SCAN_KEY, scope_revision, items)
            .map(Some);
    }
    if source.is_scoped_scan() {
        return Ok(None);
    }
    let key = executor::uninstall_review::analysis_key(app, uninstall_review);
    scan_allow::resolve_app_files(&key, scope_revision, items).map(Some)
}

#[tauri::command]
async fn analyze_associations(
    app: InstalledApp,
    scan_id: Option<u32>,
    uninstall_review: Option<u64>,
) -> Result<ScanResult, String> {
    // The client may rewrite any field; analyze only the server-scanned copy.
    // A refused request must release its reserved scan task.
    tauri::async_runtime::spawn_blocking(move || {
        let app = if let Some(receipt) = uninstall_review {
            match executor::uninstall_review::verify(&app, receipt) {
                Ok(app) => app,
                Err(error) => {
                    release_scan_reservation(scan_id);
                    return Err(error);
                }
            }
        } else {
            resolve_analysis_app(app, scan_id, crate::sysops::is_elevated_for_gate())?
        };
        let work = || {
            let mut result = scanner::analyze_associations(
                &app.name,
                &app.install_location,
                &app.publisher,
                &app.registry_key,
                &app.source,
            );
            if result.app_key.is_empty() {
                result.app_key = apps::app_key(&app);
            }
            let key = executor::uninstall_review::analysis_key(&app, uninstall_review);
            result.scope_revision = scan_allow::remember_uninstall_items(&key, &result.items);
            result
        };
        match scan_id {
            Some(id) => scan_task::run(id, work),
            None => Ok(work()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn run_cleanup_dry_run(
    app: InstalledApp,
    items: Vec<CleanupItem>,
    cleanup_source: Option<String>,
    scope_revision: Option<u64>,
    uninstall_review: Option<u64>,
) -> Result<CleanupReport, String> {
    let source = cleanup_source_from_arg(cleanup_source.as_deref());
    #[cfg(test)]
    let inventory = apps::inventory_test_support::current();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(test)]
        let _inventory = inventory.map(apps::inventory_test_support::Fixture::enter);
        let app = resolve_review_cleanup_app(app, source, uninstall_review)?;
        if let Some(id) = uninstall_review {
            executor::uninstall_review::verify(&app, id)?;
        }
        let scan = authorize_cleanup_items_for_review(
            &app,
            &items,
            source,
            uninstall_review,
            scope_revision,
        )?;
        let items = match source.specialty_scope() {
            Some(scope) => scan_allow::resolve_scoped_items(scope, scope_revision, &items)?,
            None => items,
        };
        Ok(executor::run_cleanup_dry_for_app_snapshot(
            &app,
            &items,
            source,
            scan.as_ref(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn resolve_review_cleanup_app(
    app: InstalledApp,
    source: policy::CleanupSource,
    receipt: Option<u64>,
) -> Result<InstalledApp, String> {
    match receipt {
        Some(id) if source == policy::CleanupSource::Uninstall => {
            executor::uninstall_review::resolve(&app, id)
        }
        Some(_) => Err("uninstall review expired".into()),
        None => resolve_cleanup_app(app, source, crate::sysops::is_elevated_for_gate()),
    }
}

fn validate_cleanup_intent(
    source: policy::CleanupSource,
    options: &FullCleanupOptions,
) -> Result<(), String> {
    let force = options.cleanup_source.as_deref() == Some("force");
    if options.uninstall_review.is_some()
        && (source.is_scoped_scan() || force || !options.skip_official_uninstall)
    {
        return Err("uninstall review expired".into());
    }
    if options.skip_official_uninstall
        && !source.is_scoped_scan()
        && !options.dry_run
        && !force
        && options.uninstall_review.is_none()
    {
        return Err("uninstall review expired".into());
    }
    Ok(())
}

#[tauri::command]
async fn run_official_uninstall(
    app: InstalledApp,
) -> Result<executor::OfficialUninstallResult, String> {
    let app =
        apps::trusted_app(&app).ok_or_else(|| "app is not from the latest scan".to_string())?;
    tauri::async_runtime::spawn_blocking(move || executor::run_official_uninstall(&app))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_full_cleanup(
    app: InstalledApp,
    items: Vec<CleanupItem>,
    options: FullCleanupOptions,
    operation_id: Option<u32>,
) -> Result<FullCleanupReport, String> {
    let request = operation_id
        .map(executor::cleanup_request::CleanupRequest::claim)
        .transpose()?;
    if options.dry_run && request.is_some() {
        return Err("a cleanup request cannot be used for a preview".into());
    }
    let source = cleanup_source_from_arg(options.cleanup_source.as_deref());
    validate_cleanup_intent(source, &options)?;
    let app = resolve_review_cleanup_app(app, source, options.uninstall_review)?;
    let scan = authorize_cleanup_items_for_review(
        &app,
        &items,
        source,
        options.uninstall_review,
        options.scope_revision,
    )?;
    #[cfg(test)]
    let history_dir = history::test_history_dir();
    #[cfg(test)]
    let inventory = apps::inventory_test_support::current();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(test)]
        let _history = history_dir.as_deref().map(history::use_history_dir);
        #[cfg(test)]
        let _inventory = inventory.map(apps::inventory_test_support::Fixture::enter);
        let work = || executor::run_full_cleanup_for_scan(&app, &items, &options, scan.as_ref());
        let mut report = match request {
            Some(request) => request.run(work),
            None => work(),
        };
        if let Err(error) = history::append(
            &report.app_name,
            report.deleted,
            report.failed,
            report.skipped,
            report.delayed,
            report.aborted,
            report.dry_run,
            &report.backup_dir,
        ) {
            eprintln!("cleanup history could not be saved: {error}");
            report.history_save_failed = true;
        }
        report
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn is_elevated() -> Result<bool, String> {
    // Keep the UI's admin indicator fail-closed with the destructive gates: an
    // unreadable token is reported as elevated rather than as a normal user.
    Ok(crate::sysops::is_elevated_for_gate())
}

#[derive(serde::Serialize)]
struct DiskInfo {
    free_gb: f64,
    total_gb: f64,
    drive: String,
}

#[tauri::command]
fn disk_usage() -> Result<DiskInfo, String> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        // System drive, not hardcoded C:
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let drive = drive.trim_end_matches('\\').to_uppercase();
        let root = format!("{drive}\\\0");
        let wide: Vec<u16> = root.encode_utf16().collect();
        let mut free = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        unsafe {
            let ok = GetDiskFreeSpaceExW(
                PCWSTR(wide.as_ptr()),
                Some(&mut free),
                Some(&mut total),
                Some(&mut total_free),
            );
            if ok.is_err() {
                return Err("GetDiskFreeSpaceExW failed".into());
            }
        }
        Ok(DiskInfo {
            free_gb: total_free as f64 / 1024f64.powi(3),
            total_gb: total as f64 / 1024f64.powi(3),
            drive,
        })
    }
    #[cfg(not(windows))]
    {
        Ok(DiskInfo {
            free_gb: 0.0,
            total_gb: 0.0,
            drive: String::new(),
        })
    }
}

#[tauri::command]
async fn elevate_restart(app: tauri::AppHandle, approved: Option<bool>) -> Result<(), String> {
    // Ask the old window to finish its busy confirmation before spawning
    // the elevated copy: user deliberation must not consume the handoff wait.
    if approved != Some(true) {
        return app
            .emit("remova:request-elevate", ())
            .map_err(|e| e.to_string());
    }
    tauri::async_runtime::spawn_blocking(|| {
        // Busy confirmation is complete. The elevated copy waits for the
        // old window to finish exiting before acquiring the instance lock.
        let handoff = format!("--elevated-relaunch={}", std::process::id());
        sysops::elevate_relaunch(&[handoff])
    })
    .await
    .map_err(|e| e.to_string())??;
    // The requesting window destroys itself only after this succeeds.
    Ok(())
}

type PendingAnalysis = std::sync::Mutex<std::collections::VecDeque<String>>;

fn analyze_argument(args: impl Iterator<Item = String>) -> Option<String> {
    args.skip_while(|arg| arg != "--analyze")
        .nth(1)
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
}

#[tauri::command]
fn take_pending_analyze(queue: tauri::State<'_, PendingAnalysis>) -> Option<String> {
    queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
}

#[tauri::command]
async fn scan_orphan_leftovers(
    scan_id: Option<u32>,
    extended: Option<bool>,
) -> Result<orphans::OrphanScanResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    let extended = extended.unwrap_or(false);
    #[cfg(test)]
    let inventory = apps::inventory_test_support::current();
    #[cfg(test)]
    let roots = orphans::test_roots();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(test)]
        let _inventory = inventory.map(apps::inventory_test_support::Fixture::enter);
        #[cfg(test)]
        let _roots = roots.map(orphans::use_test_roots);
        scan_task::run_scoped(id, "orphan", || {
            scan_task::checkpoint("registry", 0);
            let inventory = apps::scan_install_owners();
            orphans::scan_orphans_with_install_owners(&inventory, extended)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Installation vectors (M4E.2): product families with one row per install
/// vector (per-machine / per-user / Store). Read-only; grouping never merges
/// authorization identities.
#[tauri::command]
async fn scan_install_vectors() -> Result<Vec<vectors::ProductFamily>, String> {
    tauri::async_runtime::spawn_blocking(vectors::scan_install_vectors)
        .await
        .map_err(|e| e.to_string())
}

/// MSI database cross-check (M4E.3): Uninstall records x Installer product
/// keys x Windows Installer database. Read-only diagnosis. Runs under a scan
/// task so the in-scan cancellation checks are live.
#[tauri::command]
async fn scan_msi_consistency(
    scan_id: Option<u32>,
) -> Result<msiconsistency::MsiConsistencyReport, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "msiconsistency", msiconsistency::scan_msi_consistency)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// CLI residue diagnostics (M4E.4): prefixes, shims, empty scopes, PATH
/// entries and redacted .npmrc keys. Read-only. Runs under a scan task.
#[tauri::command]
async fn scan_cli_residue(scan_id: Option<u32>) -> Result<cliresidue::CliResidueReport, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "cliresidue", cliresidue::scan_cli_residue)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Shell-association orphans (M4E.5): Applications / URL protocols /
/// Compatibility Assistant entries whose exe is gone. Read-only. Runs under a
/// scan task so the in-loop cancellation checks are live.
#[tauri::command]
async fn scan_shell_assoc_orphans(
    scan_id: Option<u32>,
) -> Result<shellassoc::ShellAssocScanResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "shellassoc", shellassoc::scan_shell_assoc_orphans)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Toolchain audit (M4E.6): every Visual Studio family instance with
/// filesystem workload probes. Read-only. Runs under a scan task.
#[tauri::command]
async fn audit_toolchains(
    scan_id: Option<u32>,
) -> Result<toolchain_audit::ToolchainAuditResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "toolchain", toolchain_audit::audit_toolchains)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Bounded project-consumer search under user-chosen roots. Read-only. Runs
/// under a scan task.
#[tauri::command]
async fn find_project_consumers(
    scan_id: Option<u32>,
    roots: Vec<String>,
) -> Result<toolchain_audit::ConsumerSearchResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "consumers", || toolchain_audit::find_consumers(&roots))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Broken MSI install records: the directory is gone but the Uninstall entry
/// remains, so Programs & Features can no longer remove the product (§9.7).
#[tauri::command]
async fn scan_broken_installs() -> Result<Vec<apps::BrokenInstall>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let rows = apps::scan_broken_installs();
        #[cfg(windows)]
        icon::remember_additional_icon_apps(rows.iter().map(|row| row.app.clone()));
        rows
    })
    .await
    .map_err(|e| e.to_string())
}

/// Remove a broken record after exporting its keys to a Safety Vault session.
#[tauri::command]
async fn cleanup_broken_install(
    app_name: String,
    registry_key: String,
    guid: String,
) -> Result<apps::BrokenCleanupReport, String> {
    let elevated = is_elevated()?;
    tauri::async_runtime::spawn_blocking(move || {
        apps::cleanup_broken_install(&app_name, &registry_key, &guid, elevated)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Orphan firewall rules whose executable is gone (read-only).
#[tauri::command]
async fn scan_firewall_orphans() -> Result<firewall::FirewallScanResult, String> {
    tauri::async_runtime::spawn_blocking(firewall::scan_orphan_firewall_rules)
        .await
        .map_err(|e| e.to_string())
}

/// Directory reparse points under the known roots (read-only ledger).
#[tauri::command]
async fn scan_junction_ledger() -> Result<junction::JunctionScanResult, String> {
    tauri::async_runtime::spawn_blocking(junction::scan_junction_ledger)
        .await
        .map_err(|e| e.to_string())
}

/// WinSxS component-store analysis via DISM (read-only, elevated).
#[tauri::command]
async fn analyze_component_store() -> Result<winsxs::ComponentStoreAnalysis, String> {
    if !is_elevated()? {
        return Err("administrator privileges are required to analyze the component store".into());
    }
    winsxs::analyze_shared().await
}

/// Resolve the current Windows account's profile for local audit exports.
#[tauri::command]
fn audit_profile_root() -> Option<String> {
    #[cfg(windows)]
    return sysops::lookup_known_folder(&windows::Win32::UI::Shell::FOLDERID_Profile)
        .map(|path| path.to_string_lossy().into_owned());
    #[cfg(not(windows))]
    None
}

/// Space triage for the disk overview (read-only, judgement table).
#[tauri::command]
async fn scan_space_triage() -> Result<triage::TriageScanResult, String> {
    tauri::async_runtime::spawn_blocking(triage::scan_space_triage)
        .await
        .map_err(|e| e.to_string())
}

/// Regenerable-layout scan (read-only; an optional path narrows the fixed set).
#[tauri::command]
async fn scan_regenerable_layouts(
    path: Option<String>,
) -> Result<regenclean::RegenerableScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || regenclean::scan_regenerable(path.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(all(test, windows))]
#[test]
#[ignore = "times the read-only space checkup on the current Windows machine"]
fn inspect_disk_overview_scans() {
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let start = std::time::Instant::now();
            let drives = diskradar::list_local_drives();
            println!(
                "Volume: {:?}, system found={}",
                start.elapsed(),
                drives.iter().any(|d| d.is_system)
            );
        });
        scope.spawn(|| {
            let start = std::time::Instant::now();
            let result = junction::scan_junction_ledger();
            println!(
                "Junctions: {:?}, links={}, incomplete roots={}, omitted={}",
                start.elapsed(),
                result.entries.len(),
                result.degraded_roots.len(),
                result.omitted
            );
        });
        scope.spawn(|| {
            let start = std::time::Instant::now();
            let result = triage::scan_space_triage();
            println!(
                "Triage: {:?}, rows={}",
                start.elapsed(),
                result.entries.len()
            );
        });
        scope.spawn(|| {
            let start = std::time::Instant::now();
            let result = regenclean::scan_regenerable(None).expect("Windows cache roots");
            println!(
                "Regenerable layouts: {:?}, rows={}",
                start.elapsed(),
                result.items.len()
            );
        });
    });
}

/// Global CLI package inventory, npm/pnpm/yarn (read-only).
#[tauri::command]
async fn scan_cli_globals() -> Result<cliglobals::CliGlobalScanResult, String> {
    tauri::async_runtime::spawn_blocking(cliglobals::scan_cli_globals)
        .await
        .map_err(|e| e.to_string())
}

/// Junction migration: admission and sizes (read-only).
#[tauri::command]
async fn migrate_plan(source: String, target: String) -> Result<migrate::MigrationPlan, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(migrate::plan(&source, &target)))
        .await
        .map_err(|e| e.to_string())?
}

/// Junction migration phase 2: copy + per-file diff (non-destructive).
#[tauri::command]
fn begin_migrate_copy(source: String, target: String) -> Result<u32, String> {
    migrate::copy_operation::begin(source, target)
}

#[tauri::command]
async fn migrate_copy(operation_id: u32) -> Result<migrate::MigrationCopyReport, String> {
    let operation = migrate::copy_operation::Operation::claim(operation_id)?;
    tauri::async_runtime::spawn_blocking(move || operation.run())
        .await
        .map_err(|e| e.to_string())?
}

/// Junction migration phase 3: rename + junction + probe (transaction lock).
#[tauri::command]
async fn migrate_cutover(
    source: String,
    target: String,
) -> Result<migrate::MigrationCutoverReport, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(migrate::cutover(&source, &target)))
        .await
        .map_err(|e| e.to_string())?
}

/// Junction migration phase 4: verify the backup, then drop it (confirmed).
#[tauri::command]
async fn migrate_finish(
    source: String,
    target: String,
    confirmed: bool,
) -> Result<migrate::MigrationFinishReport, String> {
    tauri::async_runtime::spawn_blocking(move || migrate::finish(&source, &target, confirmed))
        .await
        .map_err(|e| e.to_string())?
}

/// Restore the original directory from either a cutover or an interrupted switch.
#[tauri::command]
async fn migrate_rollback(
    source: String,
    target: String,
    recover_interrupted: Option<bool>,
) -> Result<migrate::MigrationRollbackReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(if recover_interrupted.unwrap_or(false) {
            migrate::recover(&source, &target)
        } else {
            migrate::rollback(&source, &target)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Where the migration stands, derived from the filesystem.
#[tauri::command]
async fn migrate_status(source: String, target: String) -> Result<migrate::MigrationState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(migrate::state_of(
            std::path::Path::new(source.trim()),
            std::path::Path::new(target.trim()),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Orphaned ConsentStore NonPackaged entries (read-only, evaluation-gated).
#[tauri::command]
async fn scan_consent_orphans() -> Result<traces::TraceScanResult, String> {
    tauri::async_runtime::spawn_blocking(traces::scan_consent_orphans)
        .await
        .map_err(|e| e.to_string())
}

/// Portable-app candidates from firewall rules x install records (read-only).
#[tauri::command]
async fn scan_portable_candidates() -> Result<portable::PortableScanResult, String> {
    tauri::async_runtime::spawn_blocking(portable::scan_portable_candidates)
        .await
        .map_err(|e| e.to_string())
}

/// Cancel the specified queued or running migration copy.
#[tauri::command]
fn cancel_migrate_copy(operation_id: u32) -> Result<(), String> {
    migrate::copy_operation::cancel(operation_id)
}

/// Remove npm global packages (Safety Vault backup first; admission strict).
#[tauri::command]
async fn remove_npm_globals(
    packages: Vec<npmglobals::NpmPackageRef>,
) -> Result<npmglobals::NpmCleanupReport, String> {
    tauri::async_runtime::spawn_blocking(move || npmglobals::remove_npm_globals(&packages))
        .await
        .map_err(|e| e.to_string())?
}

/// Delete the contents of regenerable layouts (no backup: invariant 7; the
/// server re-admits every path through a fresh scan first).
#[tauri::command]
async fn delete_regenerable_layouts(
    paths: Vec<String>,
) -> Result<regenclean::RegenerableCleanupReport, String> {
    tauri::async_runtime::spawn_blocking(move || regenclean::delete_regenerable(&paths))
        .await
        .map_err(|e| e.to_string())?
}

/// Delete unchanged orphan rule values after verifying their registry backups.
#[tauri::command]
async fn delete_firewall_rules(
    selections: Vec<firewall::FirewallSelection>,
) -> Result<firewall::FirewallCleanupReport, String> {
    if !is_elevated()? {
        return Err("administrator privileges are required to delete firewall rules".into());
    }
    tauri::async_runtime::spawn_blocking(move || firewall::delete_firewall_rules(&selections))
        .await
        .map_err(|e| e.to_string())?
}

/// Name the processes holding a file/directory (read-only, Restart Manager).
#[tauri::command]
async fn diagnose_locked_path(path: String) -> Result<Vec<locks::LockingProcess>, String> {
    tauri::async_runtime::spawn_blocking(move || locks::diagnose_locked_path(&path))
        .await
        .map_err(|e| e.to_string())?
}

/// End one confirmed process after re-verifying its image is inside the app.
#[tauri::command]
async fn end_locking_process(
    pid: u32,
    expected_path: String,
    target_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        locks::end_locking_process(pid, &expected_path, &target_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Bounded, read-only search for product-named keys under SOFTWARE.
#[tauri::command]
async fn deep_registry_check(app_name: String) -> Result<deep_reg::RegistryCheckResult, String> {
    tauri::async_runtime::spawn_blocking(move || deep_reg::deep_registry_check(&app_name))
        .await
        .map_err(|e| e.to_string())?
}

/// Idle software radar (read-only ranking).
#[tauri::command]
async fn rank_idle_apps(scan_id: Option<u32>) -> Result<idle::IdleScanResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "idle", || {
            scan_task::checkpoint("registry", 0);
            let mut installed = apps::scan_installed_inventory();
            // HKCU entries are user-writable; an elevated idle pass must not use
            // their InstallLocation as a filesystem probe target.
            if crate::sysops::is_elevated_for_gate() {
                installed.retain(|a| !a.source.to_ascii_uppercase().starts_with("HKCU"));
            }
            if scan_task::cancelled() {
                return idle::IdleScanResult {
                    items: Vec::new(),
                    omitted: 0,
                };
            }
            apps::filter_visible_apps(&mut installed, &ignore::load());
            let result = idle::rank_idle_apps(&installed);
            #[cfg(windows)]
            icon::remember_additional_icon_apps(result.items.iter().map(|row| row.app.clone()));
            result
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Installer packages + updater caches (scoped delete allow-list).
#[tauri::command]
async fn scan_installer_caches(scan_id: Option<u32>) -> Result<scanner::ScopedScanResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "installer", installers::scan_installer_caches)
            .and_then(|result| result)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Dev / game / browser tool caches (scoped delete allow-list).
#[tauri::command]
async fn scan_tool_caches(scan_id: Option<u32>) -> Result<scanner::ScopedScanResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "toolcache", toolcache::scan_tool_caches)
            .and_then(|result| result)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Disk radar: local fixed drives with free/total space (read-only).
#[tauri::command]
async fn list_local_drives() -> Result<Vec<diskradar::DriveInfo>, String> {
    tauri::async_runtime::spawn_blocking(diskradar::list_local_drives)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn disk_radar_volume_id(drive: char) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || diskradar::volume_id(drive))
        .await
        .map_err(|error| error.to_string())
}

/// Disk radar: top directories for one drive (system drive → well-known roots).
#[tauri::command]
async fn list_top_dir_sizes(
    drive: Option<String>,
    scan_id: Option<u32>,
) -> Result<Vec<diskradar::DirSizeRow>, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        let letter = drive
            .as_deref()
            .and_then(|s| s.trim().chars().next())
            .filter(|c| c.is_ascii_alphabetic());
        scan_task::run_scoped(id, "disk", || diskradar::top_dir_sizes_for_drive(letter))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Disk radar drill-down: immediate children of a directory (read-only).
#[tauri::command]
async fn list_dir_children(
    path: String,
    scan_id: Option<u32>,
) -> Result<diskradar::DirChildrenResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "disk", || diskradar::list_dir_children(&path))
            .and_then(|result| result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn inspect_folder_references(
    path: String,
    scan_id: u32,
) -> Result<folder_references::InspectionOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || folder_references::inspect_scoped(&path, scan_id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn pick_reference_folder(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|_| "folder:picker_unavailable")?.0 as isize;
        tauri::async_runtime::spawn_blocking(move || folder_references::pick_folder(hwnd))
            .await
            .map_err(|_| "folder:picker_unavailable")?
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("folder:picker_unavailable".into())
    }
}

/// re-check selected paths after cleanup (checklist evidence).
#[tauri::command]
async fn verify_cleanup_leftovers(
    items: Vec<scanner::CleanupItem>,
) -> Result<Vec<sysops::VerifyRow>, String> {
    tauri::async_runtime::spawn_blocking(move || sysops::verify_cleanup_leftovers(&items))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn install_monitor_status() -> Result<installmon::MonitorStatus, String> {
    tauri::async_runtime::spawn_blocking(installmon::status)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn discard_install_monitor_recovery() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(installmon::discard_recovery)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn begin_install_monitor(
    scan_id: Option<u32>,
    trial_name: Option<String>,
) -> Result<(), String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "monitor", || installmon::begin_named(trial_name))
            .and_then(|result| result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn end_install_monitor(scan_id: Option<u32>) -> Result<installmon::MonitorEndResult, String> {
    let id = scan_id.map_or_else(scan_task::begin, Ok)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_task::run_scoped(id, "monitor", installmon::end).and_then(|result| result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_install_trial_records() -> Result<Vec<installmon::trial_records::TrialRecord>, String>
{
    tauri::async_runtime::spawn_blocking(installmon::trial_records::list)
        .await
        .map_err(|error| error.to_string())?
}

fn tray_labels(lang: &str) -> (&'static str, &'static str) {
    if lang.eq_ignore_ascii_case("zh") {
        ("打开 Remova", "退出 Remova")
    } else {
        ("Open Remova", "Quit Remova")
    }
}

fn system_language_is_zh() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::GetUserDefaultUILanguage;
        // LANG_CHINESE = 0x04 in the low 10 bits (primary language id).
        unsafe { GetUserDefaultUILanguage() & 0x03ff == 0x04 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[tauri::command]
fn begin_cleanup_operation() -> Result<u32, String> {
    executor::cleanup_request::begin()
}

#[tauri::command]
fn cancel_cleanup(operation_id: u32) -> bool {
    executor::cleanup_request::cancel(operation_id)
}

#[tauri::command]
fn reboot_gate_status() -> String {
    executor::reboot::gate_status().as_str().into()
}

#[tauri::command]
async fn list_pending_uninstall_reviews(
) -> Result<Vec<executor::pending_review::PendingReview>, String> {
    tauri::async_runtime::spawn_blocking(executor::pending_review::list)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn check_pending_uninstall_review(
    id: u32,
) -> Result<executor::pending_review::ReviewStatus, String> {
    tauri::async_runtime::spawn_blocking(move || executor::pending_review::check(id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn dismiss_pending_uninstall_review(id: u32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || executor::pending_review::dismiss(id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn clear_corrupt_reboot_gate() -> Result<bool, String> {
    match executor::reboot::clear_corrupt_gate() {
        executor::reboot::ClearCorruptGate::Cleared => Ok(true),
        executor::reboot::ClearCorruptGate::NotCorrupt => Err("reboot:gate_not_corrupt".into()),
        executor::reboot::ClearCorruptGate::RenameFailed => Err("reboot:gate_rename_failed".into()),
    }
}

/// Rebuild the tray menu when the UI language changes. The tray is created
/// before the webview loads, so its labels otherwise stay in the OS language
/// even after the user switches Remova to English.
#[tauri::command]
fn set_tray_language(app: tauri::AppHandle, lang: String) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    let (show, quit) = tray_labels(&lang);
    let show_i =
        MenuItem::with_id(&app, "show", show, true, None::<&str>).map_err(|e| e.to_string())?;
    let quit_i =
        MenuItem::with_id(&app, "quit", quit, true, None::<&str>).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&show_i, &quit_i]).map_err(|e| e.to_string())?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // One-shot Safety Vault retention (not on list/read paths).
    std::thread::spawn(|| {
        let _ = restore::prune_old_sessions(crate::constants::BACKUP_RETENTION_DAYS);
        #[cfg(windows)]
        path_seal::cleanup_abandoned_stages();
    });
    // The secondary process only forwards its arguments. The primary process
    // owns both startup and forwarded requests until the WebView consumes them.
    let initial_analysis = analyze_argument(std::env::args().skip(1));
    let builder = tauri::Builder::default()
        .manage(PendingAnalysis::new(initial_analysis.into_iter().collect()))
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Focus the existing window when a second launch happens
        // (e.g. Explorer context menu while Remova is already open).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let queued = if let Some(path) = analyze_argument(args.into_iter().skip(1)) {
                app.state::<PendingAnalysis>()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push_back(path);
                true
            } else {
                false
            };
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
                if queued {
                    let _ = win.emit("pending-analyze", ());
                }
            }
        }))
        .setup(|app| {
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
            use tauri::Manager;

            let (show_label, quit_label) =
                tray_labels(if system_language_is_zh() { "zh" } else { "en" });
            let show_i = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;
            // Prefer dedicated 32×32 transparent PNG for the tray (crisp at 16–32 px).
            let icon = match tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png")) {
                Ok(i) => i,
                Err(_) => match app.default_window_icon().cloned() {
                    Some(i) => i,
                    None => {
                        eprintln!("[remova] tray icon missing — tray disabled");
                        return Ok(());
                    }
                },
            };
            // Taskbar / alt-tab: larger raster so high-DPI does not upscale a 32px glyph.
            if let Ok(big) =
                tauri::image::Image::from_bytes(include_bytes!("../icons/128x128@2x.png"))
            {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_icon(big);
                }
            }
            let _tray = TrayIconBuilder::with_id("main")
                .icon(icon)
                .tooltip("Remova")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                            let _ = app.emit("remova:request-quit", ());
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        // @commands:start
        // Machine-readable command inventory. scripts/check-commands.ps1 verifies this block
        // against generate_handler! below, so the cross-check runs in CI, where docs/ is
        // absent and the human-readable table is unavailable. Add one entry per command
        // when registering a new handler.
        // `list_installed_apps`
        // `app_icon_data`
        // `estimate_dir_size_kb`
        // `cancel_size_estimate`
        // `begin_size_estimate`
        // `open_path_in_explorer`
        // `check_github_latest`
        // `online_update_supported`
        // `analyze_associations`
        // `begin_association_scan`
        // `cancel_association_scan`
        // `association_scan_progress`
        // `run_cleanup_dry_run`
        // `run_full_cleanup`
        // `run_official_uninstall`
        // `list_cleanup_history`
        // `export_history_csv`
        // `delete_cleanup_history`
        // `clear_cleanup_history`
        // `is_elevated`
        // `disk_usage`
        // `elevate_restart`
        // `list_restore_sessions`
        // `list_backup_sessions`
        // `delete_backup_session`
        // `restore_session_by_name`
        // `cancel_restore`
        // `preview_restore_session`
        // `cancel_restore_preview`
        // `list_startup_items`
        // `list_services`
        // `list_scheduled_tasks`
        // `set_startup_enabled`
        // `set_service_start_disabled`
        // `set_service_running`
        // `set_task_enabled`
        // `load_ignore`
        // `ignore_publisher`
        // `ignore_app_name`
        // `unignore_publisher`
        // `unignore_app_name`
        // `unignore_path`
        // `suggest_ignore_rules`
        // `apply_ignore_suggestions`
        // `scan_orphan_leftovers`
        // `rank_idle_apps`
        // `scan_broken_installs`
        // `cleanup_broken_install`
        // `scan_firewall_orphans`
        // `delete_firewall_rules`
        // `scan_junction_ledger`
        // `analyze_component_store`
        // `audit_profile_root`
        // `scan_space_triage`
        // `scan_regenerable_layouts`
        // `delete_regenerable_layouts`
        // `scan_cli_globals`
        // `remove_npm_globals`
        // `migrate_plan`
        // `begin_migrate_copy`
        // `migrate_copy`
        // `migrate_cutover`
        // `migrate_finish`
        // `migrate_rollback`
        // `migrate_status`
        // `cancel_migrate_copy`
        // `scan_consent_orphans`
        // `scan_install_vectors`
        // `scan_msi_consistency`
        // `scan_cli_residue`
        // `scan_shell_assoc_orphans`
        // `audit_toolchains`
        // `find_project_consumers`
        // `scan_portable_candidates`
        // `diagnose_locked_path`
        // `end_locking_process`
        // `deep_registry_check`
        // `scan_installer_caches`
        // `scan_tool_caches`
        // `list_local_drives`
        // `disk_radar_volume_id`
        // `list_top_dir_sizes`
        // `list_dir_children`
        // `inspect_folder_references`
        // `pick_reference_folder`
        // `verify_cleanup_leftovers`
        // `begin_install_monitor`
        // `end_install_monitor`
        // `list_install_trial_records`
        // `install_monitor_status`
        // `discard_install_monitor_recovery`
        // `take_pending_analyze`
        // `begin_cleanup_operation`
        // `cancel_cleanup`
        // `reboot_gate_status`
        // `list_pending_uninstall_reviews`
        // `check_pending_uninstall_review`
        // `dismiss_pending_uninstall_review`
        // `clear_corrupt_reboot_gate`
        // `set_tray_language`
        // `get_ai_config`
        // `save_ai_config`
        // `ai_risk_brief`
        // `ai_explain_items`
        // `ai_summarize_report`
        // `ai_parse_intent`
        // @commands:end
        .invoke_handler(tauri::generate_handler![
            list_installed_apps,
            app_icon_data,
            estimate_dir_size_kb,
            cancel_size_estimate,
            begin_size_estimate,
            open_path_in_explorer,
            commands::update::check_github_latest,
            commands::update::online_update_supported,
            analyze_associations,
            begin_association_scan,
            cancel_association_scan,
            association_scan_progress,
            run_cleanup_dry_run,
            run_full_cleanup,
            run_official_uninstall,
            commands::history_cmd::list_cleanup_history,
            commands::history_cmd::export_history_csv,
            commands::history_cmd::delete_cleanup_history,
            commands::history_cmd::clear_cleanup_history,
            is_elevated,
            disk_usage,
            elevate_restart,
            commands::backup_cmd::list_restore_sessions,
            commands::backup_cmd::list_backup_sessions,
            commands::backup_cmd::delete_backup_session,
            commands::backup_cmd::restore_session_by_name,
            commands::backup_cmd::cancel_restore,
            commands::backup_cmd::preview_restore_session,
            commands::backup_cmd::cancel_restore_preview,
            commands::manage_cmd::list_startup_items,
            commands::manage_cmd::list_services,
            commands::manage_cmd::list_scheduled_tasks,
            commands::manage_cmd::set_startup_enabled,
            commands::manage_cmd::set_service_start_disabled,
            commands::manage_cmd::set_service_running,
            commands::manage_cmd::set_task_enabled,
            commands::ignore_cmd::load_ignore,
            commands::ignore_cmd::ignore_publisher,
            commands::ignore_cmd::ignore_app_name,
            commands::ignore_cmd::unignore_publisher,
            commands::ignore_cmd::unignore_app_name,
            commands::ignore_cmd::unignore_path,
            commands::ignore_cmd::suggest_ignore_rules,
            commands::ignore_cmd::apply_ignore_suggestions,
            scan_orphan_leftovers,
            rank_idle_apps,
            scan_broken_installs,
            cleanup_broken_install,
            scan_firewall_orphans,
            delete_firewall_rules,
            scan_junction_ledger,
            analyze_component_store,
            audit_profile_root,
            scan_space_triage,
            scan_regenerable_layouts,
            delete_regenerable_layouts,
            scan_cli_globals,
            remove_npm_globals,
            migrate_plan,
            begin_migrate_copy,
            migrate_copy,
            migrate_cutover,
            migrate_finish,
            migrate_rollback,
            migrate_status,
            cancel_migrate_copy,
            scan_consent_orphans,
            scan_install_vectors,
            scan_msi_consistency,
            scan_cli_residue,
            scan_shell_assoc_orphans,
            audit_toolchains,
            find_project_consumers,
            scan_portable_candidates,
            diagnose_locked_path,
            end_locking_process,
            deep_registry_check,
            scan_installer_caches,
            scan_tool_caches,
            list_local_drives,
            disk_radar_volume_id,
            list_top_dir_sizes,
            list_dir_children,
            inspect_folder_references,
            pick_reference_folder,
            verify_cleanup_leftovers,
            begin_install_monitor,
            end_install_monitor,
            list_install_trial_records,
            install_monitor_status,
            discard_install_monitor_recovery,
            take_pending_analyze,
            begin_cleanup_operation,
            cancel_cleanup,
            reboot_gate_status,
            list_pending_uninstall_reviews,
            check_pending_uninstall_review,
            dismiss_pending_uninstall_review,
            clear_corrupt_reboot_gate,
            set_tray_language,
            commands::ai_cmd::get_ai_config,
            commands::ai_cmd::save_ai_config,
            commands::ai_cmd::ai_risk_brief,
            commands::ai_cmd::ai_explain_items,
            commands::ai_cmd::ai_summarize_report,
            commands::ai_cmd::ai_parse_intent
        ]);
    match builder.run(tauri::generate_context!()) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("[remova] failed to run: {e}");
            std::process::exit(1);
        }
    }
}
