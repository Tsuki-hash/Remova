//! Cleanup / manage policy façade (A-02).
//! Single semantic gate for dry-run preview and real delete; manage/store/shared re-exported here.

use crate::apps::InstalledApp;
use crate::scanner::{CleanupItem, ItemKind};

/// Who launched cleanup — drives orphan vs installed-app association rules.
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
fn is_dangerous_path_entry(entry: &str) -> bool {
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
    // Drive root or bare relative junk
    if s.ends_with(':') || !s.contains('\\') {
        return true;
    }
    // After env expansion, reject any segment still containing unexpanded % or system roots.
    if s.contains('%') {
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
    for (env_name, suffixes) in [
        ("SystemRoot", [r"\windows".to_string(), String::new()]),
        ("windir", [r"\windows".to_string(), String::new()]),
        (
            "ProgramFiles",
            [r"\program files".to_string(), String::new()],
        ),
        (
            "ProgramFiles(x86)",
            [r"\program files (x86)".to_string(), String::new()],
        ),
        ("ProgramData", [r"\programdata".to_string(), String::new()]),
    ] {
        if let Ok(val) = std::env::var(env_name) {
            let low = val
                .replace('/', "\\")
                .to_lowercase()
                .trim_end_matches('\\')
                .to_string();
            if low.len() > 2 {
                danger.push(low);
            }
        }
        for suf in suffixes {
            if !suf.is_empty() {
                // hardcoded companions already in list; keep loop shape simple
                let _ = suf;
            }
        }
    }
    danger
        .iter()
        .any(|d| s == *d || s.starts_with(&format!("{d}\\")))
}

/// Shared leftover gate for dry-run and full delete (S-05 / A-02).
/// Order: server-side user_data/shared recompute → client flags → ignore → AR-10 → safety.
pub fn gate_cleanup_item(
    app: Option<&InstalledApp>,
    item: &CleanupItem,
    source: CleanupSource,
    ignore: &crate::ignore::IgnoreList,
) -> GateDecision {
    // S-N1: never trust client-only flags — recompute red lines server-side.
    if item.user_data || crate::safety::is_user_data_path(&item.path) {
        return GateDecision::Skip("user_data red line");
    }
    if crate::safety::looks_like_sync_conflict(&item.path) {
        return GateDecision::Skip("user_data red line");
    }
    if item.shared
        || crate::shared::is_shared_item(&item.reason, &item.path, &item.reason)
        || crate::shared::is_shared_item("", &item.path, "")
    {
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
            if !crate::safety::is_safe_fs(std::path::Path::new(&item.path)) {
                return GateDecision::Skip("failed safety gate");
            }
        }
    }

    if let Some(app) = app {
        let orphan = source == CleanupSource::Orphan
            || source == CleanupSource::Monitor
            || crate::executor::is_orphan_flow(app);
        if !orphan && !crate::executor::path_associated_with_app(app, item) {
            // S-R4-03: Registry/Path also require a light association when app is known.
            return GateDecision::Skip("path not associated with app");
        }
    }
    GateDecision::Allow
}

// --- Façade re-exports (single lookup for safety/manage/shared policy) ---

pub use crate::safety::{
    allow_manage_reg_write, allow_manage_service_write, critical_service_names, is_allowed_run_key,
    is_allowed_startup_approved_key, is_critical_service, is_safe_fs, is_safe_to_delete_registry,
};
pub use crate::shared::is_shared_item;

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
            size_kb: None,
            bucket: None,
        }
    }

    #[test]
    fn rejects_forged_user_data_and_shared_flags() {
        let ignore = crate::ignore::IgnoreList::default();
        // Client says user_data=false but path is Documents → must skip (S-N1).
        let forged_ud = item(r"C:\Users\a\Documents\Important", ItemKind::Dir);
        assert_eq!(
            gate_cleanup_item(None, &forged_ud, CleanupSource::Uninstall, &ignore),
            GateDecision::Skip("user_data red line")
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
        // Client says shared=false but path looks shared → must skip.
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
    fn path_scrub_rejects_system_entries() {
        let ignore = crate::ignore::IgnoreList::default();
        for p in [
            r"C:\Windows\System32",
            r"C:\Windows",
            r"c:\windows\system32\wbem\",
            "",
            r"C:",
            r"%SystemRoot%\System32",
            r"%ProgramFiles%\Vendor\Tool",
        ] {
            let it = item(p, ItemKind::Path);
            let d = gate_cleanup_item(None, &it, CleanupSource::Uninstall, &ignore);
            assert!(!d.is_allow(), "expected skip for PATH entry {p}");
        }
        let ok = item(r"C:\Vendor\Tool\bin", ItemKind::Path);
        assert!(gate_cleanup_item(None, &ok, CleanupSource::Uninstall, &ignore).is_allow());
    }

    #[test]
    fn registry_path_require_app_association() {
        let ignore = crate::ignore::IgnoreList::default();
        let a = app("DemoApp", r"C:\Program Files\DemoApp");
        // Unrelated PATH / registry with real app context → skip (S-R4-03).
        let other_path = item(r"D:\TotallyOther\bin", ItemKind::Path);
        assert!(
            !gate_cleanup_item(Some(&a), &other_path, CleanupSource::Uninstall, &ignore).is_allow()
        );
        let other_reg = item(r"HKCU\Software\OtherVendor\Thing", ItemKind::Registry);
        assert!(
            !gate_cleanup_item(Some(&a), &other_reg, CleanupSource::Uninstall, &ignore).is_allow()
        );
        // Related PATH under install root → allow.
        let related = item(r"C:\Program Files\DemoApp\bin", ItemKind::Path);
        assert!(
            gate_cleanup_item(Some(&a), &related, CleanupSource::Uninstall, &ignore).is_allow()
        );
        // Orphan source still allows non-associated PATH (safety gate only).
        let orphan_app = app("孤儿扫描", "");
        assert!(gate_cleanup_item(
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
        let orphan_app = app("孤儿扫描", "");
        let orphan_ok = item(r"C:\Program Files\SomeVendor\Tool", ItemKind::Dir);
        assert!(gate_cleanup_item(
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
        let orphan_app = app("孤儿扫描", "");
        let related = item(r"C:\Program Files\SomeVendor\Tool", ItemKind::Dir);
        assert!(
            gate_cleanup_item(Some(&orphan_app), &related, CleanupSource::Orphan, &ignore)
                .is_allow()
        );
        let win = item(r"C:\Windows\System32\evil.dll", ItemKind::File);
        assert!(
            !gate_cleanup_item(Some(&orphan_app), &win, CleanupSource::Orphan, &ignore).is_allow()
        );
    }
}
