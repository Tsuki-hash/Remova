//! Backup →official uninstall →per-item delete.

use crate::scanner::{CleanupItem, ItemKind};
use std::path::Path;

use super::preview::run_cleanup_dry_for_app_source;
use super::uninstall::run_official_uninstall;
use super::{cleanup_source_from_opts, FullCleanupOptions, FullCleanupReport, ItemDetail};

/// Process-wide lock so batch + manual cleanup cannot race PATH/backup.
static CLEANUP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    // Backup session is optional (product opt-in). Restore point stays decoupled.
    if !opts.backup_enabled {
        return BackupOutcome::Ready {
            backup_dir: String::new(),
            restore_point_ok,
            restore_point_msg,
        };
    }
    match crate::backup::create_session(&app.name) {
        Ok(session) => {
            let backup_dir = session.to_string_lossy().to_string();
            // Only backup items that pass the cleanup gate.
            let ignore = crate::ignore::load();
            let source = cleanup_source_from_opts(opts);
            let allow: Vec<CleanupItem> = items
                .iter()
                .filter(|it| {
                    crate::policy::gate_cleanup_item(Some(app), it, source, &ignore).is_allow()
                })
                .cloned()
                .collect();
            let (_ok, fail, errors) = crate::backup::backup_items(&allow, &session);
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
                    delayed: 0,
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
            uninstall_message: "backup session failed".to_string(),
            deleted: 0,
            failed: 0,
            skipped: 0,
            delayed: 0,
            aborted: true,
            restore_point_ok,
            restore_point_msg,
            errors: vec![e.to_string()],
            item_details: vec![],
        })),
    }
}

/// Full scheduled-task name from a TaskCache\Tree registry path.
pub fn task_full_name_from_reg_path(reg_path: &str) -> String {
    let low = reg_path.replace('/', "\\");
    let marker = r"\TaskCache\Tree\";
    if let Some(pos) = low.to_lowercase().find(&marker.to_lowercase()) {
        let rest = &low[pos + marker.len()..];
        let rest = rest.trim_matches('\\');
        if !rest.is_empty() {
            return format!(r"\{rest}");
        }
    }
    crate::regops::leaf_name(reg_path)
}

/// Extract the SERVICE name from a `...\Services\<name>[\<subkey>]` registry
/// path (value names via `|` are stripped first). Subkeys like `Parameters`
/// are never service names — returning them would run `sc delete Parameters`.
pub fn service_name_from_reg_path(reg_path: &str) -> Option<String> {
    let key = reg_path.split('|').next().unwrap_or(reg_path);
    let segs: Vec<&str> = key.split(['\\', '/']).collect();
    let idx = segs
        .iter()
        .position(|s| s.eq_ignore_ascii_case("Services"))?;
    let name = segs.get(idx + 1)?;
    if name.is_empty() {
        return None;
    }
    Some((*name).to_string())
}

