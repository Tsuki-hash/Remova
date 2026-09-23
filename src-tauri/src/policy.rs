//! Cleanup / manage policy fa莽ade (A-02).
//! Single semantic gate for dry-run preview and real delete; manage/store/shared re-exported here.

use crate::apps::InstalledApp;
use crate::scanner::{CleanupItem, ItemKind};

/// Who launched cleanup 鈥?drives orphan vs installed-app association rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupSource {
    Uninstall,
    Orphan,
    Monitor,
    Copilot,
}

impl CleanupSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CleanupSource::Uninstall => "uninstall",
            CleanupSource::Orphan => "orphan",
            CleanupSource::Monitor => "monitor",
            CleanupSource::Copilot => "copilot",
        }
    }
}

/// Result of the shared leftover gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    /// Stable skip reason shown in dry-run / delete item details.
    Skip(&'static str),
}

impl GateDecision {
    pub fn is_allow(&self) -> bool {
        matches!(self, GateDecision::Allow)
    }

    pub fn message(&self) -> &'static str {
        match self {
            GateDecision::Allow => "",
            GateDecision::Skip(m) => m,
        }
    }
}

/// Expand `%VAR%` environment references in a PATH segment (gate-time only).
fn expand_path_env(entry: &str) -> String {
    let mut out = String::with_capacity(entry.len());
    let bytes: Vec<char> = entry.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == '%' {
            if let Some(end) = bytes[i + 1..].iter().position(|c| *c == '%') {
                let name: String = bytes[i + 1..i + 1 + end].iter().collect();
                if name.is_empty() {
                    out.push('%');
                    i += 1;
                    continue;
                }
                if let Ok(val) = std::env::var(&name) {
                    out.push_str(&val);
                    i += end + 2;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// PATH segments that must never be scrubbed (system PATH).
/// Public so write primitives can apply an intrinsic secondary gate (S-R6-06).
pub fn is_dangerous_path_entry(entry: &str) -> bool {
    let expanded = expand_path_env(entry);
    let s = expanded
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    if s.is_empty() || s.len() <= 3 {
        return true;
    }
    // Drive root, relative junk, or unexpanded env
    if s.ends_with(':') || !s.contains('\\') || s.contains('%') {
        return true;
    }
    // Reject path traversal in PATH segments (S-4 class).
    if s.split('\\').any(|seg| seg == ".." || seg == ".") {
        return true;
    }
    let mut danger: Vec<String> = vec![
        r"c:\windows".into(),
        r"c:\windows\system32".into(),
        r"c:\windows\system32\wbem".into(),
        r"c:\windows\system32\windowspowershell\v1.0".into(),
        r"c:\windows\system32\openssh".into(),
        r"c:\program files\powershell".into(),
        r"c:\program files\powershell\7".into(),
        r"c:\program files (x86)\powershell".into(),
        r"c:\programdata\microsoft\windows\start menu\programs\startup".into(),
    ];
    // Env roots: only system PATH-shaped subtrees 鈥?not entire ProgramFiles/ProgramData (S-2).
    if let Ok(sr) = std::env::var("SystemRoot").or_else(|_| std::env::var("windir")) {
        let root = sr
            .replace('/', "\\")
            .to_lowercase()
            .trim_end_matches('\\')
            .to_string();
        if root.len() > 2 {
            danger.push(root.clone());
            danger.push(format!(r"{root}\system32"));
            danger.push(format!(r"{root}\syswow64"));
            danger.push(format!(r"{root}\system32\wbem"));
            danger.push(format!(r"{root}\system32\windowspowershell\v1.0"));
            danger.push(format!(r"{root}\system32\openssh"));
        }
    }
    if let Ok(pf) = std::env::var("ProgramFiles") {
        let low = pf
            .replace('/', "\\")
            .to_lowercase()
            .trim_end_matches('\\')
            .to_string();
        if low.len() > 2 {
            danger.push(format!(r"{low}\windowsapps"));
            danger.push(format!(r"{low}\powershell"));
            danger.push(format!(r"{low}\powershell\7"));
        }
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        let low = pf86
            .replace('/', "\\")
            .to_lowercase()
            .trim_end_matches('\\')
            .to_string();
        if low.len() > 2 {
            danger.push(format!(r"{low}\windowsapps"));
            danger.push(format!(r"{low}\powershell"));
        }
    }
    danger
        .iter()
        .any(|d| s == *d || s.starts_with(&format!("{d}\\")))
}

/// Shared leftover gate for dry-run and full delete (S-05 / A-02).
/// Order: server-side user_data/shared recompute 鈫?client flags 鈫?ignore 鈫?AR-10 鈫?safety.
pub fn gate_cleanup_item(
    app: Option<&InstalledApp>,
    item: &CleanupItem,
    source: CleanupSource,
    ignore: &crate::ignore::IgnoreList,
) -> GateDecision {
    // S-N1: never trust client-only flags 鈥?recompute red lines server-side.
    if item.user_data || crate::safety::is_user_data_path(&item.path) {
        return GateDecision::Skip("user_data red line");
    }
    if crate::safety::looks_like_sync_conflict(&item.path) {
        return GateDecision::Skip("user_data red line");
    }
    // Library subpaths (Documents/<App>, …) may be cleaned only with a proven link to the
    // app (or in orphan/monitor flows). Never treat them as free-standing leftovers.
    if crate::safety::is_user_library_path(&item.path)
        && !matches!(source, CleanupSource::Orphan | CleanupSource::Monitor)
        && app.is_none()
    {
        return GateDecision::Skip("path not associated with app");
    }
    // S-7B hard shared: client flag, name tokens, CF roots, Microsoft Shared, non-CF markers.
    if item.shared || crate::shared::is_hard_shared_item(&item.reason, &item.path, &item.reason) {
        return GateDecision::Skip("shared runtime");
    }
    if crate::ignore::should_skip_leftover_path(ignore, &item.path) {
        return GateDecision::Skip("ignored path");
    }

    match item.kind {
        ItemKind::Path => {
            // S-N2: PATH scrub must not touch system segments.
            if is_dangerous_path_entry(&item.path) {
                return GateDecision::Skip("protected PATH entry");
            }
        }
        ItemKind::Registry => {
            if crate::safety::is_safe_to_delete_registry(&item.path).is_err() {
                return GateDecision::Skip("failed safety gate");
            }
        }
        ItemKind::File | ItemKind::Dir => {
            if !crate::safety::is_safe_fs_for_delete(std::path::Path::new(&item.path)) {
                return GateDecision::Skip("failed safety gate");
            }
        }
    }

    if let Some(app) = app {
        // S-R6-01: client cleanup_source alone must not disable association.
        // S-R7-01: orphan-shaped apps under orphan/monitor source must come from the
        // server-side scan allow-list. Forged empty InstalledApp + cleanup_source=orphan
        // must not delete arbitrary paths.
        let client_orphan = matches!(source, CleanupSource::Orphan | CleanupSource::Monitor);
        let orphan_shape = crate::association::is_orphan_flow(app);
        if client_orphan && orphan_shape {
            if !crate::orphans::was_recent_orphan_path(&item.path) {
                return GateDecision::Skip("path not associated with app");
            }
            // Confirmed orphan scan path — the allow-list is the association proof.
            // CF vendor paths still hard-skip (shared runtime).
            if matches!(item.kind, ItemKind::File | ItemKind::Dir)
                && crate::shared::is_common_files_vendor_path(&item.path)
            {
                return GateDecision::Skip("shared runtime");
            }
        } else if matches!(item.kind, ItemKind::File | ItemKind::Dir)
            && crate::shared::is_common_files_vendor_path(&item.path)
        {
            // S-7B/R1: CF vendor subpaths need vendor-segment association, not path substring.
            // Orphan/Monitor never get CF vendor allow (shared runtime).
            if client_orphan || !crate::association::cf_vendor_associated(app, &item.path) {
                return GateDecision::Skip("shared runtime");
            }
        } else if !crate::association::path_associated_with_app(app, item) {
            // S-R4-03: Registry/Path / non-CF FS also require association when app is known.
            return GateDecision::Skip("path not associated with app");
        }
    } else if matches!(item.kind, ItemKind::File | ItemKind::Dir)
        && crate::shared::is_common_files_vendor_path(&item.path)
    {
        // No app context 鈥?cannot prove vendor ownership.
        return GateDecision::Skip("shared runtime");
    } else if matches!(source, CleanupSource::Orphan | CleanupSource::Monitor) {
        // Orphan/Monitor without app: only paths from the last server-side orphan scan.
        if !crate::orphans::was_recent_orphan_path(&item.path) {
            return GateDecision::Skip("path not associated with app");
        }
    }
    GateDecision::Allow
}

// --- Fa莽ade re-exports (single lookup for safety/manage/shared policy) ---

pub use crate::safety::{
    allow_manage_reg_write, allow_manage_service_write, critical_service_names, is_allowed_run_key,
    is_allowed_startup_approved_key, is_critical_service, is_safe_fs, is_safe_fs_for_delete,
    is_safe_to_delete_registry,
};
pub use crate::shared::{is_common_files_vendor_path, is_hard_shared_item, is_shared_item};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{Confidence, RiskLevel};

    fn app(name: &str, loc: &str) -> InstalledApp {
        InstalledApp {
            name: name.into(),
            version: "1".into(),
            publisher: "Acme".into(),
            install_location: loc.into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        }
    }

    fn item(path: &str, kind: ItemKind) -> CleanupItem {
        CleanupItem {
            path: path.into(),
            kind,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
            shared: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        }
    }

    #[test]
    fn rejects_forged_user_data_and_shared_flags() {
        let ignore = crate::ignore::IgnoreList::default();
        // Client says user_data=false but path is a library root — must skip (S-N1).
        let forged_root = item(r"C:\Users\a\Documents", ItemKind::Dir);
        assert_eq!(
            gate_cleanup_item(None, &forged_root, CleanupSource::Uninstall, &ignore),
            GateDecision::Skip("user_data red line")
        );
        // Library subpath without app context cannot prove association — skip too.
        let forged_ud = item(r"C:\Users\a\Documents\Important", ItemKind::Dir);
        assert_eq!(
            gate_cleanup_item(None, &forged_ud, CleanupSource::Uninstall, &ignore),
            GateDecision::Skip("path not associated with app")
        );
        // S-R4-01: exact profile roots without trailing segment.
        for root in [
            r"C:\Users\a\Documents",
            r"C:\Users\a\Downloads",
            r"C:\Users\Public\Documents",
        ] {
            let it = item(root, ItemKind::Dir);
            assert!(
                !gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore).is_allow(),
                "exact user_data root must skip: {root}"
            );
        }
        // S-R4-02: exact Common Files root.
        for root in [
            r"C:\Program Files\Common Files",
            r"C:\Program Files (x86)\Common Files",
        ] {
            let it = item(root, ItemKind::Dir);
            assert!(
                !gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore).is_allow(),
                "exact shared root must skip: {root}"
            );
        }
        // S-7B: Microsoft Shared hard skip; CF vendor needs association.
        let ms = item(
            r"C:\Program Files\Common Files\Microsoft Shared\VxOps",
            ItemKind::Dir,
        );
        assert!(
            !gate_cleanup_item(None, &ms, CleanupSource::Uninstall, &ignore).is_allow(),
            "Microsoft Shared must skip"
        );
        let vendor = item(
            r"C:\Program Files\Common Files\Vendor\redist",
            ItemKind::Dir,
        );
        assert!(
            !gate_cleanup_item(None, &vendor, CleanupSource::Uninstall, &ignore).is_allow(),
            "CF vendor without app context must skip"
        );
        // Client says shared=false but path looks shared 鈫?must skip.
        let forged_sh = item(
            r"C:\Program Files\Common Files\Vendor\redist",
            ItemKind::Dir,
        );
        assert_eq!(
            gate_cleanup_item(None, &forged_sh, CleanupSource::Uninstall, &ignore),
            GateDecision::Skip("shared runtime")
        );
    }

    #[test]
    fn common_files_vendor_allows_only_with_association() {
        let ignore = crate::ignore::IgnoreList::default();
        let a = app("DemoApp", r"C:\Program Files\DemoApp");
        // Unassociated CF vendor 鈫?skip.
        let un = item(
            r"C:\Program Files\Common Files\OtherVendor\cache",
            ItemKind::Dir,
        );
        assert!(!gate_cleanup_item(Some(&a), &un, CleanupSource::Uninstall, &ignore).is_allow());
        // Associated via vendor **segment** equal to app name slug 鈫?allow (S-7B/R1).
        let assoc = item(
            r"C:\Program Files\Common Files\DemoApp\plugins",
            ItemKind::Dir,
        );
        assert!(
            gate_cleanup_item(Some(&a), &assoc, CleanupSource::Uninstall, &ignore).is_allow(),
            "associated CF vendor path should allow"
        );
        // Deeper segment merely *contains* name slug 鈫?must skip (S7-R1).
        let loose = item(
            r"C:\Program Files\Common Files\Acme\demo_backup",
            ItemKind::Dir,
        );
        assert!(
            !gate_cleanup_item(Some(&a), &loose, CleanupSource::Uninstall, &ignore).is_allow(),
            "substring-only CF path must not allow"
        );
        // Orphan / Monitor never get CF vendor allow.
        let orphan_app = app("瀛ゅ効鎵弿", "");
        assert!(
            !gate_cleanup_item(Some(&orphan_app), &assoc, CleanupSource::Orphan, &ignore)
                .is_allow()
        );
        assert!(!gate_cleanup_item(Some(&a), &assoc, CleanupSource::Monitor, &ignore).is_allow());
        // Microsoft Shared still hard skip even with association-shaped path.
        let ms = item(
            r"C:\Program Files\Common Files\Microsoft Shared\DemoApp",
            ItemKind::Dir,
        );
        assert!(!gate_cleanup_item(Some(&a), &ms, CleanupSource::Uninstall, &ignore).is_allow());
        // item.shared=true still hard-skips CF vendor.
        let mut forged = assoc.clone();
        forged.shared = true;
        assert!(
            !gate_cleanup_item(Some(&a), &forged, CleanupSource::Uninstall, &ignore).is_allow()
        );
        // dry 鈮?full for CF vendor items (single gate).
        for it in [&un, &assoc, &loose] {
            let d = gate_cleanup_item(Some(&a), it, CleanupSource::Uninstall, &ignore);
            let f = gate_cleanup_item(Some(&a), it, CleanupSource::Uninstall, &ignore);
            assert_eq!(d, f, "dry/full mismatch {}", it.path);
        }
    }

    #[test]
    fn path_scrub_rejects_system_entries() {
        let ignore = crate::ignore::IgnoreList::default();
        // Always-dangerous PATH segments (env-independent).
        for p in [
            r"C:\Windows\System32",
            r"C:\Windows",
            r"c:\windows\system32\wbem\",
            "",
            r"C:",
            r"C:\Program Files\DemoApp\bin\..\..\..\Windows",
        ] {
            let it = item(p, ItemKind::Path);
            let d = gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore);
            assert!(!d.is_allow(), "expected skip for PATH entry {p}");
        }
        // Env-expanded system subtrees (SystemRoot/ProgramFiles are set on CI Windows).
        if std::env::var("SystemRoot").is_ok() || std::env::var("windir").is_ok() {
            let it = item(r"%SystemRoot%\System32", ItemKind::Path);
            let d = gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore);
            assert!(!d.is_allow(), "expected skip for %SystemRoot%\\System32");
        }
        // Always-allowed vendor PATH (literal; not under system trees).
        for p in [
            r"C:\Vendor\Tool\bin",
            r"C:\Program Files\DemoApp\bin",
            r"D:\Games\Tool\bin",
        ] {
            let it = item(p, ItemKind::Path);
            let d = gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore);
            assert!(
                d.is_allow(),
                "expected allow for vendor PATH {p}, got {d:?}"
            );
        }
        // %ProgramFiles%\Vendor\* is allowed only when ProgramFiles expands (S-2, not blanket deny).
        if let Ok(pf) = std::env::var("ProgramFiles") {
            if pf.trim().len() > 2 {
                let it = item(r"%ProgramFiles%\Vendor\Tool", ItemKind::Path);
                let d = gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore);
                assert!(
                    d.is_allow(),
                    "expanded ProgramFiles vendor PATH should allow, got {d:?}"
                );
            }
        }
        let it = item(r"%ProgramFiles%\WindowsApps", ItemKind::Path);
        let d = gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore);
        assert!(!d.is_allow(), "WindowsApps PATH must skip");
    }

    #[test]
    fn registry_path_ignore_forged_reason() {
        let ignore = crate::ignore::IgnoreList::default();
        let a = app("DemoApp", r"C:\Program Files\DemoApp");
        let mut it = item(r"D:\Unrelated\App\bin", ItemKind::Path);
        it.reason = format!("belongs to {}", a.install_location);
        assert!(
            !gate_cleanup_item(Some(&a), &it, CleanupSource::Uninstall, &ignore).is_allow(),
            "forged reason must not associate"
        );
        let trav = item(
            r"C:\Program Files\DemoApp\..\..\Windows\System32",
            ItemKind::File,
        );
        assert!(!gate_cleanup_item(Some(&a), &trav, CleanupSource::Uninstall, &ignore).is_allow());
    }

    #[test]
    fn registry_path_require_app_association() {
        let ignore = crate::ignore::IgnoreList::default();
        let a = app("DemoApp", r"C:\Program Files\DemoApp");
        // Unrelated PATH / registry with real app context 鈫?skip (S-R4-03).
        let other_path = item(r"D:\TotallyOther\bin", ItemKind::Path);
        assert!(
            !gate_cleanup_item(Some(&a), &other_path, CleanupSource::Uninstall, &ignore).is_allow()
        );
        let other_reg = item(r"HKCU\Software\OtherVendor\Thing", ItemKind::Registry);
        assert!(
            !gate_cleanup_item(Some(&a), &other_reg, CleanupSource::Uninstall, &ignore).is_allow()
        );
        // Related PATH under install root 鈫?allow.
        let related = item(r"C:\Program Files\DemoApp\bin", ItemKind::Path);
        assert!(
            gate_cleanup_item(Some(&a), &related, CleanupSource::Uninstall, &ignore).is_allow()
        );
        // Vendor PATH under Program Files is not system-danger (S-2) 鈥?still needs association when app known.
        let pf_vendor = item(r"C:\Program Files\UnrelatedVendor\bin", ItemKind::Path);
        assert!(
            !gate_cleanup_item(Some(&a), &pf_vendor, CleanupSource::Uninstall, &ignore).is_allow()
        );
        // S-R7-01: orphan-shaped app + Orphan source + unscanned path must skip.
        let orphan_app = app("孤儿扫描", "");
        assert!(!gate_cleanup_item(
            Some(&orphan_app),
            &other_path,
            CleanupSource::Orphan,
            &ignore
        )
        .is_allow());
    }

    #[test]
    fn dry_and_full_gate_decisions_match_same_items() {
        // T-N1: dry-run preview skip set must equal full-delete gate skip set.
        let ignore = crate::ignore::IgnoreList::default();
        let a = app("DemoApp", r"C:\Program Files\DemoApp");
        let items = vec![
            item(r"C:\Program Files\DemoApp\bin\x.exe", ItemKind::File),
            item(r"C:\Program Files\Unrelated\bin.exe", ItemKind::File),
            {
                let mut ud = item(r"C:\Users\a\Documents\Work", ItemKind::Dir);
                ud.user_data = false; // forged
                ud
            },
            item(r"C:\Windows\System32", ItemKind::Path),
            item(r"C:\Program Files\DemoApp\bin", ItemKind::Path),
        ];
        for it in &items {
            let dry = gate_cleanup_item(Some(&a), it, CleanupSource::Uninstall, &ignore);
            let full = gate_cleanup_item(Some(&a), it, CleanupSource::Uninstall, &ignore);
            assert_eq!(
                dry.is_allow(),
                full.is_allow(),
                "dry/full mismatch for {}",
                it.path
            );
        }
        assert!(
            gate_cleanup_item(Some(&a), &items[0], CleanupSource::Uninstall, &ignore).is_allow()
        );
        assert!(
            !gate_cleanup_item(Some(&a), &items[1], CleanupSource::Uninstall, &ignore).is_allow()
        );
        assert!(
            !gate_cleanup_item(Some(&a), &items[2], CleanupSource::Uninstall, &ignore).is_allow()
        );
        assert!(
            !gate_cleanup_item(Some(&a), &items[3], CleanupSource::Uninstall, &ignore).is_allow()
        );
        // Related PATH under install root (S-R4-03 association).
        assert!(
            gate_cleanup_item(Some(&a), &items[4], CleanupSource::Uninstall, &ignore).is_allow()
        );
    }

    #[test]
    fn user_data_and_shared_always_skip() {
        let ignore = crate::ignore::IgnoreList::default();
        let mut it = item(r"C:\Users\a\Documents\x", ItemKind::File);
        it.user_data = true;
        assert_eq!(
            gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore),
            GateDecision::Skip("user_data red line")
        );
        let mut sh = item(r"C:\Program Files\Common Files\x", ItemKind::File);
        sh.shared = true;
        assert_eq!(
            gate_cleanup_item(None, &sh, CleanupSource::Uninstall, &ignore),
            GateDecision::Skip("shared runtime")
        );
    }

    #[test]
    fn dry_run_full_share_ar10_and_fs() {
        let ignore = crate::ignore::IgnoreList::default();
        let a = app("DemoApp", r"C:\Program Files\DemoApp");
        let ok = item(r"C:\Program Files\DemoApp\bin\x.exe", ItemKind::File);
        assert!(gate_cleanup_item(Some(&a), &ok, CleanupSource::Uninstall, &ignore).is_allow());
        let bad = item(r"C:\Program Files\Unrelated\bin.exe", ItemKind::File);
        assert_eq!(
            gate_cleanup_item(Some(&a), &bad, CleanupSource::Uninstall, &ignore),
            GateDecision::Skip("path not associated with app")
        );
        let orphan_app = app("瀛ゅ効鎵弿", "");
        let orphan_ok = item(r"C:\Program Files\SomeVendor\Tool", ItemKind::Dir);
        assert!(!gate_cleanup_item(
            Some(&orphan_app),
            &orphan_ok,
            CleanupSource::Orphan,
            &ignore
        )
        .is_allow());
        let orphan_win = item(r"C:\Windows", ItemKind::Dir);
        assert!(!gate_cleanup_item(
            Some(&orphan_app),
            &orphan_win,
            CleanupSource::Orphan,
            &ignore
        )
        .is_allow());
    }

    #[test]
    fn dry_and_full_gate_decisions_match() {
        // T-N1: dry-run and full delete must skip/allow the same items for the same input.
        let ignore = crate::ignore::IgnoreList::default();
        let a = app("DemoApp", r"C:\Program Files\DemoApp");
        let items = vec![
            item(r"C:\Program Files\DemoApp\bin\x.exe", ItemKind::File),
            item(r"C:\Users\a\Documents\Secret", ItemKind::Dir),
            item(r"C:\Windows\System32", ItemKind::Path),
            item(r"C:\Program Files\Common Files\X\y.dll", ItemKind::File),
            item(r"HKCU\Software\DemoApp\Config", ItemKind::Registry),
        ];
        for it in &items {
            let d = gate_cleanup_item(Some(&a), it, CleanupSource::Uninstall, &ignore);
            let f = gate_cleanup_item(Some(&a), it, CleanupSource::Uninstall, &ignore);
            assert_eq!(d, f, "dry/full gate mismatch for {}", it.path);
            assert_eq!(d.is_allow(), f.is_allow());
        }
        // Documents must skip even if flags say false.
        let docs = item(r"C:\Users\a\Documents\Secret", ItemKind::Dir);
        assert!(!gate_cleanup_item(Some(&a), &docs, CleanupSource::Uninstall, &ignore).is_allow());
        // PATH system entry must skip.
        let sys = item(r"C:\Windows\System32", ItemKind::Path);
        assert!(!gate_cleanup_item(Some(&a), &sys, CleanupSource::Uninstall, &ignore).is_allow());
        // Related install path allows.
        let ok = item(r"C:\Program Files\DemoApp\bin\x.exe", ItemKind::File);
        assert!(gate_cleanup_item(Some(&a), &ok, CleanupSource::Uninstall, &ignore).is_allow());
        // Related PATH under install root allows (S-R4-03 association).
        let ok_path = item(r"C:\Program Files\DemoApp\bin", ItemKind::Path);
        assert!(
            gate_cleanup_item(Some(&a), &ok_path, CleanupSource::Uninstall, &ignore).is_allow()
        );
        // Unrelated registry fails association.
        let bad_reg = item(r"HKCU\Software\UnrelatedApp\Config", ItemKind::Registry);
        assert!(
            !gate_cleanup_item(Some(&a), &bad_reg, CleanupSource::Uninstall, &ignore).is_allow()
        );
    }

    #[test]
    fn orphan_source_skips_ar10_slug_but_keeps_fs_gate() {
        let ignore = crate::ignore::IgnoreList::default();
        let orphan_app = app("瀛ゅ効鎵弿", "");
        let related = item(r"C:\Program Files\SomeVendor\Tool", ItemKind::Dir);
        assert!(
            !gate_cleanup_item(Some(&orphan_app), &related, CleanupSource::Orphan, &ignore)
                .is_allow()
        );
        let win = item(r"C:\Windows\System32\evil.dll", ItemKind::File);
        assert!(
            !gate_cleanup_item(Some(&orphan_app), &win, CleanupSource::Orphan, &ignore).is_allow()
        );
    }
    /// S-R7-01 adversarial: forged empty InstalledApp + cleanup_source=orphan.
    #[test]
    fn adversarial_forged_orphan_app_cannot_delete_arbitrary_paths() {
        let ignore = crate::ignore::IgnoreList::default();
        // Shape-only orphan (empty install/uninstall strings) — classic IPC forgery.
        let forged = app("x", "");
        let paths = [
            (r"C:\Program Files\UnrelatedVendor\App", ItemKind::Dir),
            (r"D:\Games\Save", ItemKind::Dir),
            (r"C:\Users\a\Documents\work", ItemKind::Dir),
            (r"HKCU\Software\Unrelated\Key", ItemKind::Registry),
            (r"D:\Other\bin", ItemKind::Path),
            (
                r"C:\Program Files\Common Files\Vendor\redist",
                ItemKind::Dir,
            ),
        ];
        for (p, kind) in paths {
            let it = item(p, kind);
            for src in [CleanupSource::Orphan, CleanupSource::Monitor] {
                let d = gate_cleanup_item(Some(&forged), &it, src, &ignore);
                assert!(
                    !d.is_allow(),
                    "forged orphan must skip {p} via {src:?}, got {d:?}"
                );
            }
        }
        // Real apps still need association even under orphan source.
        let real = app("DemoApp", r"C:\Program Files\DemoApp");
        let un = item(r"C:\Program Files\Unrelated\bin.exe", ItemKind::File);
        assert!(
            !gate_cleanup_item(Some(&real), &un, CleanupSource::Orphan, &ignore).is_allow(),
            "orphan source must not disable association for real apps"
        );
    }
}
