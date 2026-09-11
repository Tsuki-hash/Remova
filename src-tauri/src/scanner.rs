//! Read-only association scanner (Phase 1) — parity-oriented port of Python association.

use crate::safety::is_safe_to_delete_registry;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCORE_CONFIRMED: i32 = 90;
pub const SCORE_SUSPECTED_MIN: i32 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    File,
    Dir,
    Registry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Confirmed,
    Suspected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub code: String,
    pub label: String,
    pub weight: i32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupItem {
    pub path: String,
    pub kind: ItemKind,
    pub score: i32,
    pub confidence: Confidence,
    pub risk: RiskLevel,
    pub reason: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub app_name: String,
    pub items: Vec<CleanupItem>,
}

pub fn normalize_for_match(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

pub fn slugify(text: &str) -> Vec<String> {
    let t = text.trim();
    if t.is_empty() {
        return vec![];
    }
    let re_dropped = [
        "installer",
        "setup",
        "uninstall",
        "for windows",
        "win64",
        "win32",
        "x64",
        "x86",
        "64-bit",
        "32-bit",
    ];
    let mut cleaned = t.to_string();
    for d in re_dropped {
        let pat = format!(" {} ", d);
        cleaned = cleaned.to_lowercase().replace(&pat, " ");
    }
    let cleaned = cleaned.trim();
    let tokens: Vec<String> = cleaned
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if tokens.is_empty() {
        return vec![];
    }
    let joined = tokens.concat();
    let hyphen = tokens.join("-");
    let underscore = tokens.join("_");
    let spaced = tokens.join(" ");
    let short = if tokens.len() >= 2 {
        format!("{}{}", tokens[0], tokens[1])
    } else {
        joined.clone()
    };
    let mut slugs: Vec<String> = vec![];
    for s in [joined, hyphen, underscore, spaced.to_lowercase(), cleaned.to_lowercase(), short] {
        let s = s.trim().to_string();
        if s.len() >= 2 && !slugs.contains(&s) {
            slugs.push(s);
        }
    }
    slugs
}

pub fn extract_exe_stems(install: &Path, name_slugs: &[String]) -> Vec<String> {
    let mut stems: Vec<String> = vec![];
    if let Ok(rd) = install.read_dir() {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false) && p.is_file() {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    let lower = stem.to_lowercase();
                    if !matches!(
                        lower.as_str(),
                        "unins000" | "uninstall" | "unins001" | "setup"
                    ) {
                        stems.push(stem.to_string());
                    }
                }
            }
        }
    }
    let name_norms: Vec<String> = name_slugs.iter().map(|s| normalize_for_match(s)).collect();
    stems.sort_by_key(|s| {
        let sn = normalize_for_match(s);
        let rank = if name_norms.iter().any(|n| {
            (n.len() >= 3 && (sn.contains(n.as_str()) || n.contains(sn.as_str()))) || n == &sn
        }) {
            0
        } else if matches!(
            sn.as_str(),
            "update" | "helper" | "launcher" | "crashhandler" | "service" | "agent"
        ) {
            3
        } else {
            2
        };
        (rank, sn)
    });
    stems.dedup();
    stems
}

pub fn finalize_score(score: i32) -> (Confidence, RiskLevel) {
    let score = score.clamp(-200, 120);
    let conf = if score >= SCORE_CONFIRMED {
        Confidence::Confirmed
    } else {
        Confidence::Suspected
    };
    let risk = if score >= SCORE_CONFIRMED {
        RiskLevel::Low
    } else if score >= SCORE_SUSPECTED_MIN {
        RiskLevel::Medium
    } else {
        RiskLevel::High
    };
    (conf, risk)
}

fn push_item(
    items: &mut Vec<CleanupItem>,
    path: String,
    kind: ItemKind,
    reason: String,
    evidence: Vec<Evidence>,
) {
    let mut score: i32 = evidence.iter().map(|e| e.weight).sum();
    score = score.clamp(-200, 120);
    if score < SCORE_SUSPECTED_MIN {
        return;
    }
    let (confidence, risk) = finalize_score(score);
    items.push(CleanupItem {
        path,
        kind,
        score,
        confidence,
        risk,
        reason,
        evidence,
    });
}

fn matches_product_dir(name: &str, name_slugs: &[String], exe_stems: &[String]) -> Option<&'static str> {
    let norm = normalize_for_match(name);
    if norm.len() < 2 {
        return None;
    }
    for slug in name_slugs {
        let sn = normalize_for_match(slug);
        if sn.len() >= 2 && norm == sn {
            return Some("confirmed");
        }
    }
    for stem in exe_stems {
        let sn = normalize_for_match(stem);
        if sn.len() >= 3 && norm == sn {
            return Some("confirmed");
        }
    }
    for slug in name_slugs {
        let sn = normalize_for_match(slug);
        if sn.len() >= 4 && norm.len() >= 4 && (sn.contains(norm.as_str()) || norm.contains(sn.as_str())) {
            return Some("suspected");
        }
    }
    None
}

