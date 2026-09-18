//! Uninstall command parsing and dry-run cleanup planning (Phase 2).

use crate::safety::is_safe_to_delete_registry;
use crate::scanner::{CleanupItem, ItemKind};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub app_name: String,
    pub dry_run: bool,
    pub uninstall_command: Vec<String>,
    pub uninstall_message: String,
    pub deleted_planned: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
    pub item_details: Vec<ItemDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDetail {
    pub path: String,
    pub kind: String,
    pub status: String,
    pub message: String,
}

/// Lightweight error alias until thiserror is fully adopted (ARCH-5).
pub type ExecResult<T> = Result<T, String>;

/// Build argv for the official uninstaller (parity with Python `build_uninstall_command`).
/// Store packages (`remova-store:<PackageFullName>`) map to PowerShell Remove-AppxPackage.
pub fn build_uninstall_command(
    uninstall_string: &str,
    quiet_uninstall: &str,
    prefer_quiet: bool,
) -> Option<Vec<String>> {
    let mut raw = String::new();
    if prefer_quiet && !quiet_uninstall.trim().is_empty() {
        raw = quiet_uninstall.trim().to_string();
    }
    if raw.is_empty() {
        raw = uninstall_string.trim().to_string();
    }
    if raw.is_empty() {
        return None;
    }

    if let Some(full) = raw.strip_prefix("remova-store:") {
        let full = full.trim();
        if full.is_empty() {
            return None;
        }
        return Some(vec![
            "powershell.exe".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            format!("Remove-AppxPackage -Package '{full}' -ErrorAction Stop"),
        ]);
    }

    // MSI product code: only when the string is clearly MSI (msiexec present, or bare {GUID}).
    if let Some(guid) = extract_guid(&raw) {
        let lower = raw.to_lowercase();
        let trimmed = raw.trim();
        let bare_guid = trimmed == guid || trimmed.trim_matches('"').eq_ignore_ascii_case(&guid);
        if lower.contains("msiexec") || bare_guid {
            return Some(vec![
                "msiexec.exe".into(),
                "/x".into(),
                guid,
                "/qn".into(),
                "/norestart".into(),
            ]);
        }
    }

    if let Some(rest) = raw.strip_prefix('"') {
        let end = rest.find('"')?;
        let exe = rest[..end].to_string();
        let after = rest[end + 1..].trim();
        let args: Vec<String> = split_win_args(after);
        let mut cmd = vec![exe];
        cmd.extend(args);
        return Some(cmd);
    }

    let parts = split_win_args(&raw);
    if parts.is_empty() {
        return None;
    }
    Some(parts)
}

fn extract_guid(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s[start..].find('}')? + start;
    let g = &s[start..=end];
    let body = &g[1..g.len() - 1];
    if body.len() == 36 && body.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        Some(g.to_string())
    } else {
        None
    }
}

fn split_win_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let chars = s.chars().peekable();
    for c in chars {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !cur.is_empty() {
                    out.push(cur.trim_matches('"').to_string());
                    cur.clear();
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur.trim_matches('"').to_string());
    }
    out
}

fn is_safe_fs(p: &Path) -> bool {
    crate::safety::is_safe_fs(p)
}

