//! Registry association scans (PATH, shell, drivers, software keys, services, tasks).

use super::*;

pub(super) fn scan_path_env(
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    let name_norms: Vec<String> = name_slugs.iter().map(|s| normalize_for_match(s)).collect();
    let mut seen = std::collections::HashSet::new();
    for scope in ["User", "Machine"] {
        // Read the real per-scope PATH (not the merged process env).
        // REV-BE-13: on read failure skip this scope — never fall back to process PATH
        // (that would mis-tag Machine/User scope).
        let Ok(raw) = crate::regops::read_path_scope_public(scope) else {
            continue;
        };
        for entry in raw.split(';') {
            let e = entry.trim();
            if e.is_empty() {
                continue;
            }
            let key = normalize_for_match(e);
            if key.len() < 3 || !seen.insert(format!("{scope}:{key}")) {
                continue;
            }
            let low = e.replace('/', "\\").to_lowercase();
            let hit_install = !install_low.is_empty() && low.contains(install_low);
            let hit_name = name_norms
                .iter()
                .any(|n| n.len() >= 5 && normalize_for_match(e).contains(n));
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
                user_data: false,
                user_library: false,
                size_kb: None,
                bucket: None,
            });
        }
    }
}

/// Shell extension / CLSID / ProgID leftovers (per uninstall SOP).
pub(super) fn scan_shell_extensions(
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    let name_norms: Vec<String> = name_slugs.iter().map(|s| normalize_for_match(s)).collect();
    for alias in ["HKLM64", "HKLM32", "HKCU"] {
        let classes = format!(r"{alias}\SOFTWARE\Classes");
        for leaf in crate::regscan::list_subkeys(&classes) {
            let n = normalize_for_match(&leaf);
            if n.len() < 4 {
                continue;
            }
            if !name_norms
                .iter()
                .any(|s| s.len() >= 4 && n.contains(s.as_str()))
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
                user_data: false,
                user_library: false,
                size_kb: None,
                bucket: None,
            });
        }
        for root in [
            format!(r"{alias}\SOFTWARE\Classes\CLSID"),
            format!(r"{alias}\SOFTWARE\Classes\Wow6432Node\CLSID"),
        ] {
            // PERF-1: skip full CLSID walk without a strong product token.
            let has_strong = !install_low.is_empty() || name_norms.iter().any(|s| s.len() >= 5);
            if !has_strong {
                continue;
            }
            for leaf in crate::regscan::list_subkeys(&root) {
                let def = crate::regscan::read_string_default(&format!(r"{root}\{leaf}"))
                    .unwrap_or_default();
                let blob = normalize_for_match(&format!("{leaf} {def}"));
                let name_hit = name_norms
                    .iter()
                    .any(|s| s.len() >= 5 && blob.contains(s.as_str()));
                let install_hit =
                    !install_low.is_empty() && blob.contains(&normalize_for_match(install_low));
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
                    user_data: false,
                    user_library: false,
                    size_kb: None,
                    bucket: None,
                });
            }
        }
    }
}

/// Kernel/file-system drivers under Services (Type=1) (per uninstall SOP).
pub(super) fn scan_drivers(name_slugs: &[String], install_low: &str, items: &mut Vec<CleanupItem>) {
    let root = r"HKLM64\SYSTEM\CurrentControlSet\Services";
    let name_norms: Vec<String> = name_slugs.iter().map(|s| normalize_for_match(s)).collect();
    for svc in crate::regscan::list_subkeys(root) {
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
        let image = crate::regscan::read_string_default(&format!(r"{svc_path}\ImagePath"))
            .unwrap_or_default();
        let blob = format!("{svc} {image}").to_lowercase();
        let hit_install = !install_low.is_empty() && blob.contains(install_low);
        let svc_n = normalize_for_match(&svc);
        let strong = hit_install
            || name_norms
                .iter()
                .any(|n| n.len() >= 5 && (n == &svc_n || svc_n.contains(n.as_str())));
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
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        });
    }
}

pub(super) fn scan_software_keys(name_slugs: &[String], items: &mut Vec<CleanupItem>) {
    let pubs: [(&str, &str); 3] = [
        ("HKLM64", r"SOFTWARE"),
        ("HKLM32", r"SOFTWARE"),
        ("HKCU", r"SOFTWARE"),
    ];
    for (alias, sub) in pubs {
        let root = format!("{alias}\\{sub}");
        for slug in name_slugs.iter().take(3) {
            let key = format!("{root}\\{slug}");
            if is_safe_to_delete_registry(&key).is_err() {
                continue;
            }
            if crate::regscan::list_subkeys(&key).is_empty()
                && crate::regscan::list_values(&key).is_empty()
            {
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
                user_data: false,
                user_library: false,
                size_kb: None,
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
    let name_norms: Vec<String> = name_slugs.iter().map(|s| normalize_for_match(s)).collect();
    for svc in crate::regscan::list_subkeys(root) {
        let svc_path = format!("{root}\\{svc}");
        if is_safe_to_delete_registry(&svc_path).is_err() {
            continue;
        }
        let display = crate::regscan::read_string_default(&format!(r"{svc_path}\DisplayName"))
            .unwrap_or_default();
        let image = crate::regscan::read_string_default(&format!(r"{svc_path}\ImagePath"))
            .unwrap_or_default();
        let blob = format!("{svc} {display} {image}").to_lowercase();
        let hit_install = !install_low.is_empty() && blob.contains(install_low);
        let svc_n = normalize_for_match(&svc);
        let strong = hit_install
            || name_norms
                .iter()
                .any(|n| n.len() >= 6 && (n == &svc_n || svc_n.contains(n.as_str())));
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
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        });
    }
}

pub(super) fn scan_scheduled_tasks(
    name_slugs: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    let root = r"HKLM64\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree";
    let name_norms: Vec<String> = name_slugs.iter().map(|s| normalize_for_match(s)).collect();
    for top in crate::regscan::list_subkeys(root) {
        if top.eq_ignore_ascii_case("microsoft") {
            continue;
        }
        let key_path = format!("{root}\\{top}");
        if is_safe_to_delete_registry(&key_path).is_err() {
            continue;
        }
        let leaf_n = normalize_for_match(&top);
        let hit_install = !install_low.is_empty() && key_path.to_lowercase().contains(install_low);
        let strong = hit_install
            || name_norms
                .iter()
                .any(|n| n == &leaf_n || (n.len() >= 6 && leaf_n.contains(n.as_str())));
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
            user_data: false,
            user_library: false,
            size_kb: None,
            bucket: None,
        });
    }
}