fn is_safe_fs(p: &Path) -> bool {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let protected = [
        r"c:\windows",
        r"c:\windows.old",
        r"c:\programdata\microsoft",
        r"c:\program files\windowsapps",
        r"c:\program files\common files\microsoft shared",
        r"c:\program files (x86)\common files\microsoft shared",
        r"c:\users\default",
    ];
    for pref in protected {
        if s == pref || s.starts_with(&format!("{pref}\\")) {
            return false;
        }
    }
    if s.len() == 2 && s.ends_with(':') {
        return false;
    }
    true
}

/// Analyze filesystem + registry associations for one installed app (read-only).
pub fn analyze_associations(
    name: &str,
    install_location: &str,
    publisher: &str,
    registry_key: &str,
) -> ScanResult {
    let name_slugs = slugify(name);
    let _pub_slugs = if publisher.trim().is_empty() {
        vec![]
    } else {
        slugify(publisher)
    };
    let install = if install_location.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(install_location))
    };
    let exe_stems = install
        .as_ref()
        .filter(|p| p.is_dir())
        .map(|p| extract_exe_stems(p, &name_slugs))
        .unwrap_or_default();

    let mut items = Vec::new();

    // 1. Install location
    if let Some(root) = install.as_ref() {
        if root.exists() && is_safe_fs(root) {
            let is_dir = root.is_dir();
            items.push(CleanupItem {
                path: root.to_string_lossy().to_string(),
                kind: if is_dir { ItemKind::Dir } else { ItemKind::File },
                score: SCORE_CONFIRMED,
                confidence: Confidence::Confirmed,
                risk: RiskLevel::Low,
                reason: "Install location".into(),
                evidence: vec![Evidence {
                    code: "install_location".into(),
                    label: "Matches official install path".into(),
                    weight: 90,
                    detail: root.to_string_lossy().to_string(),
                }],
            });
        }
    }

    // 2. Named roots
    let mut bases: Vec<PathBuf> = vec![];
    if let Some(v) = std::env::var_os("LOCALAPPDATA") {
        bases.push(PathBuf::from(v));
    }
    if let Some(v) = std::env::var_os("APPDATA") {
        bases.push(PathBuf::from(v));
    }
    if let Some(v) = std::env::var_os("PROGRAMDATA") {
        bases.push(PathBuf::from(v));
    }
    if let Some(v) = std::env::var_os("ProgramFiles") {
        bases.push(PathBuf::from(v));
    }
    if let Some(v) = std::env::var_os("ProgramFiles(x86)") {
        bases.push(PathBuf::from(v));
    }

    let mut seen = std::collections::HashSet::new();
    for base in bases {
        if !base.exists() || !is_safe_fs(&base) {
            continue;
        }
        let Ok(rd) = base.read_dir() else {
            continue;
        };
        for e in rd.flatten() {
            let child = e.path();
            if !child.is_dir() {
                continue;
            }
            let key = child.to_string_lossy().to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            let Some(file_name) = child.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(m) = matches_product_dir(file_name, &name_slugs, &exe_stems) else {
                continue;
            };
            if !is_safe_fs(&child) || !safety_path_ok(&child) {
                continue;
            }
            seen.insert(key);
            let conf = if m == "confirmed" {
                Confidence::Confirmed
            } else {
                Confidence::Suspected
            };
            let risk = if conf == Confidence::Confirmed {
                RiskLevel::Low
            } else {
                RiskLevel::Medium
            };
            let score = if conf == Confidence::Confirmed { 40 } else { 30 };
            let label = if conf == Confidence::Confirmed {
                "Product folder name matches exactly"
            } else {
                "Product folder name fuzzy match"
            };
            items.push(CleanupItem {
                path: child.to_string_lossy().to_string(),
                kind: ItemKind::Dir,
                score,
                confidence: conf,
                risk,
                reason: format!("Product dir under {}", base.display()),
                evidence: vec![Evidence {
                    code: if m == "confirmed" {
                        "product_dir_exact".into()
                    } else {
                        "fuzzy_name_only".into()
                    },
                    label: label.into(),
                    weight: score,
                    detail: file_name.to_string(),
                }],
            });
        }
    }

    // 3. Uninstall key
    if !registry_key.trim().is_empty() {
        if is_safe_to_delete_registry(registry_key).is_ok() {
            items.push(CleanupItem {
                path: registry_key.to_string(),
                kind: ItemKind::Registry,
                score: SCORE_CONFIRMED,
                confidence: Confidence::Confirmed,
                risk: RiskLevel::Low,
                reason: "Uninstall registry key".into(),
                evidence: vec![Evidence {
                    code: "uninstall_key".into(),
                    label: "This product's uninstall key".into(),
                    weight: 90,
                    detail: registry_key.to_string(),
                }],
            });
        }
    }

    // 4. App Paths — only when install location verifies
    if let Some(install) = install.as_ref() {
        let install_str = install.to_string_lossy().to_lowercase();
        if !install_str.is_empty() {
            for stem in exe_stems.iter().take(3) {
                for key in crate::regscan::find_app_paths(&format!("{stem}.exe")) {
                    let Some(target) = crate::regscan::read_string_default(&key) else {
                        continue;
                    };
                    let t = target.trim().trim_matches('"').to_lowercase();
                    if !t.starts_with(&install_str) {
                        continue;
                    }
                    if is_safe_to_delete_registry(&key).is_ok() {
                        items.push(CleanupItem {
                            path: key.clone(),
                            kind: ItemKind::Registry,
                            score: 50,
                            confidence: Confidence::Confirmed,
                            risk: RiskLevel::Low,
                            reason: format!("App Paths: {stem}.exe"),
                            evidence: vec![Evidence {
                                code: "app_paths".into(),
                                label: "App Paths points into install dir".into(),
                                weight: 50,
                                detail: target,
                            }],
                        });
                    }
                }
            }
        }
    }

    // 5. Run startup values
    let install_low = install
        .as_ref()
        .map(|p| p.to_string_lossy().to_lowercase().trim_end_matches('\\').to_string())
        .unwrap_or_default();
    for (alias, sub) in [
        ("HKLM64", r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"),
        ("HKCU", r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"),
    ] {
        let key = format!("{alias}\\{sub}");
        for (vname, vdata) in crate::regscan::list_values(&key) {
            let data_l = vdata.to_lowercase();
            let hit_install = !install_low.is_empty() && data_l.contains(&install_low);
            let hit_name = name_slugs
                .iter()
                .map(|s| normalize_for_match(s))
                .any(|n| n.len() >= 4 && normalize_for_match(&vdata).contains(&n));
            if !hit_install && !hit_name {
                continue;
            }
            let path = format!("{key}|{vname}");
            if is_safe_to_delete_registry(&path).is_err() {
                continue;
            }
            let score = if hit_install { 55 } else { 40 };
            push_item(
                &mut items,
                path,
                ItemKind::Registry,
                format!("Run startup: {vname}"),
                vec![Evidence {
                    code: "run_startup".into(),
                    label: if hit_install {
                        "Startup points at install dir".into()
                    } else {
                        "Startup name matches product".into()
                    },
                    weight: score,
                    detail: vdata.chars().take(120).collect(),
                }],
            );
        }
    }

    // 6. Windows services (suspected / high — display only)
    scan_services(&name_slugs, &exe_stems, &install_low, &mut items);

    // 7. Scheduled tasks (suspected / high — display only)
    scan_scheduled_tasks(&name_slugs, &install_low, &mut items);

    ScanResult {
        app_name: name.to_string(),
        items,
    }
}

fn scan_services(
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
        });
    }
}

