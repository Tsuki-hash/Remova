//! Backup →official uninstall →per-item delete.

use crate::scanner::{CleanupItem, ItemKind};
use std::path::Path;

use super::preview::run_cleanup_dry_for_app_source;
use super::uninstall::run_official_uninstall_locked;
use super::{
    cleanup_source_from_opts, FullCleanupOptions, FullCleanupReport, ItemDetail, CLEANUP_LOCK,
};

/// Stage 1: restore point + Safety Vault backup.
enum BackupOutcome {
    Ready {
        backup_dir: String,
        restore_point_ok: bool,
        restore_point_msg: String,
    },
    Abort(Box<FullCleanupReport>),
}

/// The gate decision for every selected item, taken once.
///
/// The backup phase and the delete phase used to run `gate_cleanup_item`
/// separately, with up to 300s of vendor uninstaller in between and mutable
/// global state on both sides (the ignore list is re-read from disk, and a
/// concurrent scan can replace an allow-list). An item refused at backup time
/// could therefore be allowed at delete time and be deleted with no backup.
/// One decision, shared by both phases, closes that window; the delete path
/// takes the allowed set as given rather than deciding again.
struct CleanupPlan {
    /// Items that passed the gate — exactly what is backed up and deleted.
    allowed: Vec<CleanupItem>,
    /// Items the gate refused, with the reason, reported as `skipped`.
    refused: Vec<ItemDetail>,
}

/// One constructor for aborted reports: every abort has zero counters and no
/// item details, so copying 20 fields per branch only invited semantic drift.
#[allow(clippy::too_many_arguments)]
fn aborted_report(
    app: &crate::apps::InstalledApp,
    backup_dir: String,
    uninstall_ok: bool,
    uninstall_message: String,
    reboot_pending: bool,
    restore_point_ok: bool,
    restore_point_msg: String,
    errors: Vec<String>,
) -> FullCleanupReport {
    FullCleanupReport {
        app_name: app.name.clone(),
        app_key: crate::apps::app_key(app),
        dry_run: false,
        backup_dir,
        uninstall_ok,
        uninstall_message,
        deleted: 0,
        failed: 0,
        skipped: 0,
        delayed: 0,
        reclaimed_kb: 0,
        kept_kb: 0,
        delayed_kb: 0,
        aborted: true,
        reboot_pending,
        restore_point_ok,
        restore_point_msg,
        errors,
        item_details: vec![],
    }
}

fn plan_cleanup_items(
    app: &crate::apps::InstalledApp,
    items: &[CleanupItem],
    source: crate::policy::CleanupSource,
    ignore: &crate::ignore::IgnoreList,
) -> CleanupPlan {
    let mut allowed = Vec::new();
    let mut refused = Vec::new();
    for it in items {
        let decision = crate::policy::gate_cleanup_item(Some(app), it, source, ignore);
        if decision.is_allow() {
            allowed.push(it.clone());
        } else {
            refused.push(ItemDetail {
                path: it.path.clone(),
                kind: format!("{:?}", it.kind).to_lowercase(),
                status: "skipped".into(),
                message: decision.message().to_string(),
            });
        }
    }
    CleanupPlan { allowed, refused }
}

fn try_backup_phase(
    app: &crate::apps::InstalledApp,
    plan: &CleanupPlan,
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
    // F-18: a run with nothing to back up (e.g. uninstall-only) must not leave
    // an empty sealed session in the Safety Vault.
    if plan.allowed.is_empty() {
        return BackupOutcome::Ready {
            backup_dir: String::new(),
            restore_point_ok,
            restore_point_msg,
        };
    }
    match crate::backup::create_session(&app.name) {
        Ok(session) => {
            let backup_dir = session.to_string_lossy().to_string();
            // Only items that pass the cleanup gate — the same set the delete
            // phase will act on, decided once and above.
            let (_ok, fail, errors) = crate::backup::backup_items(&plan.allowed, &session);
            if fail > 0 {
                return BackupOutcome::Abort(Box::new(aborted_report(
                    app,
                    backup_dir,
                    false,
                    format!("backup failed for {fail} item(s); aborted"),
                    false,
                    restore_point_ok,
                    restore_point_msg,
                    errors,
                )));
            }
            BackupOutcome::Ready {
                backup_dir,
                restore_point_ok,
                restore_point_msg,
            }
        }
        Err(e) => BackupOutcome::Abort(Box::new(aborted_report(
            app,
            String::new(),
            false,
            "backup session failed".to_string(),
            false,
            restore_point_ok,
            restore_point_msg,
            vec![e.to_string()],
        ))),
    }
}

