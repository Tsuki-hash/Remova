//! Dry-run (preview-only) cleanup plan —shares the real delete gates, writes nothing.

use crate::scanner::{CleanupItem, ItemKind};
use std::path::Path;

use super::{CleanupReport, ItemDetail};

/// Dry-run with the same semantic gates as full delete when `app` is known.
pub fn run_cleanup_dry_for_app(
    app: &crate::apps::InstalledApp,
    items: &[CleanupItem],
) -> CleanupReport {
    run_cleanup_dry_for_app_source(app, items, crate::policy::CleanupSource::Uninstall)
}

pub fn run_cleanup_dry_for_app_source(
    app: &crate::apps::InstalledApp,
    items: &[CleanupItem],
    source: crate::policy::CleanupSource,
) -> CleanupReport {
    let mut deleted_planned = 0u32;
    let mut skipped = 0u32;
    let errors = vec![];
    let mut details = vec![];
    // the preview must not answer a damaged ignore list with a
    // wider list than the run that follows. `load()` swallows the read error.
    let ignore = match crate::ignore::load_checked() {
        Ok(ignore) => ignore,
        Err(e) => return ignore_read_abort_dry(app.name.clone(), e),
    };

    for it in items {
        let decision = crate::policy::gate_cleanup_item(Some(app), it, source, &ignore);
        if !decision.is_allow() {
            skipped += 1;
            details.push(ItemDetail {
                path: it.path.clone(),
                kind: format!("{:?}", it.kind).to_lowercase(),
                status: "skipped".into(),
                message: decision.message().to_string(),
            });
            continue;
        }
        // align dry-run outcome probes with full delete: a probe error is
        // "cannot tell" and must not be planned as deletable.
        match it.kind {
            ItemKind::File | ItemKind::Dir => {
                let message = match Path::new(&it.path).try_exists() {
                    Ok(true) => None,
                    Ok(false) => Some("path missing"),
                    Err(_) => Some("access denied"),
                };
                if let Some(message) = message {
                    skipped += 1;
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: format!("{:?}", it.kind).to_lowercase(),
                        status: "skipped".into(),
                        message: message.into(),
                    });
                    continue;
                }
            }
            ItemKind::Path if !path_entry_in_system_path(&it.path) => {
                skipped += 1;
                details.push(ItemDetail {
                    path: it.path.clone(),
                    kind: "path".into(),
                    status: "skipped".into(),
                    message: "not found in PATH".into(),
                });
                continue;
            }
            _ => {}
        }
        deleted_planned += 1;
        details.push(ItemDetail {
            path: it.path.clone(),
            kind: format!("{:?}", it.kind).to_lowercase(),
            status: "planned".into(),
            message: "[dry-run]".into(),
        });
    }

    CleanupReport {
        app_name: app.name.clone(),
        app_key: crate::apps::app_key(app),
        dry_run: true,
        uninstall_command: vec![],
        uninstall_message: "dry-run: official uninstaller not launched".into(),
        deleted_planned,
        skipped,
        errors,
        item_details: details,
    }
}

/// Legacy dry-run without app context —still shares policy safety/user_data gates (A-N3).
/// Prefer `run_cleanup_dry_for_app` when an InstalledApp is known.
pub fn run_cleanup_dry(app_name: &str, items: &[CleanupItem]) -> CleanupReport {
    let ignore = match crate::ignore::load_checked() {
        Ok(ignore) => ignore,
        Err(e) => return ignore_read_abort_dry(app_name.to_string(), e),
    };
    let mut deleted_planned = 0u32;
    let mut skipped = 0u32;
    let errors = vec![];
    let mut details = vec![];

    for it in items {
        let decision = crate::policy::gate_cleanup_item(
            None,
            it,
            crate::policy::CleanupSource::Uninstall,
            &ignore,
        );
        let path_probe = match it.kind {
            ItemKind::File | ItemKind::Dir => Some(Path::new(&it.path).try_exists()),
            _ => None,
        };
        let ok = decision.is_allow()
            && match path_probe {
                Some(Ok(true)) => true,
                Some(_) => false,
                None => match it.kind {
                    ItemKind::Path => path_entry_in_system_path(&it.path),
                    _ => true,
                },
            };
        if !ok {
            skipped += 1;
            details.push(ItemDetail {
                path: it.path.clone(),
                kind: format!("{:?}", it.kind).to_lowercase(),
                status: "skipped".into(),
                message: if decision.is_allow() {
                    match path_probe {
                        Some(Ok(false)) => "path missing".into(),
                        Some(Err(_)) => "access denied".into(),
                        _ if matches!(it.kind, ItemKind::Path)
                            && !path_entry_in_system_path(&it.path) =>
                        {
                            "not found in PATH".into()
                        }
                        _ => "failed safety gate".into(),
                    }
                } else {
                    decision.message().to_string()
                },
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

    CleanupReport {
        app_name: app_name.to_string(),
        app_key: String::new(),
        dry_run: true,
        uninstall_command: vec![],
        uninstall_message: "dry-run: official uninstaller not launched".into(),
        deleted_planned,
        skipped,
        errors,
        item_details: details,
    }
}

/// True when a PATH leftover segment is present in User/Machine PATH (same source as scrub).
fn path_entry_in_system_path(entry: &str) -> bool {
    crate::regops::scrub_path_entry_ok(entry)
}

/// A preview stops, like the real run, when the ignore list cannot be read.
/// Named apart from pipeline.rs's FullCleanupReport variant on purpose: the
/// two share a purpose, not a return type.
fn ignore_read_abort_dry(app_name: String, error: String) -> CleanupReport {
    CleanupReport {
        app_name,
        app_key: String::new(),
        dry_run: true,
        uninstall_command: vec![],
        uninstall_message: error.clone(),
        deleted_planned: 0,
        skipped: 0,
        errors: vec![error],
        item_details: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
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
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        }];
        let r = run_cleanup_dry("App", &items);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.deleted_planned, 0);
    }
}
