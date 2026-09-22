//! Dry-run (preview-only) cleanup plan 鈥?shares the real delete gates, writes nothing.

use crate::scanner::{CleanupItem, ItemKind};
use std::path::Path;

use super::{CleanupReport, ItemDetail};

/// Dry-run with the same semantic gates as full delete when `app` is known (S-05 / A-02).
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
    let ignore = crate::ignore::load();

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
        // S-R4-08: align dry-run outcome probes with full delete.
        match it.kind {
            ItemKind::File | ItemKind::Dir => {
                if !Path::new(&it.path).exists() {
                    skipped += 1;
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: format!("{:?}", it.kind).to_lowercase(),
                        status: "skipped".into(),
                        message: "path missing".into(),
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
        dry_run: true,
        uninstall_command: vec![],
        uninstall_message: "dry-run: official uninstaller not launched".into(),
        deleted_planned,
        skipped,
        errors,
        item_details: details,
    }
}

/// Legacy dry-run without app context 鈥?still shares policy safety/user_data gates (A-N3).
/// Prefer `run_cleanup_dry_for_app` when an InstalledApp is known.
pub fn run_cleanup_dry(app_name: &str, items: &[CleanupItem]) -> CleanupReport {
    let ignore = crate::ignore::load();
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
        let ok = decision.is_allow()
            && match it.kind {
                ItemKind::File | ItemKind::Dir => Path::new(&it.path).exists(),
                ItemKind::Path => path_entry_in_system_path(&it.path),
                _ => true,
            };
        if !ok {
            skipped += 1;
            details.push(ItemDetail {
                path: it.path.clone(),
                kind: format!("{:?}", it.kind).to_lowercase(),
                status: "skipped".into(),
                message: if decision.is_allow() {
                    if matches!(it.kind, ItemKind::File | ItemKind::Dir)
                        && !Path::new(&it.path).exists()
                    {
                        "path missing".into()
                    } else if matches!(it.kind, ItemKind::Path)
                        && !path_entry_in_system_path(&it.path)
                    {
                        "not found in PATH".into()
                    } else {
                        "failed safety gate".into()
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
            user_data: false,
            user_library: false,
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
            user_library: false,
            size_kb: None,
            bucket: None,
        }];
        let r = run_cleanup_dry("App", &items);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.deleted_planned, 0);
    }
}