/// Full scheduled-task name from a TaskCache\Tree registry path.
pub fn task_full_name_from_reg_path(reg_path: &str) -> String {
    let slashed = reg_path.replace('/', "\\");
    let marker = r"\TaskCache\Tree\";
    // F-11: matching against a lowercased copy and reusing that byte offset on
    // the original string is wrong — `to_lowercase` can change byte lengths.
    if let Some(pos) = find_ascii_case_insensitive(&slashed, marker) {
        let rest = &slashed[pos + marker.len()..];
        let rest = rest.trim_matches('\\');
        if !rest.is_empty() {
            return format!(r"\{rest}");
        }
    }
    crate::regops::leaf_name(reg_path)
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    h.windows(n.len()).position(|w| w.eq_ignore_ascii_case(n))
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

/// Delete exactly the items the gate allowed.
///
/// Takes the pre-computed allowed set rather than gating again: the gate's answer
/// depends on mutable global state (the ignore list on disk, the scan
/// allow-lists), so a second decision after the uninstaller could let an item
/// through that was refused — and therefore never backed up — moments earlier.
fn delete_planned_items_with_native(
    items: &[CleanupItem],
    native_delete: &mut impl FnMut(&str) -> Option<(bool, String)>,
) -> DeleteOutcome {
    delete_planned_items_with_io(
        items,
        native_delete,
        &mut crate::fsutil::remove_tree_no_reparse,
        &mut crate::sysops::schedule_delete_on_reboot,
    )
}

fn delete_planned_items_with_io(
    items: &[CleanupItem],
    native_delete: &mut impl FnMut(&str) -> Option<(bool, String)>,
    delete_fs: &mut impl FnMut(&Path) -> std::io::Result<()>,
    schedule: &mut impl FnMut(&str) -> bool,
) -> DeleteOutcome {
    let mut deleted = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;
    let mut delayed = 0u32;
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
                let native = native_delete(&it.path);
                let native_note = native.as_ref().map(|(_, n)| n.clone()).unwrap_or_default();
                // A successful native stop/delete removes the service (with
                // its key) or the task registration itself — the follow-up
                // registry delete would only fail "not found". That outcome
                // is the SUCCESS it is, not a failed item.
                let native_done = matches!(&native, Some((true, _)));
                // `key|Value` must resolve to a non-empty value name; a trailing `|`
                // must never fall through to deleting the whole key (Run root!).
                let res: Result<bool, String> = if native_done {
                    Ok(true)
                } else if it.path.contains('|') {
                    match crate::regops::split_value_path(&it.path) {
                        Some((k, v)) => crate::regops::delete_value(k, v),
                        None => Err("registry value name must not be empty".into()),
                    }
                } else {
                    crate::regops::delete_key(&it.path)
                };
                match res {
                    Ok(true) => {
                        deleted += 1;
                        details.push(ItemDetail {
                            path: it.path.clone(),
                            kind: "registry".into(),
                            status: "deleted".into(),
                            message: native_note,
                        });
                    }
                    // The uninstaller already removed it. Same family as the
                    // File/Dir "path missing" rule, and the same outcome the
                    // dry-run records — a row that is already gone is not a
                    // failure the user has to act on.
                    Ok(false) => {
                        skipped += 1;
                        details.push(ItemDetail {
                            path: it.path.clone(),
                            kind: "registry".into(),
                            status: "skipped".into(),
                            message: "registry entry missing".into(),
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
                // F-10: `exists()` maps every metadata error to false, so an
                // ACL-denied or offline path was reported as "already gone".
                match p.try_exists() {
                    Ok(false) => {
                        skipped += 1;
                        details.push(ItemDetail {
                            path: it.path.clone(),
                            kind: format!("{:?}", it.kind).to_lowercase(),
                            status: "skipped".into(),
                            message: "path missing".into(),
                        });
                        continue;
                    }
                    Ok(true) => {}
                    Err(e) => {
                        failed += 1;
                        let msg = format!("cannot stat path: {e}");
                        errors.push(format!("{}: {msg}", it.path));
                        details.push(ItemDetail {
                            path: it.path.clone(),
                            kind: format!("{:?}", it.kind).to_lowercase(),
                            status: "failed".into(),
                            message: msg,
                        });
                        continue;
                    }
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
                let res = delete_fs(p);
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
                    Err(error) => {
                        // try schedule delete on reboot for locked files
                        if schedule(&it.path) {
                            delayed += 1;
                            details.push(ItemDetail {
                                path: it.path.clone(),
                                kind: format!("{:?}", it.kind).to_lowercase(),
                                status: "delayed".into(),
                                message: "reboot delete".into(),
                            });
                        } else {
                            failed += 1;
                            errors.push(format!("{}: delete failed: {error}", it.path));
                            details.push(ItemDetail {
                                path: it.path.clone(),
                                kind: format!("{:?}", it.kind).to_lowercase(),
                                status: "failed".into(),
                                message: error.to_string(),
                            });
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

/// Sum item sizes without double-counting a child when its parent is also in
/// the set (both can be selected from different scan phases). Registry/path
/// rows have no size and never contribute.
fn sum_sizes_without_nested<'a>(items: impl Iterator<Item = &'a CleanupItem>) -> u64 {
    let mut rows: Vec<&CleanupItem> = items.filter(|it| it.size_kb.is_some()).collect();
    rows.sort_by_key(|it| it.path.len());
    let mut roots: Vec<String> = Vec::new();
    let mut sum = 0u64;
    for item in rows {
        let key = crate::fsutil::path_key(std::path::Path::new(&item.path))
            .unwrap_or_else(|| item.path.to_lowercase());
        if roots
            .iter()
            .any(|root| key == *root || key.starts_with(&format!("{root}\\")))
        {
            continue;
        }
        roots.push(key);
        sum = sum.saturating_add(item.size_kb.unwrap_or(0));
    }
    sum
}

/// Full cleanup: optional backup →official uninstall →residual delete.
fn ignore_read_abort(
    app: &crate::apps::InstalledApp,
    dry_run: bool,
    error: String,
) -> FullCleanupReport {
    FullCleanupReport {
        app_name: app.name.clone(),
        app_key: crate::apps::app_key(app),
        dry_run,
        backup_dir: String::new(),
        uninstall_ok: false,
        uninstall_message: error.clone(),
        deleted: 0,
        failed: 0,
        skipped: 0,
        delayed: 0,
        reclaimed_kb: 0,
        kept_kb: 0,
        delayed_kb: 0,
        aborted: true,
        reboot_pending: false,
        restore_point_ok: false,
        restore_point_msg: String::new(),
        errors: vec![error],
        item_details: vec![],
    }
}

pub fn run_full_cleanup(
    app: &crate::apps::InstalledApp,
    items: &[CleanupItem],
    opts: &FullCleanupOptions,
) -> FullCleanupReport {
    let _guard = CLEANUP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // One ignore-list read for the whole run: the backup and delete phases must
    // not re-read mutable global state between them.
    let ignore = match crate::ignore::load_checked() {
        Ok(ignore) => ignore,
        Err(e) => return ignore_read_abort(app, opts.dry_run, e),
    };
    if opts.dry_run {
        let dry = run_cleanup_dry_for_app_source(app, items, cleanup_source_from_opts(opts));
        return FullCleanupReport {
            app_name: dry.app_name,
            app_key: dry.app_key,
            dry_run: true,
            backup_dir: String::new(),
            uninstall_ok: false,
            uninstall_message: dry.uninstall_message,
            deleted: dry.deleted_planned,
            failed: 0,
            skipped: dry.skipped,
            delayed: 0,
            reclaimed_kb: 0,
            kept_kb: 0,
            delayed_kb: 0,
            aborted: false,
            reboot_pending: false,
            restore_point_ok: false,
            restore_point_msg: String::new(),
            errors: dry.errors,
            item_details: dry.item_details,
        };
    }

    // A 1641 in this session has already started a reboot. Deleting anything
    // before that reboot finishes can leave a half-completed state, so every
    // cleanup fails closed, regardless of which app reported the 1641.
    if super::reboot::any_pending() {
        return aborted_report(
            app,
            String::new(),
            true,
            super::reboot::PENDING_MESSAGE.into(),
            true,
            false,
            String::new(),
            vec![],
        );
    }

    let selected: Vec<&CleanupItem> = items
        .iter()
        .filter(|it| !it.path.trim().is_empty())
        .collect();
    // Empty leftover set is valid when the user asked for official uninstall only
    // (batch "no default-selectable residue" — should still remove the app).
    if selected.is_empty() && opts.skip_official_uninstall {
        return aborted_report(
            app,
            String::new(),
            false,
            "no items".into(),
            false,
            false,
            String::new(),
            vec![],
        );
    }

    // A new cleanup replaces the lock-end authorization; stale failed paths
    // must never remain valid across reports.
    crate::locks::remember_failed_lock_roots(&app.install_location, &[]);
    let owned_selected: Vec<CleanupItem> = selected.iter().map(|it| (*it).clone()).collect();
    // The single gate decision, shared by the backup phase and the delete phase.
    let plan = plan_cleanup_items(
        app,
        &owned_selected,
        cleanup_source_from_opts(opts),
        &ignore,
    );

    let (backup_dir, restore_point_ok, restore_point_msg) = match try_backup_phase(app, &plan, opts)
    {
        BackupOutcome::Ready {
            backup_dir,
            restore_point_ok,
            restore_point_msg,
        } => (backup_dir, restore_point_ok, restore_point_msg),
        BackupOutcome::Abort(report) => return *report,
    };

    let mut uninstall_ok = false;
    let mut uninstall_message = "skipped".into();
    let mut uninstall_had_command = false;
    if !opts.skip_official_uninstall {
        let official = run_official_uninstall_locked(app);
        uninstall_ok = official.ok;
        uninstall_message = official.message.clone();
        uninstall_had_command = official.had_command;
        // MSI 1641 means the uninstaller already initiated a reboot. The product
        // was removed, but continuing file/registry deletes while shutdown is
        // pending can leave the cleanup half-done; ask the user to rerun after
        // reboot instead.
        if official.reboot_initiated {
            // Distinct from the 3010 "reboot required" message: this one
            // tells the user residual deletion was skipped on purpose.
            return aborted_report(
                app,
                backup_dir,
                true,
                super::reboot::INITIATED_MESSAGE.into(),
                true,
                restore_point_ok,
                restore_point_msg,
                vec![],
            );
        }
    }
    // when a vendor uninstaller existed and failed, do not silently wipe leftovers —
    // the main program may still be half-installed. Force-clean / skip_official remains available.
    if uninstall_had_command && !uninstall_ok {
        return aborted_report(
            app,
            backup_dir,
            uninstall_ok,
            uninstall_message,
            false,
            restore_point_ok,
            restore_point_msg,
            vec!["official uninstaller failed; leftover cleanup skipped".into()],
        );
    }

    // Only items with a real path enter backup/delete — the emptiness check
    // above already declares empty-path items invalid, and feeding them to
    // the deleter only manufactured spurious failures. The set deleted is the
    // set that was backed up, by construction.
    let mut del = delete_planned_items_with_native(&plan.allowed, &mut native_delete_note);
    // M2 (SOP §8-22): drop empty vendor/product shells left by the delete.
    let tokens: Vec<String> = [app.name.clone(), app.publisher.clone()]
        .into_iter()
        .filter(|t| !t.trim().is_empty())
        .collect();
    for detail in del.details.iter_mut() {
        if detail.status == "deleted" && detail.kind == "dir" {
            let removed =
                crate::fsutil::prune_empty_ancestors(std::path::Path::new(&detail.path), &tokens);
            if !removed.is_empty() {
                let names: Vec<String> = removed
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                let note = format!("removed empty parent: {}", names.join(", "));
                detail.message = if detail.message.is_empty() {
                    note
                } else {
                    format!("{}; {note}", detail.message)
                };
            }
        }
    }
    // M5: report what was actually freed and what the gate kept, by size.
    let deleted_paths: std::collections::HashSet<&str> = del
        .details
        .iter()
        .filter(|d| d.status == "deleted")
        .map(|d| d.path.as_str())
        .collect();
    let reclaimed_kb = sum_sizes_without_nested(
        plan.allowed
            .iter()
            .filter(|it| deleted_paths.contains(it.path.as_str())),
    );
    let allowed_paths: std::collections::HashSet<&str> =
        plan.allowed.iter().map(|it| it.path.as_str()).collect();
    let kept_kb = sum_sizes_without_nested(
        owned_selected
            .iter()
            .filter(|it| !allowed_paths.contains(it.path.as_str())),
    );
    let delayed_paths: std::collections::HashSet<&str> = del
        .details
        .iter()
        .filter(|d| d.status == "delayed")
        .map(|d| d.path.as_str())
        .collect();
    let delayed_kb = sum_sizes_without_nested(
        plan.allowed
            .iter()
            .filter(|it| delayed_paths.contains(it.path.as_str())),
    );
    del.skipped += plan.refused.len() as u32;
    del.details.extend(plan.refused);
    let failed_lock_paths: Vec<(String, bool)> = del
        .details
        .iter()
        .filter(|d| {
            d.status == "failed"
                && (!app.install_location.trim().is_empty()
                    || matches!(d.kind.as_str(), "file" | "dir"))
        })
        .map(|d| (d.path.clone(), d.kind == "dir"))
        .collect();
    crate::locks::remember_failed_lock_roots(&app.install_location, &failed_lock_paths);

    FullCleanupReport {
        app_name: app.name.clone(),
        app_key: crate::apps::app_key(app),
        dry_run: false,
        backup_dir,
        uninstall_ok,
        uninstall_message,
        deleted: del.deleted,
        failed: del.failed,
        skipped: del.skipped,
        delayed: del.delayed,
        reclaimed_kb,
        kept_kb,
        delayed_kb,
        aborted: false,
        reboot_pending: false,
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
    fn report_totals_do_not_double_count_nested_paths() {
        let mk = |path: &str, kb: Option<u64>| CleanupItem {
            path: path.into(),
            kind: ItemKind::Dir,
            score: 90,
            confidence: crate::scanner::Confidence::Confirmed,
            risk: crate::scanner::RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: kb,
            size_capped: false,
            bucket: None,
        };
        let items = [
            mk(r"C:\Apps\Demo", Some(100)),
            mk(r"C:\Apps\Demo\cache", Some(40)),
            mk(r"C:\Apps\Other", Some(10)),
            mk(r"HKCU\Software\Demo", None),
        ];
        assert_eq!(super::sum_sizes_without_nested(items.iter()), 110);
    }

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
        // F-11: `İ` grows under to_lowercase, so an offset measured in the
        // lowercased copy used to slice the original string at the wrong byte.
        assert_eq!(
            task_full_name_from_reg_path("HKLM64\\İ\\TaskCache\\Tree\\Vendor\\Foo\\MyTask"),
            r"\Vendor\Foo\MyTask"
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
    fn pending_reboot_blocks_residual_cleanup_across_calls() {
        // MSI 1641 happens in one IPC call; any later cleanup is a separate
        // call. The gate is global, so cleaning *another* app must also abort
        // before backup/delete, not just the app that reported the 1641.
        let marked = crate::apps::InstalledApp {
            name: "RebootPendingFixture".into(),
            publisher: "Vendor".into(),
            version: "1.0".into(),
            install_location: r"C:\Program Files\RebootPendingFixture".into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key:
                r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\RebootPendingFixture"
                    .into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let other = crate::apps::InstalledApp {
            name: "OtherCleanupFixture".into(),
            registry_key:
                r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\OtherCleanupFixture"
                    .into(),
            ..marked.clone()
        };
        // A real, deletable file: the gate must abort before touching it. An
        // empty item list could not prove the delete phase was skipped.
        let tmp = crate::test_support::TempDir::new("pending-gate");
        let dir = tmp.path().join("OtherCleanupFixture");
        std::fs::create_dir_all(&dir).unwrap();
        let payload = dir.join("residual.bin");
        std::fs::write(&payload, b"must survive the pending gate").unwrap();
        let item = crate::scanner::CleanupItem {
            path: dir.to_string_lossy().into_owned(),
            kind: crate::scanner::ItemKind::Dir,
            score: 90,
            confidence: crate::scanner::Confidence::Confirmed,
            risk: crate::scanner::RiskLevel::Low,
            reason: "fixture".into(),
            evidence: vec![],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        };
        crate::executor::reboot::reset_for_test();
        crate::executor::reboot::mark_pending(&crate::apps::app_key(&marked));
        let opts = FullCleanupOptions {
            dry_run: false,
            skip_official_uninstall: true,
            backup_enabled: true,
            restore_point: false,
            cleanup_source: Some("uninstall".into()),
        };
        let report = run_full_cleanup(&other, &[item], &opts);
        assert!(report.aborted);
        assert!(report.uninstall_ok);
        assert_eq!(
            report.uninstall_message,
            crate::executor::reboot::PENDING_MESSAGE
        );
        assert_eq!((report.deleted, report.failed), (0, 0));
        assert!(
            report.backup_dir.is_empty(),
            "backup must not run while a reboot is pending"
        );
        assert!(
            payload.exists(),
            "the pending gate must block the delete phase, not just the report"
        );
        crate::executor::reboot::reset_for_test();
        // 3010 (reboot merely requested) must not arm the gate at all.
        assert!(!crate::executor::reboot::any_pending());
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
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        }];
        let mut native_items = items.clone();
        let mut native_app = app.clone();
        native_app.name = format!(
            "RemovaNativeCountFixture{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        native_items[0].path = format!(
            r"HKLM64\SYSTEM\CurrentControlSet\Services\{}",
            native_app.name
        );
        native_items[0].kind = ItemKind::Registry;
        let mut calls = 0;
        let native = delete_planned_items_with_native(&native_items, &mut |_| {
            calls += 1;
            Some((true, "native service deleted".into()))
        });
        assert_eq!(calls, 1);
        assert_eq!((native.deleted, native.failed, native.skipped), (1, 0, 0));
        assert_eq!(native.details[0].status, "deleted");
        let fixture = std::env::temp_dir().join(&native_app.name);
        std::fs::create_dir(&fixture).unwrap();
        let locked = fixture.join("locked.bin");
        std::fs::write(&locked, b"preserved").unwrap();
        let mut failed_app = app.clone();
        failed_app.install_location = fixture.to_string_lossy().into_owned();
        let mut failed_items = items.clone();
        failed_items[0].path = locked.to_string_lossy().into_owned();
        let outcome = delete_planned_items_with_io(
            &failed_items,
            &mut |_| None,
            &mut |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "fixture denied",
                ))
            },
            &mut |_| false,
        );
        assert_eq!(outcome.failed, 1);
        assert_eq!(outcome.details[0].path, failed_items[0].path);
        assert_eq!(outcome.details[0].status, "failed");
        assert!(outcome.details[0].message.contains("fixture denied"));
        assert_eq!(std::fs::read(&locked).unwrap(), b"preserved");
        crate::fsutil::remove_tree_no_reparse(&fixture).unwrap();
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

    /// The gate decision is taken once and shared by the backup and delete
    /// phases. The delete phase used to decide again, after up to 300s of vendor
    /// uninstaller and with mutable global state on both sides, so an item
    /// refused at backup time could be allowed at delete time — deleted with no
    /// backup. An item the gate refuses must therefore never be deleted, and must
    /// still be reported as skipped with the gate's reason.
    #[test]
    fn one_gate_decision_is_shared_by_backup_and_delete() {
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
        // One item the gate refuses outright (a library root is a red line) and
        // one it allows.
        let refused = CleanupItem {
            path: r"C:\Users\a\Documents".into(),
            kind: ItemKind::Dir,
            score: 90,
            confidence: crate::scanner::Confidence::Confirmed,
            risk: crate::scanner::RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            service_bound: false,
            user_data: true,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        };
        let root = crate::test_support::TempDir::new("plan");
        let leftover = root.join("Leftover");
        std::fs::create_dir_all(&leftover).unwrap();
        std::fs::write(leftover.join("keep.bin"), b"payload").unwrap();
        let allowed = CleanupItem {
            path: leftover.to_string_lossy().into_owned(),
            kind: ItemKind::Dir,
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
        };
        let mut app_with_loc = app.clone();
        app_with_loc.install_location = root.to_string_lossy().into_owned();

        let ignore = crate::ignore::IgnoreList::default();
        let source = crate::policy::CleanupSource::Uninstall;
        let plan = plan_cleanup_items(
            &app_with_loc,
            &[refused.clone(), allowed.clone()],
            source,
            &ignore,
        );
        assert_eq!(
            plan.allowed.len(),
            1,
            "only the allowed item may be deleted"
        );
        assert_eq!(plan.allowed[0].path, allowed.path);
        assert_eq!(plan.refused.len(), 1);
        assert_eq!(plan.refused[0].path, refused.path);
        assert_eq!(plan.refused[0].status, "skipped");
        assert_eq!(plan.refused[0].message, "user_data red line");

        let report = crate::executor::run_full_cleanup(
            &app_with_loc,
            &[refused, allowed],
            &crate::executor::FullCleanupOptions {
                dry_run: false,
                skip_official_uninstall: true,
                backup_enabled: false,
                restore_point: false,
                cleanup_source: None,
            },
        );
        assert_eq!(report.deleted, 1, "{:?}", report.item_details);
        assert_eq!(report.failed, 0);
        assert_eq!(report.skipped, 1, "the refused item is still reported");
        assert!(!leftover.exists(), "the allowed item is deleted");
        assert!(report
            .item_details
            .iter()
            .any(|d| { d.status == "skipped" && d.message == "user_data red line" }));
    }

    /// F-3 end to end: an uninstall string that exists but cannot be parsed
    /// must abort the whole run — deleting leftovers of an app that is still
    /// registered would leave it un-uninstallable from Programs & Features.
    #[test]
    fn unusable_uninstall_command_aborts_before_any_delete() {
        let root = crate::test_support::TempDir::new("had_cmd");
        let dir = root.join("App");
        std::fs::create_dir_all(&dir).unwrap();
        let kept = dir.join("keep.bin");
        std::fs::write(&kept, b"payload").unwrap();
        // A leading quote with no closing one is real vendor dirty data and
        // makes build_uninstall_command return None.
        let app = crate::apps::InstalledApp {
            name: "DemoApp".into(),
            publisher: "Vendor".into(),
            version: "1.0".into(),
            install_location: dir.to_string_lossy().into_owned(),
            uninstall_string: r#""C:\Program Files\App\unins.exe"#.into(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        // Trust the command pair so the run reaches the parse branch; the
        // trust set is process-global, so restore whatever was there.
        let _trust = crate::apps::trust_test_support::guard();
        crate::apps::remember_uninstall_commands(std::slice::from_ref(&app));
        let item = CleanupItem {
            path: kept.to_string_lossy().into_owned(),
            kind: ItemKind::File,
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
        };
        let report = run_full_cleanup(
            &app,
            &[item],
            &FullCleanupOptions {
                dry_run: false,
                skip_official_uninstall: false,
                backup_enabled: false,
                restore_point: false,
                cleanup_source: None,
            },
        );
        assert!(report.aborted, "{:?}", report.item_details);
        assert_eq!(report.deleted, 0);
        assert!(
            report
                .uninstall_message
                .contains("unusable uninstall command"),
            "the real cause must be reported, got {:?}",
            report.uninstall_message
        );
        assert!(kept.exists(), "nothing may be deleted on an aborted run");
    }

    /// F-18: backup enabled but nothing to back up must not create (and seal)
    /// an empty session that the Safety Vault then offers for restore.
    #[test]
    fn empty_plan_skips_backup_session_creation() {
        let _guard = crate::backup::lock_backup_env();
        let root = crate::test_support::TempDir::new("no_empty_session");
        let _env = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", root.join("Backup"));
        let app = crate::apps::InstalledApp {
            name: "DemoApp".into(),
            publisher: "Vendor".into(),
            version: "1.0".into(),
            install_location: r"C:\Program Files\DemoApp".into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        // A valid selection whose every row the gate refuses: the plan has
        // nothing to back up, but the run is not the empty-selection early exit.
        let fixture = crate::test_support::TempDir::new("no_empty_session_item");
        let kept = fixture.join("keep.bin");
        std::fs::write(&kept, b"x").unwrap();
        let item = CleanupItem {
            path: kept.to_string_lossy().into_owned(),
            kind: ItemKind::File,
            score: 90,
            confidence: crate::scanner::Confidence::Confirmed,
            risk: crate::scanner::RiskLevel::Low,
            reason: "user data".into(),
            evidence: vec![],
            shared: false,
            service_bound: false,
            user_data: true,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        };
        let report = run_full_cleanup(
            &app,
            &[item],
            &FullCleanupOptions {
                dry_run: false,
                skip_official_uninstall: true,
                backup_enabled: true,
                restore_point: false,
                cleanup_source: None,
            },
        );
        assert!(!report.aborted, "{:?}", report.errors);
        assert_eq!(report.backup_dir, "");
        assert!(
            crate::restore::list_sessions().unwrap().is_empty(),
            "a run with nothing allowed must not leave a session on disk"
        );
    }

    /// A registry key the vendor uninstaller already removed is recorded as
    /// `skipped`, exactly like the File/Dir "path missing" rule and the dry-run
    /// path — a row that is already gone is not a failure the user must act on.
    #[test]
    fn missing_registry_entry_is_skipped_not_failed() {
        // A GUID-shaped Uninstall key: the association proof the gate requires
        // for a registry item, so the test exercises the delete outcome and not
        // the association gate. HKCU, so the fixture needs no elevation.
        let uninstall_key = r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\12345678-1234-1234-1234-123456789012";
        let app = crate::apps::InstalledApp {
            name: "DemoApp".into(),
            publisher: "Vendor".into(),
            version: "1.0".into(),
            install_location: r"C:\Program Files\DemoApp".into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKCU".into(),
            registry_key: uninstall_key.into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let items = vec![CleanupItem {
            path: uninstall_key.into(),
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
        // The uninstall key itself exists but the GUID subkey does not, which is
        // the "the uninstaller already took it" shape.
        let outcome = delete_planned_items_with_native(&items, &mut |_| None);
        assert_eq!(
            (outcome.deleted, outcome.failed, outcome.skipped),
            (0, 0, 1)
        );
        assert_eq!(outcome.details[0].kind, "registry");
        assert_eq!(outcome.details[0].status, "skipped");
        assert!(
            outcome.details[0].message.contains("missing"),
            "{}",
            outcome.details[0].message
        );
        assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);

        // A missing VALUE under a key that does not exist is the same family.
        let value_items = vec![CleanupItem {
            path: r"HKCU\SOFTWARE\RemovaNoSuchVendor|NoSuchValue".into(),
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
        let value_outcome = delete_planned_items_with_native(&value_items, &mut |_| None);
        assert_eq!(
            (
                value_outcome.deleted,
                value_outcome.failed,
                value_outcome.skipped
            ),
            (0, 0, 1)
        );
        assert_eq!(value_outcome.details[0].status, "skipped");

        // The full run reports the same thing end to end.
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
        assert_eq!(report.failed, 0, "{:?}", report.errors);
        assert_eq!(report.skipped, 1, "{:?}", report.item_details);
        assert!(report
            .item_details
            .iter()
            .any(|d| d.status == "skipped" && d.message.contains("missing")));
    }
}