fn scan_scheduled_tasks(
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
        let hit_install =
            !install_low.is_empty() && key_path.to_lowercase().contains(install_low);
        let strong =
            hit_install || name_norms.iter().any(|n| n == &leaf_n || (n.len() >= 6 && leaf_n.contains(n.as_str())));
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
        });
    }
}

fn safety_path_ok(_p: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_tokens() {
        let s = slugify("DemoApp Installer");
        assert!(s.iter().any(|x| x.to_lowercase().contains("demoapp")));
    }

    #[test]
    fn score_finalize() {
        assert_eq!(finalize_score(90).0, Confidence::Confirmed);
        assert_eq!(finalize_score(40).0, Confidence::Suspected);
        assert_eq!(finalize_score(5).1, RiskLevel::High);
    }

    #[test]
    fn product_dir_match() {
        let slugs = slugify("DemoApp");
        assert_eq!(matches_product_dir("DemoApp", &slugs, &[]), Some("confirmed"));
        assert!(matches_product_dir("TotallyOther", &slugs, &[]).is_none());
    }

    #[test]
    fn analyze_empty_location_no_panic() {
        let r = analyze_associations("DemoApp", "", "DemoVendor", r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{X}");
        assert!(r.items.iter().any(|i| i.path.contains("Uninstall")));
    }
}