/// Dry-run cleanup: validate items and report what would happen. Never deletes.
pub fn run_cleanup_dry(app_name: &str, items: &[CleanupItem]) -> CleanupReport {
    let mut deleted_planned = 0u32;
    let mut skipped = 0u32;
    let errors = vec![];
    let mut details = vec![];

    for it in items {
        let ok = match it.kind {
            ItemKind::Registry => is_safe_to_delete_registry(&it.path).is_ok(),
            ItemKind::Path => !it.path.trim().is_empty(),
            _ => is_safe_fs(Path::new(&it.path)) && Path::new(&it.path).exists(),
        };
        if !ok {
            skipped += 1;
            details.push(ItemDetail {
                path: it.path.clone(),
                kind: format!("{:?}", it.kind).to_lowercase(),
                status: "skipped".into(),
                message: "failed safety gate".into(),
            });
            continue;
        }
        deleted_planned += 1;
        details.push(ItemDetail {
            path: it.path.clone(),
            kind: format!("{:?}", it.kind).to_lowercase(),
            status: "planned".into(),
            message: "[dry-run]".into(),
        });
    }

    let _ = app_name;
    CleanupReport {
        app_name: app_name.to_string(),
        dry_run: true,
        uninstall_command: vec![],
        uninstall_message: "dry-run: official uninstaller not launched".into(),
        deleted_planned,
        skipped,
        errors,
        item_details: details,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCleanupOptions {
    pub dry_run: bool,
    pub skip_official_uninstall: bool,
    pub backup_enabled: bool,
    /// Create a Windows restore point (tests set false to avoid real side effects).
    #[serde(default = "default_true")]
    pub restore_point: bool,
}

fn default_true() -> bool {
    true
}

/// Result of launching only the official uninstaller (no residual delete).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialUninstallResult {
    pub ok: bool,
    pub message: String,
    pub had_command: bool,
}

/// Launch the official uninstaller and wait (shared by full cleanup and beginner uninstall).
pub fn run_official_uninstall(app: &crate::apps::InstalledApp) -> OfficialUninstallResult {
    let cmd = build_uninstall_command(&app.uninstall_string, &app.quiet_uninstall_string, true);
    match cmd {
        Some(argv) if !argv.is_empty() => {
            let mut parts = argv.iter();
            let exe = parts.next().unwrap().clone();
            let rest: Vec<String> = parts.cloned().collect();
            match std::process::Command::new(&exe).args(&rest).spawn() {
                Ok(mut child) => {
                    let timeout =
                        std::time::Duration::from_secs(crate::constants::UNINSTALL_TIMEOUT_SECS);
                    let start = std::time::Instant::now();
                    loop {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                let ok = status.success();
                                let message = if ok {
                                    format!("uninstaller finished: {}", argv[0])
                                } else {
                                    format!("uninstaller exited with {:?}", status.code())
                                };
                                return OfficialUninstallResult {
                                    ok,
                                    message,
                                    had_command: true,
                                };
                            }
                            Ok(None) => {
                                if start.elapsed() >= timeout {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    return OfficialUninstallResult {
                                        ok: false,
                                        message: format!("uninstaller timed out (5m): {}", argv[0]),
                                        had_command: true,
                                    };
                                }
                                std::thread::sleep(std::time::Duration::from_millis(200));
                            }
                            Err(e) => {
                                return OfficialUninstallResult {
                                    ok: false,
                                    message: format!("wait failed: {e}"),
                                    had_command: true,
                                };
                            }
                        }
                    }
                }
                Err(e) => OfficialUninstallResult {
                    ok: false,
                    message: format!("launch failed: {e}"),
                    had_command: true,
                },
            }
        }
        _ => OfficialUninstallResult {
            ok: false,
            message: "no uninstall string".into(),
            had_command: false,
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCleanupReport {
    pub app_name: String,
    pub dry_run: bool,
    pub backup_dir: String,
    pub uninstall_ok: bool,
    pub uninstall_message: String,
    pub deleted: u32,
    pub failed: u32,
    pub skipped: u32,
    pub aborted: bool,
    pub restore_point_ok: bool,
    pub restore_point_msg: String,
    pub errors: Vec<String>,
    pub item_details: Vec<ItemDetail>,
}

/// Stage 1: restore point + Safety Vault backup.
enum BackupOutcome {
    Ready {
        backup_dir: String,
        restore_point_ok: bool,
        restore_point_msg: String,
    },
    Abort(Box<FullCleanupReport>),
}

fn try_backup_phase(
    app: &crate::apps::InstalledApp,
    items: &[CleanupItem],
    opts: &FullCleanupOptions,
) -> BackupOutcome {
    if !opts.backup_enabled {
        return BackupOutcome::Ready {
            backup_dir: String::new(),
            restore_point_ok: false,
            restore_point_msg: String::new(),
        };
    }
    let mut restore_point_ok = false;
    let mut restore_point_msg = String::new();
    if opts.restore_point {
        let (rp_ok, rp_msg) = crate::sysops::create_restore_point(&format!(
            "Remova: {}",
            app.name.chars().take(40).collect::<String>()
        ));
        restore_point_ok = rp_ok;
        restore_point_msg = rp_msg;
    }
    match crate::backup::create_session(&app.name) {
        Ok(session) => {
            let backup_dir = session.to_string_lossy().to_string();
            let (_ok, fail, errors) = crate::backup::backup_items(items, &session);
            if fail > 0 {
                return BackupOutcome::Abort(Box::new(FullCleanupReport {
                    app_name: app.name.clone(),
                    dry_run: false,
                    backup_dir,
                    uninstall_ok: false,
                    uninstall_message: format!("backup failed for {fail} item(s); aborted"),
                    deleted: 0,
                    failed: 0,
                    skipped: 0,
                    aborted: true,
                    restore_point_ok,
                    restore_point_msg,
                    errors,
                    item_details: vec![],
                }));
            }
            BackupOutcome::Ready {
                backup_dir,
                restore_point_ok,
                restore_point_msg,
            }
        }
        Err(e) => BackupOutcome::Abort(Box::new(FullCleanupReport {
            app_name: app.name.clone(),
            dry_run: false,
            backup_dir: String::new(),
            uninstall_ok: false,
            uninstall_message: format!("backup session failed: {e}"),
            deleted: 0,
            failed: 0,
            skipped: 0,
            aborted: true,
            restore_point_ok,
            restore_point_msg,
            errors: vec![e.to_string()],
            item_details: vec![],
        })),
    }
}

/// Full cleanup: optional backup → official uninstall → residual delete.
pub fn run_full_cleanup(
    app: &crate::apps::InstalledApp,
    items: &[CleanupItem],
    opts: &FullCleanupOptions,
) -> FullCleanupReport {
    if opts.dry_run {
        let dry = run_cleanup_dry(&app.name, items);
        return FullCleanupReport {
            app_name: dry.app_name,
            dry_run: true,
            backup_dir: String::new(),
            uninstall_ok: false,
            uninstall_message: dry.uninstall_message,
            deleted: dry.deleted_planned,
            failed: 0,
            skipped: dry.skipped,
            aborted: false,
            restore_point_ok: false,
            restore_point_msg: String::new(),
            errors: dry.errors,
            item_details: dry.item_details,
        };
    }

    let selected: Vec<&CleanupItem> = items
        .iter()
        .filter(|it| !it.path.trim().is_empty())
        .collect();
    // Empty leftover set is valid when the user asked for official uninstall only
    // (batch “no default-selectable residue” should still remove the app).
    if selected.is_empty() && opts.skip_official_uninstall {
        return FullCleanupReport {
            app_name: app.name.clone(),
            dry_run: false,
            backup_dir: String::new(),
            uninstall_ok: false,
            uninstall_message: "no items".into(),
            deleted: 0,
            failed: 0,
            skipped: 0,
            aborted: true,
            restore_point_ok: false,
            restore_point_msg: String::new(),
            errors: vec![],
            item_details: vec![],
        };
    }

    let (mut backup_dir, mut restore_point_ok, mut restore_point_msg) =
        match try_backup_phase(app, items, opts) {
            BackupOutcome::Ready {
                backup_dir,
                restore_point_ok,
                restore_point_msg,
            } => (backup_dir, restore_point_ok, restore_point_msg),
            BackupOutcome::Abort(report) => return *report,
        };
    let _ = &mut backup_dir;
    let _ = &mut restore_point_ok;
    let _ = &mut restore_point_msg;

    let mut uninstall_ok = false;
    let mut uninstall_message = "skipped".into();
    if !opts.skip_official_uninstall {
        let official = run_official_uninstall(app);
        uninstall_ok = official.ok;
        uninstall_message = official.message;
    }

    let mut deleted = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;
    let mut errors = vec![];
    let mut details = vec![];

    for it in items {
        match it.kind {
            ItemKind::Path => match crate::regops::scrub_path_entry(&it.path) {
                Ok(true) => {
                    deleted += 1;
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: "path".into(),
                        status: "deleted".into(),
                        message: "PATH entry removed".into(),
                    });
                }
                Ok(false) => {
                    skipped += 1;
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: "path".into(),
                        status: "skipped".into(),
                        message: "not found in PATH".into(),
                    });
                }
                Err(e) => {
                    failed += 1;
                    errors.push(format!("{}: {e}", it.path));
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: "path".into(),
                        status: "failed".into(),
                        message: e,
                    });
                }
            },
            ItemKind::Registry => {
                if is_safe_to_delete_registry(&it.path).is_err() {
                    skipped += 1;
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: "registry".into(),
                        status: "skipped".into(),
                        message: "safety".into(),
                    });
                    continue;
                }
                // Service / task: try native delete first; surface native result in the report.
                let low = it.path.to_uppercase();
                let mut native_note = String::new();
                if low.contains(r"\SYSTEM\CURRENTCONTROLSET\SERVICES\") {
                    let svc = crate::regops::leaf_name(&it.path);
                    let native_ok = crate::regops::sc_delete_service(&svc);
                    native_note = if native_ok {
                        format!("sc delete {svc}: ok")
                    } else {
                        format!("sc delete {svc}: failed or not found")
                    };
                } else if low.contains(r"\SCHEDULE\TASKCACHE\TREE\") {
                    let tn = crate::regops::leaf_name(&it.path);
                    let native_ok = crate::regops::schtasks_delete(&tn);
                    native_note = if native_ok {
                        format!("schtasks delete {tn}: ok")
                    } else {
                        format!("schtasks delete {tn}: failed or not found")
                    };
                }
                let res = if let Some((k, v)) = crate::regops::split_value_path(&it.path) {
                    crate::regops::delete_value(k, v)
                } else {
                    crate::regops::delete_key(&it.path)
                };
                match res {
                    Ok(()) => {
                        deleted += 1;
                        details.push(ItemDetail {
                            path: it.path.clone(),
                            kind: "registry".into(),
                            status: "deleted".into(),
                            message: native_note,
                        });
                    }
                    Err(e) => {
                        failed += 1;
                        let msg = if native_note.is_empty() {
                            e.clone()
                        } else {
                            format!("{native_note}; {e}")
                        };
                        errors.push(format!("{}: {msg}", it.path));
                        details.push(ItemDetail {
                            path: it.path.clone(),
                            kind: "registry".into(),
                            status: "failed".into(),
                            message: msg,
                        });
                    }
                }
            }
            _ => {
                let p = Path::new(&it.path);
                if it.user_data {
                    skipped += 1;
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: format!("{:?}", it.kind).to_lowercase(),
                        status: "skipped".into(),
                        message: "user_data red line".into(),
                    });
                    continue;
                }
                if !is_safe_fs(p) {
                    skipped += 1;
                    continue;
                }
                let res = if p.is_dir() {
                    std::fs::remove_dir_all(p)
                } else if p.exists() {
                    std::fs::remove_file(p)
                } else {
                    Ok(())
                };
                match res {
                    Ok(()) => {
                        deleted += 1;
                        details.push(ItemDetail {
                            path: it.path.clone(),
                            kind: format!("{:?}", it.kind).to_lowercase(),
                            status: "deleted".into(),
                            message: String::new(),
                        });
                    }
                    Err(_e) => {
                        // try schedule delete on reboot for locked files
                        if crate::sysops::schedule_delete_on_reboot(&it.path) {
                            deleted += 1;
                            details.push(ItemDetail {
                                path: it.path.clone(),
                                kind: format!("{:?}", it.kind).to_lowercase(),
                                status: "delayed".into(),
                                message: "reboot delete".into(),
                            });
                        } else {
                            failed += 1;
                            errors.push(format!("{}: delete failed", it.path));
                        }
                    }
                }
            }
        }
    }

    FullCleanupReport {
        app_name: app.name.clone(),
        dry_run: false,
        backup_dir,
        uninstall_ok,
        uninstall_message,
        deleted,
        failed,
        skipped,
        aborted: false,
        restore_point_ok,
        restore_point_msg,
        errors,
        item_details: details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msi_only_for_msiexec_or_bare_guid() {
        // MSI product code string
        let bare = build_uninstall_command(r"{9A1B2C3D-1111-2222-3333-444455556666}", "", false);
        assert!(matches!(bare.as_deref(), Some([msi, ..]) if msi == "msiexec.exe"));
        // msiexec with extra args
        let msi = build_uninstall_command(
            r#"C:\Windows\System32\msiexec.exe /x {9A1B2C3D-1111-2222-3333-444455556666}"#,
            "",
            false,
        );
        assert!(msi.is_some());
        // Non-MSI uninstall that merely contains a GUID → quoted exe path
        let other = build_uninstall_command(
            r#""C:\Program Files\Vendor\App\unins000.exe" /GUID={9A1B2C3D-1111-2222-3333-444455556666}"#,
            "",
            false,
        )
        .expect("quoted exe");
        assert_eq!(other[0], r"C:\Program Files\Vendor\App\unins000.exe");
        assert!(!other[0].eq_ignore_ascii_case("msiexec.exe"));
    }

    #[test]
    fn store_uninstall_maps_to_powershell() {
        let cmd = build_uninstall_command(
            "remova-store:Microsoft.WindowsCalculator_10.2210.0.0_x64__8wekyb3d8bbwe",
            "",
            true,
        )
        .unwrap();
        assert_eq!(cmd[0], "powershell.exe");
        assert!(cmd.iter().any(|a| a.contains("Remove-AppxPackage")));
        assert!(cmd
            .iter()
            .any(|a| a.contains("Microsoft.WindowsCalculator_10.2210.0.0_x64__8wekyb3d8bbwe")));
    }

    #[test]
    fn msi_guid() {
        let cmd = build_uninstall_command(
            r"MsiExec.exe /X{12345678-1234-1234-1234-1234567890AB}",
            "",
            true,
        )
        .unwrap();
        assert_eq!(cmd[0].to_lowercase(), "msiexec.exe");
        assert!(
            cmd.contains(&"/x".to_string()) || cmd.iter().any(|x| x.eq_ignore_ascii_case("/x"))
        );
        assert!(cmd
            .iter()
            .any(|x| x.eq_ignore_ascii_case("{12345678-1234-1234-1234-1234567890AB}")));
    }

    #[test]
    fn quoted_exe() {
        let cmd =
            build_uninstall_command(r#""C:\Program Files\App\uninst.exe" /S /foo=bar"#, "", true)
                .unwrap();
        assert_eq!(cmd[0], r"C:\Program Files\App\uninst.exe");
        assert!(cmd.iter().any(|x| x == "/S"));
    }

    #[test]
    fn prefer_quiet() {
        let cmd =
            build_uninstall_command(r"C:\a\uninst.exe", r#""C:\a\uninst.exe" /S"#, true).unwrap();
        assert!(cmd.iter().any(|x| x == "/S"));
    }

    #[test]
    fn empty_none() {
        assert!(build_uninstall_command("", "", true).is_none());
    }

    #[test]
    fn official_uninstall_no_string() {
        let app = crate::apps::InstalledApp {
            name: "Ghost".into(),
            version: String::new(),
            publisher: String::new(),
            install_location: String::new(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let r = run_official_uninstall(&app);
        assert!(!r.had_command);
        assert!(!r.ok);
        assert_eq!(r.message, "no uninstall string");
    }

    #[test]
    fn dry_run_counts_registry_safe() {
        let items = vec![CleanupItem {
            path: r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{ABC}".into(),
            kind: ItemKind::Registry,
            score: 90,
            confidence: crate::scanner::Confidence::Confirmed,
            risk: crate::scanner::RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            size_kb: None,
            bucket: None,
        }];
        let r = run_cleanup_dry("App", &items);
        assert!(r.dry_run);
        assert_eq!(r.deleted_planned, 1);
        assert_eq!(r.skipped, 0);
    }

    #[test]
    fn dry_run_skips_critical_service() {
        let items = vec![CleanupItem {
            path: r"HKLM64\SYSTEM\CurrentControlSet\Services\Winmgmt".into(),
            kind: ItemKind::Registry,
            score: 40,
            confidence: crate::scanner::Confidence::Suspected,
            risk: crate::scanner::RiskLevel::High,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            size_kb: None,
            bucket: None,
        }];
        let r = run_cleanup_dry("App", &items);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.deleted_planned, 0);
    }
}
