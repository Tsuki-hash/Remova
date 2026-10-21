//! Registry association scans (PATH, shell, drivers, software keys, services, tasks).

use super::*;

pub(super) fn scan_path_env(
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    let mut seen = std::collections::HashSet::new();
    for scope in ["User", "Machine"] {
        if crate::scan_task::cancelled() {
            break;
        }
        // Read the real per-scope PATH (not the merged process env).
        // on read failure skip this scope — never fall back to process PATH
        // (that would mis-tag Machine/User scope).
        let Ok(raw) = crate::regops::read_path_scope_public(scope) else {
            continue;
        };
        for entry in raw.split(';') {
            if crate::scan_task::cancelled() {
                break;
            }
            let e = entry.trim();
            if e.is_empty() {
                continue;
            }
            let key = normalize_for_match(e);
            if key.len() < 3 || !seen.insert(format!("{scope}:{key}")) {
                continue;
            }
            let low = e.replace('/', "\\").to_lowercase();
            // Boundary-anchored install hit: `C:\Steam` must not claim the
            // `C:\SteamTools\bin` PATH entry.
            let hit_install = is_under_install(low.trim_end_matches('\\'), install_low);
            let hit_name = name_slugs
                .iter()
                .any(|n| n.len() >= 5 && super::name_boundary_hit(e, n));
            if !hit_install && !hit_name {
                continue;
            }
            let score = if hit_install { 60 } else { 40 };
            items.push(CleanupItem {
                path: e.to_string(),
                kind: ItemKind::Path,
                score,
                confidence: if hit_install {
                    Confidence::Confirmed
                } else {
                    Confidence::Suspected
                },
                risk: RiskLevel::Medium,
                reason: format!("PATH entry ({scope})"),
                evidence: vec![Evidence {
                    code: "path_env".into(),
                    label: "Environment PATH leftover".into(),
                    weight: score,
                    detail: e.chars().take(120).collect(),
                }],
                shared: false,
                service_bound: false,
                user_data: false,
                user_library: false,
                size_kb: None,
                size_capped: false,
                bucket: None,
            });
        }
    }
}

/// Direct subkeys of `SOFTWARE\Classes` that belong to Windows itself.
///
/// Deleting any of these breaks COM, shell integration or file associations
/// machine-wide. The registry gate has no rule for `SOFTWARE\Classes`, so the
/// scan has to refuse them before they are ever offered.
const SYSTEM_CLASSES_ROOTS: &[&str] = &[
    "CLSID",
    "Interface",
    "TypeLib",
    "AppID",
    "Component Categories",
    "Record",
    "Directory",
    "Folder",
    "Drive",
    "AllFilesystemObjects",
    "*",
    "Applications",
    "Local Settings",
    "MIME",
    "MEDIA",
    "Network",
    "SystemFileAssociations",
    "InternetShortcut",
    "Unknown",
    "Console",
];

