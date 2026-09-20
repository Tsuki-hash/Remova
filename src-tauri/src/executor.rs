//! Uninstall command parsing and dry-run cleanup planning (Phase 2).

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

/// Process-wide lock so batch + manual cleanup cannot race PATH/backup (S-08).
static CLEANUP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

/// Legacy dry-run without app context — still shares policy safety/user_data gates (A-N3).
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCleanupOptions {
    pub dry_run: bool,
    pub skip_official_uninstall: bool,
    pub backup_enabled: bool,
    /// Create a Windows restore point (tests set false to avoid real side effects).
    #[serde(default = "default_true")]
    pub restore_point: bool,
    /// uninstall | orphan | monitor | copilot — default uninstall (S-R4-07).
    #[serde(default)]
    pub cleanup_source: Option<String>,
}

pub fn cleanup_source_from_opts(opts: &FullCleanupOptions) -> crate::policy::CleanupSource {
    match opts.cleanup_source.as_deref() {
        Some("orphan") => crate::policy::CleanupSource::Orphan,
        Some("monitor") => crate::policy::CleanupSource::Monitor,
        Some("copilot") => crate::policy::CleanupSource::Copilot,
        _ => crate::policy::CleanupSource::Uninstall,
    }
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
    /// Reboot-delayed deletes (not counted in `deleted`) — S-R4-13.
    #[serde(default)]
    pub delayed: u32,
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
    // Backup session is optional (product opt-in). Restore point stays decoupled (S-R4-05).
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
            // A-N2 / S-N1: only backup items that pass the cleanup gate.
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
            uninstall_message: format!("backup session failed: {e}"),
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

/// True when cleanup was launched from the orphan leftovers page (no real InstalledApp).
pub fn is_orphan_flow(app: &crate::apps::InstalledApp) -> bool {
    app.source.eq_ignore_ascii_case("orphan")
        || (app.registry_key.trim().is_empty()
            && app.install_location.trim().is_empty()
            && app.uninstall_string.trim().is_empty()
            && app.quiet_uninstall_string.trim().is_empty())
}

/// Generic English tokens that create AR-10 false positives on short path segments.
const AR10_NAME_STOPWORDS: &[&str] = &[
    "app", "tool", "free", "pro", "data", "user", "file", "setup", "client", "server", "service",
    "manager", "helper", "plugin", "update", "code", "edit",
];

fn ar10_name_slug_ok(slug: &str) -> bool {
    let s = slug.trim().to_lowercase();
    s.len() >= 5 && !AR10_NAME_STOPWORDS.contains(&s.as_str())
}

fn ar10_in_install_root(low: &str) -> bool {
    low.contains("\\appdata\\")
        || low.contains("\\programdata\\")
        || low.contains("\\program files")
        || low.contains("\\program files (x86)")
}

/// First path segment under a Common Files root (vendor folder name).
pub fn common_files_vendor_segment(path: &str) -> Option<String> {
    let p = path.replace('/', "\\").to_lowercase();
    let idx = p.find(r"\common files\")?;
    let rest = &p[idx + r"\common files\".len()..];
    let seg = rest.split('\\').next().unwrap_or("").trim();
    if seg.is_empty() || seg == "." || seg == ".." {
        return None;
    }
    Some(seg.to_string())
}

/// S-7R1: CF vendor association — vendor **directory segment** equals install prefix
/// under Common Files, or equals a strong name/publisher slug (segment equality).
pub fn cf_vendor_associated(app: &crate::apps::InstalledApp, path: &str) -> bool {
    if is_orphan_flow(app) {
        return false;
    }
    let Some(vendor_seg) = common_files_vendor_segment(path) else {
        return false;
    };
    let low = path.replace('/', "\\").to_lowercase();
    if low.split('\\').any(|s| s == ".." || s == ".") {
        return false;
    }
    let install = app
        .install_location
        .trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    if !install.is_empty() {
        if low == install || low.starts_with(&format!(r"{install}\")) {
            return true;
        }
        if let Some(inst_vendor) = common_files_vendor_segment(&install) {
            if inst_vendor == vendor_seg {
                return true;
            }
        }
    }
    let slugs = crate::scanner::slugify(&app.name);
    if slugs
        .iter()
        .any(|s| ar10_name_slug_ok(s) && s.to_lowercase() == vendor_seg)
    {
        return true;
    }
    crate::scanner::slugify(&app.publisher).iter().any(|s| {
        let sl = s.to_lowercase();
        sl.len() >= 6 && !AR10_NAME_STOPWORDS.contains(&sl.as_str()) && sl == vendor_seg
    })
}

/// Full scheduled-task name from a TaskCache\Tree registry path (S-R4-11).
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

/// True when a PATH leftover segment is present in User/Machine PATH (same source as scrub).
fn path_entry_in_system_path(entry: &str) -> bool {
    crate::regops::scrub_path_entry_ok(entry)
}

fn guid_in_text(s: &str) -> Option<String> {
    let low = s.to_lowercase();
    let start = low.find('{')?;
    let end = low[start..].find('}')? + start;
    let g = &s[start..=end];
    if g.len() >= 38 {
        Some(g.to_lowercase())
    } else {
        None
    }
}

/// Light association for Registry / PATH leftovers when an installed app is known (S-R4-03).
/// S-3: never trust client `reason` — path / registry / publisher signals only.
fn non_fs_associated_with_app(app: &crate::apps::InstalledApp, item: &CleanupItem) -> bool {
    if is_orphan_flow(app) {
        return true;
    }
    let low = item.path.replace('/', "\\").to_lowercase();
    if low.split('\\').any(|seg| seg == ".." || seg == ".") {
        return false;
    }
    let install = app
        .install_location
        .trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    if !install.is_empty() && low.contains(&install) {
        return true;
    }
    if let Some(guid) = guid_in_text(&app.registry_key) {
        if low.contains(&guid) {
            return true;
        }
    }
    let pub_low = app.publisher.trim().to_lowercase();
    if pub_low.len() >= 4 && low.contains(&pub_low) {
        return true;
    }
    let slugs = crate::scanner::slugify(&app.name);
    slugs
        .iter()
        .any(|s| ar10_name_slug_ok(s) && low.contains(&s.to_lowercase()))
}

/// Medium association gate (AR-10): leftovers must look related to the app.
/// Orphan/monitor sources skip association at the policy layer.
/// R2-11: keep fail-closed; tighten short/generic slug false positives.
pub fn path_associated_with_app(app: &crate::apps::InstalledApp, item: &CleanupItem) -> bool {
    if item.path.trim().is_empty() {
        return false;
    }
    // S-4: traversal segments never associate.
    if item
        .path
        .replace('/', "\\")
        .split('\\')
        .any(|seg| seg == ".." || seg == ".")
    {
        return false;
    }
    match item.kind {
        ItemKind::Registry | ItemKind::Path => non_fs_associated_with_app(app, item),
        ItemKind::File | ItemKind::Dir => {
            let path = item.path.replace('/', "\\");
            let low = path.to_lowercase();
            if is_orphan_flow(app) {
                // S-01: orphan leftovers have no product install_location; allow any FS path
                // that passes the shared safety gate (still user-confirmed in UI).
                return crate::safety::is_safe_fs(std::path::Path::new(&item.path));
            }
            let install = app
                .install_location
                .trim()
                .replace('/', "\\")
                .trim_end_matches('\\')
                .to_lowercase();
            let install_empty = install.is_empty();
            if !install_empty {
                let inst = install.as_str();
                if low == inst || low.starts_with(&format!("{inst}\\")) {
                    return true;
                }
            }
            let slugs = crate::scanner::slugify(&app.name);
            let name_hit = slugs
                .iter()
                .any(|s| ar10_name_slug_ok(s) && low.contains(&s.to_lowercase()));
            if name_hit {
                // With a known install location a name hit is a useful secondary signal.
                // Without one, only trust name hits under common install roots (fail-closed).
                if !install_empty || ar10_in_install_root(&low) {
                    return true;
                }
            }
            let pub_slugs = crate::scanner::slugify(&app.publisher);
            if !pub_slugs.is_empty()
                && pub_slugs
                    .iter()
                    .filter(|s| {
                        s.len() >= 6 && !AR10_NAME_STOPWORDS.contains(&s.to_lowercase().as_str())
                    })
                    .any(|s| low.contains(&s.to_lowercase()))
                && ar10_in_install_root(&low)
            {
                return true;
            }
            false
        }
    }
}

struct DeleteOutcome {
    deleted: u32,
    failed: u32,
    skipped: u32,
    delayed: u32,
    errors: Vec<String>,
    details: Vec<ItemDetail>,
}

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
                    // S-R4-11: use full task path from TaskCache tree when possible.
                    let tn = task_full_name_from_reg_path(&it.path);
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
                // S-N6: a path that is already gone is not a successful delete.
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
                let res = if p.is_dir() {
                    std::fs::remove_dir_all(p)
                } else {
                    std::fs::remove_file(p)
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

/// Full cleanup: optional backup → official uninstall → residual delete.
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
            delayed: 0,
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

    let del = delete_cleanup_items_source(app, items, cleanup_source_from_opts(opts));

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
    use crate::scanner::{Confidence, RiskLevel};

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

    #[test]
    fn cf_vendor_segment_and_association() {
        assert_eq!(
            common_files_vendor_segment(r"C:\Program Files\Common Files\Acme\lib.dll").as_deref(),
            Some("acme")
        );
        assert_eq!(
            common_files_vendor_segment(r"C:\Program Files\DemoApp"),
            None
        );
        let app = crate::apps::InstalledApp {
            name: "DemoApp".into(),
            version: "1".into(),
            publisher: "Acme Corp".into(),
            install_location: r"C:\Program Files\DemoApp".into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        // vendor_seg == app name slug
        assert!(cf_vendor_associated(
            &app,
            r"C:\Program Files\Common Files\DemoApp\plugins"
        ));
        // substring hit in a *deeper* segment must not associate (S7-R1)
        assert!(!cf_vendor_associated(
            &app,
            r"C:\Program Files\Common Files\Acme\demo_backup"
        ));
        // install under same CF vendor folder
        let cf_app = crate::apps::InstalledApp {
            install_location: r"C:\Program Files\Common Files\Acme\Libs".into(),
            ..app.clone()
        };
        assert!(cf_vendor_associated(
            &cf_app,
            r"C:\Program Files\Common Files\Acme\Extra"
        ));
        assert!(!cf_vendor_associated(
            &app,
            r"C:\Program Files\Common Files\Acme\x"
        ));
    }

    #[test]
    fn path_association_medium_gate() {
        let app = crate::apps::InstalledApp {
            name: "DemoApp".into(),
            version: "1".into(),
            publisher: "Acme Corp".into(),
            install_location: r"C:\Program Files\DemoApp".into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let mut it = CleanupItem {
            path: r"C:\Program Files\DemoApp\bin\x.exe".into(),
            kind: ItemKind::File,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            size_kb: None,
            bucket: None,
        };
        assert!(path_associated_with_app(&app, &it));
        it.path = r"C:\Users\a\AppData\Local\demoapp\cache".into();
        assert!(path_associated_with_app(&app, &it));
        it.path = r"C:\Program Files\UnrelatedVendor\Tool\bin.exe".into();
        assert!(!path_associated_with_app(&app, &it));
        it.kind = ItemKind::Registry;
        it.path = r"HKCU\Software\DemoApp\Config".into();
        assert!(path_associated_with_app(&app, &it));
        it.path = r"HKCU\Software\UnrelatedVendor\Thing".into();
        assert!(!path_associated_with_app(&app, &it));
        it.kind = ItemKind::Path;
        it.path = r"C:\Program Files\DemoApp\bin".into();
        assert!(path_associated_with_app(&app, &it));
        it.path = r"D:\OtherApp\bin".into();
        assert!(!path_associated_with_app(&app, &it));
    }

    #[test]
    fn ar10_rejects_generic_short_slug_false_positives() {
        let app = crate::apps::InstalledApp {
            name: "Code".into(),
            version: "1".into(),
            publisher: "Microsoft".into(),
            install_location: r"C:\Program Files\Code".into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let it = CleanupItem {
            path: r"C:\Users\a\AppData\Local\Temp\code-cache".into(),
            kind: ItemKind::Dir,
            score: 40,
            confidence: Confidence::Suspected,
            risk: RiskLevel::Medium,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            size_kb: None,
            bucket: None,
        };
        // "code" is a stopword / too short — not enough by itself outside install root match.
        assert!(!path_associated_with_app(&app, &it));
        // Install-location prefix still passes.
        let ok = CleanupItem {
            path: r"C:\Program Files\Code\bin\code.exe".into(),
            ..it.clone()
        };
        assert!(path_associated_with_app(&app, &ok));
    }

    #[test]
    fn orphan_flow_uses_safety_not_slug() {
        let app = crate::apps::InstalledApp {
            name: "孤儿扫描".into(),
            version: String::new(),
            publisher: String::new(),
            install_location: String::new(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "Orphan".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        assert!(is_orphan_flow(&app));
        let base = CleanupItem {
            path: r"C:\Program Files\SomeVendor\Tool".into(),
            kind: ItemKind::Dir,
            score: 40,
            confidence: Confidence::Suspected,
            risk: RiskLevel::Medium,
            reason: "orphan".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            size_kb: None,
            bucket: None,
        };
        assert!(path_associated_with_app(&app, &base));
        let win = CleanupItem {
            path: r"C:\Windows".into(),
            ..base.clone()
        };
        assert!(!path_associated_with_app(&app, &win));
    }

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

    #[test]
    fn missing_path_is_skipped_not_deleted() {
        // S-N6: full delete must not count non-existent File/Dir as deleted.
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
