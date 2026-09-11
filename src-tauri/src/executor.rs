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
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let protected = [
        r"c:\windows",
        r"c:\programdata\microsoft",
        r"c:\program files\windowsapps",
        r"c:\program files\common files\microsoft shared",
    ];
    !protected
        .iter()
        .any(|pref| s == *pref || s.starts_with(&format!("{pref}\\")))
}

/// Dry-run cleanup: validate items and report what would happen. Never deletes.
pub fn run_cleanup_dry(app_name: &str, items: &[CleanupItem]) -> CleanupReport {
    let mut deleted_planned = 0u32;
    let mut skipped = 0u32;
    let mut errors = vec![];
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

#[cfg(test)]
mod tests {
    use super::*;

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
