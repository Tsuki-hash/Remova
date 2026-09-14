//! Uninstall command parsing and dry-run cleanup planning (Phase 2).

use crate::scanner::{CleanupItem, ItemKind};
use crate::safety::is_safe_to_delete_registry;
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

/// Build argv for the official uninstaller (parity with Python `build_uninstall_command`).
/// Store packages (`remova-store:<PackageFullName>`) map to PowerShell Remove-AppxPackage.
pub fn build_uninstall_command(uninstall_string: &str, quiet_uninstall: &str, prefer_quiet: bool) -> Option<Vec<String>> {
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

    // MSI product code
    if let Some(guid) = extract_guid(&raw) {
        let lower = raw.to_lowercase();
        if lower.contains("msiexec") || raw.trim_start().starts_with('{') {
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
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
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
    pub errors: Vec<String>,
    pub item_details: Vec<ItemDetail>,
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
            errors: dry.errors,
            item_details: dry.item_details,
        };
    }

    let selected: Vec<&CleanupItem> = items.iter().collect();
    if selected.is_empty() {
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
            errors: vec![],
            item_details: vec![],
        };
    }

    let mut backup_dir = String::new();
    if opts.backup_enabled {
        // restore point first (non-fatal)
        let (_rp_ok, _rp_msg) = crate::sysops::create_restore_point(&format!(
            "Remova: {}",
            app.name.chars().take(40).collect::<String>()
        ));
        match crate::backup::create_session(&app.name) {
            Ok(session) => {
                backup_dir = session.to_string_lossy().to_string();
                let (_ok, fail, errors) = crate::backup::backup_items(items, &session);
                if fail > 0 {
                    return FullCleanupReport {
                        app_name: app.name.clone(),
                        dry_run: false,
                        backup_dir,
                        uninstall_ok: false,
                        uninstall_message: format!("backup failed for {fail} item(s); aborted"),
                        deleted: 0,
                        failed: 0,
                        skipped: 0,
                        aborted: true,
                        errors,
                        item_details: vec![],
                    };
                }
            }
            Err(e) => {
                return FullCleanupReport {
                    app_name: app.name.clone(),
                    dry_run: false,
                    backup_dir: String::new(),
                    uninstall_ok: false,
                    uninstall_message: format!("backup session failed: {e}"),
                    deleted: 0,
                    failed: 0,
                    skipped: 0,
                    aborted: true,
                    errors: vec![e.to_string()],
                    item_details: vec![],
                };
            }
        }
    }

    let mut uninstall_ok = false;
    let mut uninstall_message = "skipped".into();
    if !opts.skip_official_uninstall {
        let cmd = build_uninstall_command(
            &app.uninstall_string,
            &app.quiet_uninstall_string,
            true,
        );
        match cmd {
            Some(argv) if !argv.is_empty() => {
                let mut parts = argv.iter();
                let exe = parts.next().unwrap().clone();
                let rest: Vec<String> = parts.cloned().collect();
                match std::process::Command::new(&exe).args(&rest).spawn() {
                    Ok(mut child) => {
                        // Wait for uninstaller so residual delete is not racy.
                        let timeout = std::time::Duration::from_secs(300);
                        let start = std::time::Instant::now();
                        loop {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    uninstall_ok = status.success();
                                    uninstall_message = if status.success() {
                                        format!("uninstaller finished: {}", argv[0])
                                    } else {
                                        format!(
                                            "uninstaller exited with {:?}",
                                            status.code()
                                        )
                                    };
                                    break;
                                }
                                Ok(None) => {
                                    if start.elapsed() >= timeout {
                                        let _ = child.kill();
                                        let _ = child.wait();
                                        uninstall_ok = false;
                                        uninstall_message =
                                            format!("uninstaller timed out (5m): {}", argv[0]);
                                        break;
                                    }
                                    std::thread::sleep(std::time::Duration::from_millis(200));
                                }
                                Err(e) => {
                                    uninstall_message = format!("wait failed: {e}");
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        uninstall_message = format!("launch failed: {e}");
                    }
                }
            }
            _ => uninstall_message = "no uninstall string".into(),
        }
    }

    let mut deleted = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;
    let mut errors = vec![];
    let mut details = vec![];

    for it in items {
        match it.kind {
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
                // Service / task: try native delete first
                let low = it.path.to_uppercase();
                if low.contains(r"\SYSTEM\CURRENTCONTROLSET\SERVICES\") {
                    let svc = crate::regops::leaf_name(&it.path);
                    let _ = crate::regops::sc_delete_service(&svc);
                } else if low.contains(r"\SCHEDULE\TASKCACHE\TREE\") {
                    let tn = crate::regops::leaf_name(&it.path);
                    let _ = crate::regops::schtasks_delete(&tn);
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
                            message: String::new(),
                        });
                    }
                    Err(e) => {
                        failed += 1;
                        errors.push(format!("{}: {e}", it.path));
                        details.push(ItemDetail {
                            path: it.path.clone(),
                            kind: "registry".into(),
                            status: "failed".into(),
                            message: e,
                        });
                    }
                }
            }
            _ => {
                let p = Path::new(&it.path);
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
        errors,
        item_details: details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(cmd.contains(&"/x".to_string()) || cmd.iter().any(|x| x.eq_ignore_ascii_case("/x")));
        assert!(cmd
            .iter()
            .any(|x| x.eq_ignore_ascii_case("{12345678-1234-1234-1234-1234567890AB}")));
    }

    #[test]
    fn quoted_exe() {
        let cmd = build_uninstall_command(r#""C:\Program Files\App\uninst.exe" /S /foo=bar"#, "", true)
            .unwrap();
        assert_eq!(cmd[0], r"C:\Program Files\App\uninst.exe");
        assert!(cmd.iter().any(|x| x == "/S"));
    }

    #[test]
    fn prefer_quiet() {
        let cmd = build_uninstall_command(
            r"C:\a\uninst.exe",
            r#""C:\a\uninst.exe" /S"#,
            true,
        )
        .unwrap();
        assert!(cmd.iter().any(|x| x == "/S"));
    }

    #[test]
    fn empty_none() {
        assert!(build_uninstall_command("", "", true).is_none());
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
        }];
        let r = run_cleanup_dry("App", &items);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.deleted_planned, 0);
    }
}