/// Shell extension / CLSID / ProgID leftovers (per uninstall SOP).
pub(super) fn scan_shell_extensions(
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    let name_norms: Vec<String> = name_slugs.iter().map(|s| normalize_for_match(s)).collect();
    for alias in ["HKLM64", "HKLM32", "HKCU"] {
        if crate::scan_task::cancelled() {
            break;
        }
        let classes = format!(r"{alias}\SOFTWARE\Classes");
        for leaf in crate::regscan::list_subkeys(&classes) {
            if crate::scan_task::cancelled() {
                break;
            }
            let n = normalize_for_match(&leaf);
            if n.len() < 4 {
                continue;
            }
            // System Classes roots are never leftovers. An unbounded substring
            // match let a product slug that happened to be a substring of
            // `CLSID`, `Interface`, `TypeLib`, `Directory`, … offer the key for
            // deletion, and the registry gate has no rule for `SOFTWARE\Classes`.
            // Windows registry names are case-insensitive, so is the comparison.
            if SYSTEM_CLASSES_ROOTS
                .iter()
                .any(|root| root.eq_ignore_ascii_case(&leaf))
            {
                continue;
            }
            // Word-boundary match, like the CLSID branch below: `network` must
            // not claim `NetworkSettings`, and a four-letter product token must
            // not claim an extension key.
            if !name_norms
                .iter()
                .any(|s| s.len() >= 4 && super::name_boundary_hit(&leaf, s))
            {
                continue;
            }
            let key = format!(r"{classes}\{leaf}");
            if is_safe_to_delete_registry(&key).is_err() {
                continue;
            }
            items.push(CleanupItem {
                path: key,
                kind: ItemKind::Registry,
                score: 35,
                confidence: Confidence::Suspected,
                risk: RiskLevel::Medium,
                reason: format!("Shell/Classes leftover: {leaf}"),
                evidence: vec![Evidence {
                    code: "shell_class".into(),
                    label: "Classes key name matches product".into(),
                    weight: 35,
                    detail: leaf,
                }],
                shared: false,
                service_bound: false,
                user_data: false,
                user_library: false,
                size_kb: None,
                size_capped: false,
                bucket: None,
            });
        }
        // HKLM32 selects the 32-bit view itself. Do not walk its explicit
        // Wow6432Node again. Per-user 32-bit registrations have no HKCU32
        // alias in our reader, so keep their explicit root once.
        let has_strong = (!install_low.is_empty()
            && install_low
                .trim_end_matches('\\')
                .split('\\')
                .filter(|s| !s.is_empty())
                .count()
                >= 2)
            || name_norms.iter().any(|s| s.len() >= 5);
        if !has_strong {
            continue;
        }
        for root in clsid_roots(alias) {
            if crate::scan_task::cancelled() {
                break;
            }
            for leaf in crate::regscan::list_subkeys(&root) {
                if crate::scan_task::cancelled() {
                    break;
                }
                let def = crate::regscan::read_string_default(&format!(r"{root}\{leaf}"))
                    .unwrap_or_default();
                let name_hit = name_slugs
                    .iter()
                    .any(|s| s.len() >= 5 && super::name_boundary_hit(&format!("{leaf} {def}"), s));
                // boundary-anchored on the raw default string — the
                // normalized contains form matched any class whose default merely
                // contained the install slug (`C:\App` → `capp` in "captured").
                let install_hit = super::cmdline_refs_install(&def, install_low);
                if !name_hit && !install_hit {
                    continue;
                }
                let key = format!(r"{root}\{leaf}");
                if is_safe_to_delete_registry(&key).is_err() {
                    continue;
                }
                items.push(CleanupItem {
                    path: key,
                    kind: ItemKind::Registry,
                    score: 40,
                    confidence: Confidence::Suspected,
                    risk: RiskLevel::High,
                    reason: format!("CLSID leftover: {leaf}"),
                    evidence: vec![Evidence {
                        code: "shell_clsid".into(),
                        label: "CLSID description matches product".into(),
                        weight: 40,
                        detail: def.chars().take(80).collect(),
                    }],
                    shared: false,
                    service_bound: false,
                    user_data: false,
                    user_library: false,
                    size_kb: None,
                    size_capped: false,
                    bucket: None,
                });
            }
        }
    }
}

fn clsid_roots(alias: &str) -> Vec<String> {
    let mut roots = vec![format!(r"{alias}\SOFTWARE\Classes\CLSID")];
    if alias == "HKCU" {
        roots.push(format!(r"{alias}\SOFTWARE\Classes\Wow6432Node\CLSID"));
    }
    roots
}

