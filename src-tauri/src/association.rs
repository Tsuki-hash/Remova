//! Leftover鈫攁pp association (AR-10 / S-R4-03 / S-7R1).
//!
//! Answers one question: *does this leftover plausibly belong to this app?* `policy` consumes the
//! boolean verdicts; it deliberately knows nothing about how the evidence is weighed, so the
//! association matrix can be reasoned about and tested on its own.

use crate::scanner::{CleanupItem, ItemKind};

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

/// S-7R1: CF vendor association 鈥?vendor **directory segment** equals install prefix
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

/// Install roots that may act as association prefixes: deep enough, not a drive/library root.
fn is_safe_install_root(install: &str) -> bool {
    let norm = install.trim().replace('/', "\\");
    let p = norm.trim_end_matches('\\');
    if p.is_empty() {
        return false;
    }
    // Reject drive roots (`C:` / `C:\`) and very shallow trees (`C:\Users`).
    let segs: Vec<&str> = p.split('\\').filter(|s| !s.is_empty()).collect();
    if segs.len() < 4 {
        return false;
    }
    // Never treat profile library roots as install roots.
    let last = segs.last().copied().unwrap_or("").to_lowercase();
    const LIBS: &[&str] = &[
        "documents",
        "downloads",
        "desktop",
        "pictures",
        "videos",
        "music",
        "onedrive",
    ];
    if LIBS.contains(&last.as_str()) {
        return false;
    }
    if crate::safety::is_user_data_path(&p) {
        return false;
    }
    true
}

/// Light association for Registry / PATH leftovers when an installed app is known (S-R4-03).
/// S-3: never trust client `reason` 鈥?path / registry / publisher signals only.
fn non_fs_associated_with_app(app: &crate::apps::InstalledApp, item: &CleanupItem) -> bool {
    // S-R7-01: orphan-shaped apps must not claim arbitrary leftovers as associated.
    // Policy enforces the server-side orphan allow-list separately.
    if is_orphan_flow(app) {
        return false;
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
            // S-R7-01: orphan-shaped apps must not claim arbitrary FS paths as associated.
            // The orphan allow-list lives in policy (`was_recent_orphan_path`), not here.
            if is_orphan_flow(app) {
                return false;
            }
            let install = app
                .install_location
                .trim()
                .replace('/', "\\")
                .trim_end_matches('\\')
                .to_lowercase();
            let install_empty = install.is_empty() || !is_safe_install_root(&install);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::InstalledApp;
    use crate::scanner::{Confidence, RiskLevel};

    fn demo_app() -> InstalledApp {
        InstalledApp {
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
        }
    }

    fn orphan_app() -> InstalledApp {
        InstalledApp {
            name: "瀛ゅ効鎵弿".into(),
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
        }
    }

    fn probe(path: &str, kind: ItemKind) -> CleanupItem {
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
    fn cf_vendor_segment_and_association() {
        assert_eq!(
            common_files_vendor_segment(r"C:\Program Files\Common Files\Acme\lib.dll").as_deref(),
            Some("acme")
        );
        assert_eq!(
            common_files_vendor_segment(r"C:\Program Files\DemoApp"),
            None
        );
        // "." / ".." are not vendor names.
        assert_eq!(
            common_files_vendor_segment(r"C:\Program Files\Common Files\..\x"),
            None
        );

        let app = demo_app();
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
        let cf_app = InstalledApp {
            install_location: r"C:\Program Files\Common Files\Acme\Libs".into(),
            ..demo_app()
        };
        assert!(cf_vendor_associated(
            &cf_app,
            r"C:\Program Files\Common Files\Acme\Extra"
        ));
        assert!(!cf_vendor_associated(
            &app,
            r"C:\Program Files\Common Files\Acme\x"
        ));
        // Traversal inside a matching vendor folder still refuses.
        assert!(!cf_vendor_associated(
            &app,
            r"C:\Program Files\Common Files\DemoApp\..\Other\x"
        ));
        // Orphan flow never claims a CF vendor folder.
        assert!(!cf_vendor_associated(
            &orphan_app(),
            r"C:\Program Files\Common Files\DemoApp\x"
        ));
    }

    #[test]
    fn path_association_medium_gate() {
        let app = demo_app();
        assert!(path_associated_with_app(
            &app,
            &probe(r"C:\Program Files\DemoApp\bin\x.exe", ItemKind::File)
        ));
        assert!(path_associated_with_app(
            &app,
            &probe(r"C:\Users\a\AppData\Local\demoapp\cache", ItemKind::Dir)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(
                r"C:\Program Files\UnrelatedVendor\Tool\bin.exe",
                ItemKind::File
            )
        ));
        assert!(path_associated_with_app(
            &app,
            &probe(r"HKCU\Software\DemoApp\Config", ItemKind::Registry)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(r"HKCU\Software\UnrelatedVendor\Thing", ItemKind::Registry)
        ));
        assert!(path_associated_with_app(
            &app,
            &probe(r"C:\Program Files\DemoApp\bin", ItemKind::Path)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(r"D:\OtherApp\bin", ItemKind::Path)
        ));
    }

    #[test]
    fn association_ignores_forged_client_reason() {
        // S-3: `reason` is client-supplied and must never create an association.
        let app = demo_app();
        let mut it = probe(r"D:\Totally\Unrelated\bin", ItemKind::Path);
        it.reason = format!("belongs to {}", app.install_location);
        assert!(!path_associated_with_app(&app, &it));
    }

    #[test]
    fn association_fails_closed_on_empty_and_traversal() {
        let app = demo_app();
        assert!(!path_associated_with_app(
            &app,
            &probe("   ", ItemKind::File)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(r"C:\Program Files\DemoApp\..\..\Windows\x", ItemKind::File)
        ));
    }

    #[test]
    fn ar10_rejects_generic_short_slug_false_positives() {
        let app = InstalledApp {
            name: "Code".into(),
            publisher: "Microsoft".into(),
            install_location: r"C:\Program Files\Code".into(),
            ..demo_app()
        };
        // "code" is a stopword / too short 鈥?not enough by itself outside install root match.
        assert!(!path_associated_with_app(
            &app,
            &probe(r"C:\Users\a\AppData\Local\Temp\code-cache", ItemKind::Dir)
        ));
        // Install-location prefix still passes.
        assert!(path_associated_with_app(
            &app,
            &probe(r"C:\Program Files\Code\bin\code.exe", ItemKind::File)
        ));
    }

    #[test]
    fn orphan_flow_uses_safety_not_slug() {
        let app = orphan_app();
        assert!(is_orphan_flow(&app));
        // S-R7-01: orphan-shaped apps must NOT claim arbitrary FS/non-FS paths as associated.
        // The server-side orphan allow-list is enforced at the policy layer.
        assert!(!path_associated_with_app(
            &app,
            &probe(r"C:\Program Files\SomeVendor\Tool", ItemKind::Dir)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(r"C:\Windows", ItemKind::Dir)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(r"C:\Users\a\Documents", ItemKind::Dir)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(r"C:\Users\a\Documents\work", ItemKind::Dir)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(r"HKCU\Software\Unrelated\Thing", ItemKind::Registry)
        ));
        assert!(!path_associated_with_app(
            &app,
            &probe(r"D:\Other\bin", ItemKind::Path)
        ));
        // A real app that merely lacks an install_location is not an orphan flow.
        let reg_only = InstalledApp {
            name: "RealApp".into(),
            install_location: String::new(),
            registry_key: r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\X".into(),
            ..demo_app()
        };
        assert!(!is_orphan_flow(&reg_only));
    }
}
