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

/// Shared leftover gate for dry-run and full delete (S-05 / A-02).
/// Order: user_data → shared → ignore → AR-10/orphan association → safety fs/reg.
pub fn gate_cleanup_item(
    app: Option<&InstalledApp>,
    item: &CleanupItem,
    _source: CleanupSource,
    ignore: &crate::ignore::IgnoreList,
) -> GateDecision {
    if item.user_data {
        return GateDecision::Skip("user_data red line");
    }
    if item.shared {
        return GateDecision::Skip("shared runtime");
    }
    if crate::ignore::should_skip_leftover_path(ignore, &item.path) {
        return GateDecision::Skip("ignored path");
    }

    if let Some(app) = app {
        if matches!(item.kind, ItemKind::File | ItemKind::Dir) {
            // path_associated_with_app: orphan → is_safe_fs only; install app → AR-10.
            if !crate::executor::path_associated_with_app(app, item) {
                return GateDecision::Skip("path not associated with app");
            }
        }
    }

    match item.kind {
        ItemKind::Registry => {
            if crate::safety::is_safe_to_delete_registry(&item.path).is_err() {
                return GateDecision::Skip("failed safety gate");
            }
        }
        ItemKind::Path => {
            if item.path.trim().is_empty() {
                return GateDecision::Skip("failed safety gate");
            }
        }
        ItemKind::File | ItemKind::Dir => {
            if !crate::safety::is_safe_fs(std::path::Path::new(&item.path)) {
                return GateDecision::Skip("failed safety gate");
            }
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
}