/// Kernel/file-system drivers under Services (Type=1) (per uninstall SOP).
pub(super) fn scan_drivers(name_slugs: &[String], install_low: &str, items: &mut Vec<CleanupItem>) {
    let root = r"HKLM64\SYSTEM\CurrentControlSet\Services";
    for svc in crate::regscan::list_subkeys(root) {
        if crate::scan_task::cancelled() {
            break;
        }
        let svc_path = format!(r"{root}\{svc}");
        let Some(svc_type) = crate::regscan::read_dword(&svc_path, "Type") else {
            continue;
        };
        // 1 = kernel driver, 2 = file system driver
        if svc_type != 1 && svc_type != 2 {
            continue;
        }
        if is_safe_to_delete_registry(&svc_path).is_err() {
            continue;
        }
        let image = service_image_string(&svc_path);
        let hit_install = super::cmdline_refs_install(&image, install_low);
        let strong = hit_install
            || name_slugs
                .iter()
                .any(|n| n.len() >= 5 && super::name_boundary_hit(&svc, n));
        if !strong {
            continue;
        }
        items.push(CleanupItem {
            path: svc_path,
            kind: ItemKind::Registry,
            score: 45,
            confidence: Confidence::Suspected,
            risk: RiskLevel::High,
            reason: format!("Driver leftover: {svc}"),
            evidence: vec![Evidence {
                code: "driver".into(),
                label: "System driver matches product".into(),
                weight: 45,
                detail: image.chars().take(120).collect(),
            }],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
}

pub(super) fn scan_software_keys(name_slugs: &[String], items: &mut Vec<CleanupItem>) {
    scan_software_keys_with(name_slugs, items, crate::regscan::target_exists);
}

fn scan_software_keys_with(
    name_slugs: &[String],
    items: &mut Vec<CleanupItem>,
    exists: impl Fn(&str) -> Result<bool, String>,
) {
    let pubs: [(&str, &str); 3] = [
        ("HKLM64", r"SOFTWARE"),
        ("HKLM32", r"SOFTWARE"),
        ("HKCU", r"SOFTWARE"),
    ];
    for (alias, sub) in pubs {
        if crate::scan_task::cancelled() {
            break;
        }
        let root = format!("{alias}\\{sub}");
        for slug in name_slugs.iter().take(3) {
            if crate::scan_task::cancelled() {
                break;
            }
            let key = format!("{root}\\{slug}");
            if is_safe_to_delete_registry(&key).is_err() {
                continue;
            }
            if !matches!(exists(&key), Ok(true)) {
                continue;
            }
            let score = 40;
            let (confidence, risk) = finalize_score(score);
            items.push(CleanupItem {
                path: key,
                kind: ItemKind::Registry,
                score,
                confidence,
                risk,
                reason: format!("Software key: {slug}"),
                evidence: vec![Evidence {
                    code: "software_key_exact".into(),
                    label: "HKLM/HKCU Software\\Product".into(),
                    weight: score,
                    detail: slug.clone(),
                }],
                shared: false,
                service_bound: false,
                user_data: false,
                user_library: false,
                size_kb: None,
                size_capped: false,
                bucket: None,
            });
        }
    }
}

pub(super) fn scan_services(
    name_slugs: &[String],
    exe_stems: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    let root = r"HKLM64\SYSTEM\CurrentControlSet\Services";
    for svc in crate::regscan::list_subkeys(root) {
        if crate::scan_task::cancelled() {
            break;
        }
        let svc_path = format!("{root}\\{svc}");
        if is_safe_to_delete_registry(&svc_path).is_err() {
            continue;
        }
        let image = service_image_string(&svc_path);
        // Boundary-anchored install hit on the ImagePath only — a display
        // string or service name that merely contains the install prefix
        // must not look like an install reference.
        let hit_install = super::cmdline_refs_install(&image, install_low);
        let strong = hit_install
            || name_slugs
                .iter()
                .any(|n| n.len() >= 6 && super::name_boundary_hit(&svc, n));
        let _ = exe_stems;
        if !strong {
            continue;
        }
        // force HIGH suspected regardless of score thresholds
        let score = 40;
        items.push(CleanupItem {
            path: svc_path,
            kind: ItemKind::Registry,
            score,
            confidence: Confidence::Suspected,
            risk: RiskLevel::High,
            reason: format!("Windows service leftover: {svc}"),
            evidence: vec![Evidence {
                code: "windows_service".into(),
                label: "Service matches product (high risk, unselected)".into(),
                weight: score,
                detail: image.chars().take(120).collect(),
            }],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
}

/// Executable path from a service `ImagePath` (quotes and arguments stripped).
fn service_image_exe(image: &str) -> Option<std::path::PathBuf> {
    let trimmed = image.trim();
    if trimmed.is_empty() || trimmed.contains('%') {
        // Unexpanded system variables belong to Windows services, never to an
        // app leftover; skipping avoids matching them against user paths.
        return None;
    }
    let exe = if let Some(rest) = trimmed.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        trimmed.split_whitespace().next()?
    };
    if exe.len() <= 3 {
        return None;
    }
    Some(std::path::PathBuf::from(exe))
}

/// Mark app-like candidates whose directory hosts a registered service image.
///
/// SOP §2.3: a directory referenced by a service is a service data directory,
/// not a leftover — deleting it breaks a still-installed service. The match is
/// component-wise on the resolved key, so `C:\App` does not claim `C:\AppData`.
pub(super) fn mark_service_bound(items: &mut [CleanupItem]) {
    let mut service_keys: Vec<String> = Vec::new();
    let root = r"HKLM64\SYSTEM\CurrentControlSet\Services";
    for svc in crate::regscan::list_subkeys(root) {
        if crate::scan_task::cancelled() {
            break;
        }
        let image = service_image_string(&format!("{root}\\{svc}"));
        if let Some(exe) = service_image_exe(&image) {
            if let Some(key) = crate::fsutil::path_key(&exe) {
                service_keys.push(key);
            }
        }
    }
    mark_service_bound_with(items, &service_keys);
}

fn mark_service_bound_with(items: &mut [CleanupItem], service_keys: &[String]) {
    if service_keys.is_empty() {
        return;
    }
    for item in items.iter_mut() {
        if !matches!(item.kind, ItemKind::Dir | ItemKind::File) {
            continue;
        }
        let Some(key) = crate::fsutil::path_key(std::path::Path::new(&item.path)) else {
            continue;
        };
        let hit = service_keys
            .iter()
            .find(|svc| svc.as_str() == key || svc.starts_with(&format!("{key}\\")));
        if let Some(svc) = hit {
            item.service_bound = true;
            item.evidence.push(Evidence {
                code: "service_bound".into(),
                label: "Registered service uses this path".into(),
                weight: 0,
                detail: svc.chars().take(120).collect(),
            });
        }
    }
}

/// M4 (SOP §8-2): MuiCache keeps `FriendlyAppName` values keyed by the full
/// executable path. Only entries whose executable is gone are reported, and
/// only when the path/name matches the app being analyzed.
pub(super) fn scan_muicache(
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    const KEY: &str =
        r"HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache";
    let normalized: Vec<String> = name_slugs
        .iter()
        .map(|s| super::normalize_for_match(s))
        .filter(|s| s.len() >= 4)
        .collect();
    if normalized.is_empty() {
        return;
    }
    for (value_name, _data) in crate::regscan::list_values(KEY) {
        if crate::scan_task::cancelled() {
            break;
        }
        if !muicache_matches(&value_name, &normalized, install_low, |p| {
            p.try_exists().is_ok_and(|exists| !exists)
        }) {
            continue;
        }
        let exe = value_name
            .strip_suffix(".FriendlyAppName")
            .unwrap_or(&value_name);
        items.push(CleanupItem {
            path: format!("{KEY}|{value_name}"),
            kind: ItemKind::Registry,
            score: 30,
            confidence: Confidence::Suspected,
            risk: RiskLevel::Medium,
            reason: "MuiCache trace (executable missing)".into(),
            evidence: vec![Evidence {
                code: "muicache_trace".into(),
                label: "Shell friendly-name cache for a missing executable".into(),
                weight: 0,
                detail: exe.chars().take(160).collect(),
            }],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: Some("registry".into()),
        });
    }
}

/// True when a MuiCache value names a missing executable that belongs to this
/// app (path under the install root, or an exe stem matching a product slug).
fn muicache_matches(
    value_name: &str,
    normalized_slugs: &[String],
    install_low: &str,
    missing: impl Fn(&std::path::Path) -> bool,
) -> bool {
    let Some(exe) = value_name.strip_suffix(".FriendlyAppName") else {
        return false;
    };
    let install_low = install_low.trim_end_matches('\\');
    let exe_low = exe.to_lowercase();
    let path_hit = !install_low.is_empty()
        && (exe_low == install_low || exe_low.starts_with(&format!("{install_low}\\")));
    let name_hit = std::path::Path::new(exe)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .map(|s| super::normalize_for_match(&s))
        .is_some_and(|stem| {
            normalized_slugs.iter().any(|slug| {
                stem == *slug
                    || stem.starts_with(&format!("{slug}-"))
                    || stem.starts_with(&format!("{slug}."))
            })
        });
    (path_hit || name_hit) && missing(std::path::Path::new(exe))
}

pub(super) fn scan_scheduled_tasks(name_slugs: &[String], items: &mut Vec<CleanupItem>) {
    let root = r"HKLM64\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree";
    for top in crate::regscan::list_subkeys(root) {
        if crate::scan_task::cancelled() {
            break;
        }
        if top.eq_ignore_ascii_case("microsoft") {
            continue;
        }
        let key_path = format!("{root}\\{top}");
        if is_safe_to_delete_registry(&key_path).is_err() {
            continue;
        }
        // TaskCache\Tree key paths never contain file-system install
        // paths, and the action string lives in a binary blob under
        // TaskCache\Actions — install evidence here is the task NAME only.
        let strong = name_slugs
            .iter()
            .any(|n| n.len() >= 6 && super::name_boundary_hit(&top, n));
        if !strong {
            continue;
        }
        let score = 45;
        items.push(CleanupItem {
            path: key_path,
            kind: ItemKind::Registry,
            score,
            confidence: Confidence::Suspected,
            risk: RiskLevel::High,
            reason: format!("Scheduled task leftover: {top}"),
            evidence: vec![Evidence {
                code: "scheduled_task".into(),
                label: "Task name matches product (high risk, unselected)".into(),
                weight: score,
                detail: top,
            }],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
}

/// Service/driver `ImagePath` is a VALUE under the service key — read it with
/// `read_string`, never as a subkey default (the subkey form always
/// opened nothing and silently killed the install-location channel).
fn service_image_string(svc_path: &str) -> String {
    // REG_EXPAND_SZ ImagePath must be expanded before install matching or
    // `%ProgramFiles%\Vendor\…` never hits the install root.
    crate::regops::expand_env_string(
        &crate::regscan::read_string(svc_path, "ImagePath").unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancelled_software_scan_skips_subsequent_registry_reads() {
        let id = crate::scan_task::begin().unwrap();
        let result = crate::scan_task::run(id, || {
            crate::scan_task::cancel(id).unwrap();
            let mut items = Vec::new();
            super::scan_software_keys_with(&["sample".into()], &mut items, |_| {
                panic!("read after cancellation")
            });
            assert!(items.is_empty());
        });
        assert_eq!(result, Err("scan:cancelled".into()));
    }

    #[test]
    fn software_scan_uses_checked_existence_without_content_requirements() {
        let slugs = vec!["OwnedProductFixture".to_string()];
        for state in [Ok(true), Ok(false), Err("access denied".to_string())] {
            let mut items = Vec::new();
            super::scan_software_keys_with(&slugs, &mut items, |_| state.clone());
            assert_eq!(items.len(), if state == Ok(true) { 3 } else { 0 });
        }
    }
    /// The scan used to match `SOFTWARE\Classes` subkeys with an unbounded
    /// substring, and the registry gate has no rule for that root — so a product
    /// slug that was a substring of a system key offered it for deletion.
    /// Deleting `HKLM\SOFTWARE\Classes\CLSID` or `\Interface` breaks COM and file
    /// associations machine-wide.
    #[test]
    fn system_classes_roots_are_never_leftovers() {
        // Every entry must actually be on the deny-list, matched the way the
        // scan matches it — case-insensitively, since registry names are.
        for root in [
            "CLSID",
            "Interface",
            "TypeLib",
            "Directory",
            "Folder",
            "Drive",
            "AllFilesystemObjects",
            "Applications",
            "MIME",
            "MEDIA",
            "Media",
            "media",
            "Network",
            "Record",
            "Unknown",
            "Local Settings",
            "SystemFileAssociations",
            "Component Categories",
            "InternetShortcut",
            "AppID",
            "Console",
            "*",
        ] {
            assert!(
                super::SYSTEM_CLASSES_ROOTS
                    .iter()
                    .any(|r| r.eq_ignore_ascii_case(root)),
                "{root} must be denied as a system Classes root"
            );
        }
        // A genuine vendor ProgID is not on the list, so it can still be found.
        for vendor in ["DemoApp.File", "DemoApp", "AcmeEditor.1"] {
            assert!(
                !super::SYSTEM_CLASSES_ROOTS
                    .iter()
                    .any(|r| r.eq_ignore_ascii_case(vendor)),
                "{vendor} must remain discoverable"
            );
        }
    }

    #[test]
    fn clsid_roots_do_not_duplicate_machine_views() {
        assert_eq!(
            super::clsid_roots("HKLM64"),
            [r"HKLM64\SOFTWARE\Classes\CLSID"]
        );
        assert_eq!(
            super::clsid_roots("HKLM32"),
            [r"HKLM32\SOFTWARE\Classes\CLSID"]
        );
        assert_eq!(super::clsid_roots("HKCU").len(), 2);
    }
    /// opt-in against the real registry (`cargo test -- --ignored`).
    /// A healthy machine has many services with non-empty ImagePath strings; the
    /// old read-as-subkey form returned empty for every single one.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn service_image_string_reads_real_image_paths() {
        let root = r"HKLM64\SYSTEM\CurrentControlSet\Services";
        let services = crate::regscan::list_subkeys(root);
        assert!(
            !services.is_empty(),
            "no services listed — registry read broken"
        );
        let mut non_empty = 0;
        for svc in &services {
            let image = super::service_image_string(&format!(r"{root}\{svc}"));
            if image.is_empty() {
                continue;
            }
            non_empty += 1;
            // Real ImagePath values look like paths or driver names, not junk.
            assert!(
                image.contains('\\') || image.contains(".sys") || image.contains(".exe"),
                "unexpected ImagePath shape for {svc}: {image:?}"
            );
        }
        assert!(
            non_empty >= 1,
            "expected at least one real ImagePath read, got {non_empty}"
        );
    }

    /// M4 (SOP §8-2): MuiCache entries are only offered when the executable is
    /// gone and the path/stem belongs to the app being analyzed.
    #[test]
    fn muicache_matches_only_missing_owned_executables() {
        let slugs = vec!["acmetool".to_string()];
        let missing = |_p: &std::path::Path| true;
        let present = |_p: &std::path::Path| false;
        assert!(super::muicache_matches(
            r"C:\Tools\AcmeTool.exe.FriendlyAppName",
            &slugs,
            "",
            missing
        ));
        assert!(super::muicache_matches(
            r"C:\Program Files\Acme\bin\launcher.exe.FriendlyAppName",
            &slugs,
            r"c:\program files\acme\",
            missing
        ));
        assert!(!super::muicache_matches(
            r"C:\Tools\AcmeTool.exe.FriendlyAppName",
            &slugs,
            "",
            present
        ));
        // Install-root matching is segment-boundary anchored.
        assert!(!super::muicache_matches(
            r"C:\Program Files\AcmeTool\launcher.exe.FriendlyAppName",
            &slugs,
            r"c:\program files\acme",
            missing
        ));
        assert!(!super::muicache_matches(
            r"C:\Tools\Other.exe.FriendlyAppName",
            &slugs,
            "",
            missing
        ));
        assert!(!super::muicache_matches(
            r"C:\Tools\AcmeTool.exe.ApplicationCompany",
            &slugs,
            "",
            missing
        ));
    }

    /// M2 (SOP §2.3): a candidate directory hosting a registered service image
    /// is keep-class; matching is component-wise, so `C:\App` does not claim
    /// the sibling `C:\AppData`.
    #[test]
    fn service_bound_matching_is_component_wise() {
        use crate::scanner::{CleanupItem, Confidence, ItemKind, RiskLevel};
        let mk = |path: &str, kind: ItemKind| CleanupItem {
            path: path.into(),
            kind,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
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
        let mut items = vec![
            mk(r"C:\App", ItemKind::Dir),
            mk(r"C:\AppData", ItemKind::Dir),
            mk(r"C:\App\svc.exe", ItemKind::File),
        ];
        super::mark_service_bound_with(&mut items, &[r"c:\app\svc.exe".to_string()]);
        assert!(
            items[0].service_bound,
            "the hosting directory must be bound"
        );
        assert!(
            !items[1].service_bound,
            "a prefix sibling must not be bound"
        );
        assert!(
            items[2].service_bound,
            "the service image itself must be bound"
        );
    }

    #[test]
    fn service_image_exe_strips_quotes_and_arguments() {
        assert_eq!(
            super::service_image_exe(r#""C:\Program Files\Vendor\svc.exe" -k arg"#),
            Some(std::path::PathBuf::from(r"C:\Program Files\Vendor\svc.exe"))
        );
        assert_eq!(
            super::service_image_exe(r"C:\Windows\system32\svchost.exe -k netsvcs"),
            Some(std::path::PathBuf::from(r"C:\Windows\system32\svchost.exe"))
        );
        assert!(super::service_image_exe(r"%SystemRoot%\system32\x.exe").is_none());
    }
}