/// Native stop/delete note for service/task registry keys — anchored to the
/// canonical HKLM roots. A `contains`-style trigger would let a forged shape
/// like `HKCU\x\...\Services\Spooler` drive `sc delete` at a REAL service or
/// `schtasks /delete` at a system task; only normalized HKLM prefixes qualify.
/// Returns `(native_ok, note)` when a native path applies, else None.
fn native_delete_note(reg_path: &str) -> Option<(bool, String)> {
    // Native stop/delete is a KEY-shaped, TOP-LEVEL operation only:
    // - a value item (`Services\X|Start`) removes one value, never the whole
    //   service;
    // - a subkey item (`Services\X\Parameters`) is refused by the registry
    //   gate and must not widen into deleting the whole service;
    // - a multi-pipe shape can never resolve cleanly. All of them stay on the
    //   gated registry path below.
    if reg_path.contains('|') {
        return None;
    }
    let key_norm = crate::safety::normalize_hklm(reg_path);
    if key_norm.starts_with(r"HKLM\SYSTEM\CURRENTCONTROLSET\SERVICES\") {
        // The service name is the segment AFTER Services — a leaf like
        // `Parameters` is a subkey, never a service name; anything deeper
        // than the service key itself is not a whole-service deletion.
        let svc = service_name_from_reg_path(reg_path)?;
        let top_level = key_norm
            .eq_ignore_ascii_case(&format!(r"HKLM\SYSTEM\CURRENTCONTROLSET\SERVICES\{svc}"));
        if !top_level {
            return None;
        }
        let native_ok = crate::regops::sc_delete_service(&svc);
        return Some((
            native_ok,
            if native_ok {
                format!("sc delete {svc}: ok")
            } else {
                format!("sc delete {svc}: failed or not found")
            },
        ));
    }
    if key_norm
        .starts_with(r"HKLM\SOFTWARE\MICROSOFT\WINDOWS NT\CURRENTVERSION\SCHEDULE\TASKCACHE\TREE\")
    {
        // Use the full task path from the TaskCache tree when possible.
        let tn = task_full_name_from_reg_path(reg_path);
        let native_ok = crate::regops::schtasks_delete(&tn);
        return Some((
            native_ok,
            if native_ok {
                format!("schtasks delete {tn}: ok")
            } else {
                format!("schtasks delete {tn}: failed or not found")
            },
        ));
    }
    None
}

struct DeleteOutcome {
    deleted: u32,
    failed: u32,
    skipped: u32,
    delayed: u32,
    errors: Vec<String>,
    details: Vec<ItemDetail>,
}

/// Windows reparse point (junction/symlink) — refuse delete-through (TOCTOU).
/// Shared with backup/restore copy paths via `fsutil`.
use crate::fsutil::is_reparse_point;

fn delete_cleanup_items_source(
    app: &crate::apps::InstalledApp,
    items: &[CleanupItem],
    source: crate::policy::CleanupSource,
) -> DeleteOutcome {
    let mut deleted = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;
    let mut delayed = 0u32;
    let mut errors = vec![];
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
                    errors.push(format!("{}: path delete failed", it.path));
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: "path".into(),
                        status: "failed".into(),
                        message: e,
                    });
                }
            },
            ItemKind::Registry => {
                let native = native_delete_note(&it.path);
                let native_note = native.as_ref().map(|(_, n)| n.clone()).unwrap_or_default();
                // A successful native stop/delete removes the service (with
                // its key) or the task registration itself — the follow-up
                // registry delete would only fail "not found". That outcome
                // is the SUCCESS it is, not a failed item.
                let native_done = matches!(&native, Some((true, _)));
                // `key|Value` must resolve to a non-empty value name; a trailing `|`
                // must never fall through to deleting the whole key (Run root!).
                let res: Result<(), String> = if native_done {
                    Ok(())
                } else if it.path.contains('|') {
                    match crate::regops::split_value_path(&it.path) {
                        Some((k, v)) => crate::regops::delete_value(k, v),
                        None => Err("registry value name must not be empty".into()),
                    }
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
                    Err(_) => {
                        failed += 1;
                        let msg = if native_note.is_empty() {
                            "registry delete failed".to_string()
                        } else {
                            format!("{native_note}; registry delete failed")
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
                // A path that is already gone is not a successful delete.
                if !p.exists() {
                    skipped += 1;
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: format!("{:?}", it.kind).to_lowercase(),
                        status: "skipped".into(),
                        message: "path missing".into(),
                    });
                    continue;
                }
                // S-TOCTOU: refuse junction/symlink swaps — never follow reparse points on delete.
                if is_reparse_point(p) {
                    skipped += 1;
                    details.push(ItemDetail {
                        path: it.path.clone(),
                        kind: format!("{:?}", it.kind).to_lowercase(),
                        status: "skipped".into(),
                        message: "reparse point".into(),
                    });
                    continue;
                }
                // recursive delete that refuses to walk child reparse points
                // (std remove_dir_all can follow a junction swapped after the root check).
                let res = crate::fsutil::remove_tree_no_reparse(p);
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
                            delayed += 1;
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
    DeleteOutcome {
        deleted,
        failed,
        skipped,
        delayed,
        errors,
        details,
    }
}

/// Full cleanup: optional backup →official uninstall →residual delete.
pub fn run_full_cleanup(
    app: &crate::apps::InstalledApp,
    items: &[CleanupItem],
    opts: &FullCleanupOptions,
) -> FullCleanupReport {
    let _guard = CLEANUP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if opts.dry_run {
        let dry = run_cleanup_dry_for_app_source(app, items, cleanup_source_from_opts(opts));
        return FullCleanupReport {
            app_name: dry.app_name,
            dry_run: true,
            backup_dir: String::new(),
            uninstall_ok: false,
            uninstall_message: dry.uninstall_message,
            deleted: dry.deleted_planned,
            failed: 0,
            skipped: dry.skipped,
            delayed: 0,
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
    // (batch "no default-selectable residue" — should still remove the app).
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
            delayed: 0,
            aborted: true,
            restore_point_ok: false,
            restore_point_msg: String::new(),
            errors: vec![],
            item_details: vec![],
        };
    }

    let owned_selected: Vec<CleanupItem> = selected.iter().map(|it| (*it).clone()).collect();

    let (mut backup_dir, mut restore_point_ok, mut restore_point_msg) =
        match try_backup_phase(app, &owned_selected, opts) {
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
    let mut uninstall_had_command = false;
    if !opts.skip_official_uninstall {
        let official = run_official_uninstall(app);
        uninstall_ok = official.ok;
        uninstall_message = official.message;
        uninstall_had_command = official.had_command;
    }
    // when a vendor uninstaller existed and failed, do not silently wipe leftovers —
    // the main program may still be half-installed. Force-clean / skip_official remains available.
    if uninstall_had_command && !uninstall_ok {
        return FullCleanupReport {
            app_name: app.name.clone(),
            dry_run: false,
            backup_dir,
            uninstall_ok,
            uninstall_message,
            deleted: 0,
            failed: 0,
            skipped: 0,
            delayed: 0,
            aborted: true,
            restore_point_ok,
            restore_point_msg,
            errors: vec!["official uninstaller failed; leftover cleanup skipped".into()],
            item_details: vec![],
        };
    }

    // Only items with a real path enter backup/delete — the emptiness check
    // above already declares empty-path items invalid, and feeding them to
    // the deleter only manufactured spurious failures.
    let del = delete_cleanup_items_source(app, &owned_selected, cleanup_source_from_opts(opts));

    FullCleanupReport {
        app_name: app.name.clone(),
        dry_run: false,
        backup_dir,
        uninstall_ok,
        uninstall_message,
        deleted: del.deleted,
        failed: del.failed,
        skipped: del.skipped,
        delayed: del.delayed,
        aborted: false,
        restore_point_ok,
        restore_point_msg,
        errors: del.errors,
        item_details: del.details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_full_name_from_taskcache_tree() {
        assert_eq!(
            task_full_name_from_reg_path(
                r"HKLM64\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\Vendor\Foo\MyTask"
            ),
            r"\Vendor\Foo\MyTask"
        );
        assert_eq!(
            task_full_name_from_reg_path(r"HKLM\...\TaskCache\Tree\Simple"),
            r"\Simple"
        );
    }

    /// `sc delete` must target the segment after Services — subkeys like
    /// Parameters are service configuration, never a service name.
    #[test]
    fn service_name_taken_from_services_segment() {
        assert_eq!(
            service_name_from_reg_path(
                r"HKLM64\SYSTEM\CurrentControlSet\Services\VendorSvc\Parameters"
            )
            .as_deref(),
            Some("VendorSvc")
        );
        assert_eq!(
            service_name_from_reg_path(r"HKLM\SYSTEM\CurrentControlSet\Services\VendorSvc")
                .as_deref(),
            Some("VendorSvc")
        );
        // value items keep the key part only.
        assert_eq!(
            service_name_from_reg_path(r"HKLM\SYSTEM\CurrentControlSet\Services\VendorSvc|Start")
                .as_deref(),
            Some("VendorSvc")
        );
        // The Services root itself carries no service name.
        assert_eq!(
            service_name_from_reg_path(r"HKLM\SYSTEM\CurrentControlSet\Services"),
            None
        );
    }

    /// Native stop/delete triggers are anchored to canonical HKLM roots: a
    /// forged `HKCU\...\Services\Spooler`-shaped path must not even reach the
    /// native tools. (Red-state of this test ran `sc delete RemovaFakeSvc` —
    /// against a nonexistent service, so the red was harmless.)
    #[test]
    fn native_delete_requires_canonical_hklm_prefix() {
        assert_eq!(
            native_delete_note(r"HKCU\Software\x\SYSTEM\CURRENTCONTROLSET\SERVICES\RemovaFakeSvc"),
            None,
            "forged service shape must not trigger sc delete"
        );
        assert_eq!(
            native_delete_note(r"HKCU\Software\x\Schedule\TaskCache\Tree\Vendor\Foo"),
            None,
            "forged task shape must not trigger schtasks delete"
        );
        // Canonical shape reaches the arm; the intrinsic protected-service gate
        // refuses any real execution (no process is spawned for such a name).
        let note = native_delete_note(r"HKLM64\SYSTEM\CurrentControlSet\Services\Spooler");
        assert_eq!(
            note,
            Some((false, "sc delete Spooler: failed or not found".to_string()))
        );
        // A VALUE item removes one value, never the whole service — the native
        // arm must not fire for it (over-reach fixed in this round).
        assert_eq!(
            native_delete_note(r"HKLM\SYSTEM\CurrentControlSet\Services\Spooler|Start"),
            None
        );
        // A subkey item is gated to a registry refusal, not a whole-service
        // native deletion.
        assert_eq!(
            native_delete_note(r"HKLM64\SYSTEM\CurrentControlSet\Services\Spooler\Parameters"),
            None
        );
    }

    /// The native tools refuse critical services and the system task tree
    /// themselves. These asserts pin the refusal only — the red state of these
    /// calls would touch real system entries and was deliberately not executed.
    #[test]
    fn native_tools_refuse_critical_and_system_targets() {
        assert!(!crate::regops::sc_delete_service("Spooler"));
        assert!(!crate::regops::sc_delete_service("RpcSs"));
        // Microsoft-family services are write-protected — native deletion and
        // stop/start must refuse them equally.
        assert!(!crate::regops::sc_delete_service("MicrosoftEdgeUpdate"));
        assert!(crate::regops::sc_set_service_running("MicrosoftEdgeUpdate", false).is_err());
        assert!(crate::safety::is_safe_to_delete_registry(
            r"HKLM64\SYSTEM\CurrentControlSet\Services\MicrosoftEdgeUpdate"
        )
        .is_err());
        assert!(!crate::regops::schtasks_delete(
            r"\Microsoft\Windows\Defrag\ScheduledDefrag"
        ));
        assert!(!crate::regops::schtasks_delete(""));
    }

    #[test]
    fn missing_path_is_skipped_not_deleted() {
        // A full delete must not count non-existent File/Dir as deleted.
        let app = crate::apps::InstalledApp {
            name: "DemoApp".into(),
            publisher: "Vendor".into(),
            version: "1.0".into(),
            install_location: r"C:\Program Files\DemoApp".into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{DemoApp}"
                .into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let items = vec![CleanupItem {
            path: r"C:\Program Files\DemoApp\missing\gone.bin".into(),
            kind: ItemKind::File,
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
        let report = crate::executor::run_full_cleanup(
            &app,
            &items,
            &crate::executor::FullCleanupOptions {
                dry_run: false,
                skip_official_uninstall: true,
                backup_enabled: false,
                restore_point: false,
                cleanup_source: None,
            },
        );
        assert_eq!(report.deleted, 0);
        assert_eq!(report.skipped, 1);
        assert!(report
            .item_details
            .iter()
            .any(|d| d.status == "skipped" && d.message.contains("path missing")));
    }
}
