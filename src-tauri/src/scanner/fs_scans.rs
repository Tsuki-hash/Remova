//! Filesystem association scans (shortcuts, TEMP, WebView, cross-drive).

use super::*;

pub(super) fn scan_other_drive_roots(name_slugs: &[String], items: &mut Vec<CleanupItem>) {
    for letter in b'C'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        let root = PathBuf::from(&drive);
        if !root.exists() {
            continue;
        }
        let Ok(rd) = root.read_dir() else {
            continue;
        };
        for e in rd.flatten().take(80) {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !is_safe_fs(&p) {
                continue;
            }
            let Some(m) = matches_product_dir(fname, name_slugs, &[]) else {
                continue;
            };
            items.push(CleanupItem {
                path: p.to_string_lossy().to_string(),
                kind: ItemKind::Dir,
                score: if m == "confirmed" { 50 } else { 35 },
                confidence: if m == "confirmed" {
                    Confidence::Confirmed
                } else {
                    Confidence::Suspected
                },
                risk: if m == "confirmed" {
                    RiskLevel::Medium
                } else {
                    RiskLevel::High
                },
                reason: format!("Other-drive product folder: {drive}{fname}"),
                evidence: vec![Evidence {
                    code: "cross_drive".into(),
                    label: "Folder on non-default drive matches product".into(),
                    weight: 40,
                    detail: p.to_string_lossy().chars().take(120).collect(),
                }],
                shared: false,
                user_data: false,
                size_kb: None,
                bucket: None,
            });
        }
    }
}

/// Whether a LOCALAPPDATA/APPDATA child dir name looks like a product WebView2/Electron
/// mask. Name match is REQUIRED 鈥?a folder that merely contains EBWebView is not evidence (SEC-2).
pub(crate) fn webview_mask_matches(dir_name: &str, name_slugs: &[String]) -> bool {
    let low = dir_name.to_lowercase();
    if !low.ends_with(".exe") && !low.contains("ebwebview") {
        return false;
    }
    let stem_n = normalize_for_match(low.trim_end_matches(".exe"));
    if stem_n.len() < 4 {
        return false;
    }
    name_slugs.iter().any(|s| {
        let n = normalize_for_match(s);
        n.len() >= 4 && stem_n.contains(n.as_str())
    })
}

/// WebView2 cache dirs named like `app.exe` (SOP: ollama app.exe\EBWebView).
pub(super) fn scan_webview_masks(name_slugs: &[String], items: &mut Vec<CleanupItem>) {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let roaming = std::env::var_os("APPDATA");
    let mut roots = vec![PathBuf::from(&local)];
    if let Some(r) = roaming {
        roots.push(PathBuf::from(r));
    }
    for base in roots {
        let Ok(rd) = base.read_dir() else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            // Name match is REQUIRED 鈥?WebView2 folder alone is not evidence (SEC-2).
            if !webview_mask_matches(fname, name_slugs) {
                continue;
            }
            let has_web = p.join("EBWebView").exists() || p.join("WebView2").exists();
            if !is_safe_fs(&p) {
                continue;
            }
            items.push(CleanupItem {
                path: p.to_string_lossy().to_string(),
                kind: ItemKind::Dir,
                score: 40,
                // AppData / WebView caches are name-match only — require user confirm (BE-03).
                confidence: Confidence::Suspected,
                risk: RiskLevel::Medium,
                reason: if has_web {
                    "WebView2 / Electron cache folder".into()
                } else {
                    "AppData folder matching product name".into()
                },
                evidence: vec![Evidence {
                    code: "webview_mask".into(),
                    label: "Looks like app cache under AppData".into(),
                    weight: 40,
                    detail: fname.to_string(),
                }],
                shared: false,
                user_data: false,
                size_kb: None,
                bucket: None,
            });
        }
    }
}

pub(super) fn scan_shortcuts(
    name_slugs: &[String],
    exe_stems: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    let mut roots: Vec<PathBuf> = vec![];
    if let Some(up) = std::env::var_os("USERPROFILE") {
        let up = PathBuf::from(up);
        roots.push(up.join("Desktop"));
        roots.push(up.join("AppData/Roaming/Microsoft/Windows/Start Menu"));
    }
    if let Some(pu) = std::env::var_os("PUBLIC") {
        roots.push(PathBuf::from(pu).join("Desktop"));
    }
    // resolve from the environment so a non-C system drive is still scanned;
    // the literal path is only the last-resort fallback.
    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    roots.push(program_data.join(r"Microsoft\Windows\Start Menu"));

    for root in roots {
        if !root.exists() {
            continue;
        }
        walk_shortcuts(&root, 0, name_slugs, exe_stems, install_low, items);
    }
}

fn walk_shortcuts(
    dir: &Path,
    depth: u32,
    name_slugs: &[String],
    exe_stems: &[String],
    install_low: &str,
    items: &mut Vec<CleanupItem>,
) {
    if depth > 3 {
        return;
    }
    let Ok(rd) = dir.read_dir() else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_shortcuts(&p, depth + 1, name_slugs, exe_stems, install_low, items);
            continue;
        }
        let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        let ext = ext.to_lowercase();
        if !matches!(ext.as_str(), "lnk" | "url" | "appref-ms") {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let sn = normalize_for_match(stem);
        let hit = name_slugs.iter().any(|s| normalize_for_match(s) == sn)
            || exe_stems
                .iter()
                .any(|s| normalize_for_match(s) == sn && sn.len() >= 3)
            || {
                // binary peek for install path
                if let Ok(data) = std::fs::read(&p) {
                    !install_low.is_empty()
                        && (data
                            .windows(install_low.len())
                            .any(|w| String::from_utf8_lossy(w).to_lowercase() == *install_low)
                            || {
                                let u16s: Vec<u16> = install_low.encode_utf16().collect();
                                let bytes: Vec<u8> =
                                    u16s.iter().flat_map(|u| u.to_le_bytes()).collect();
                                data.windows(bytes.len()).any(|w| w == bytes)
                            })
                } else {
                    false
                }
            };
        if !hit || !is_safe_fs(&p) {
            continue;
        }
        items.push(CleanupItem {
            path: p.to_string_lossy().to_string(),
            kind: ItemKind::File,
            score: 50,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "Shortcut".into(),
            evidence: vec![Evidence {
                code: "shortcut_target".into(),
                label: "Shortcut matches product".into(),
                weight: 50,
                detail: stem.to_string(),
            }],
            shared: false,
            user_data: false,
            size_kb: None,
            bucket: None,
        });
    }
}

pub(super) fn scan_temp(name_slugs: &[String], items: &mut Vec<CleanupItem>) {
    let Ok(temp) = std::env::var("TEMP") else {
        return;
    };
    let temp = PathBuf::from(temp);
    if !temp.exists() {
        return;
    }
    let slugs: Vec<String> = name_slugs
        .iter()
        .map(|s| normalize_for_match(s))
        .filter(|s| s.len() >= 4)
        .collect();
    if slugs.is_empty() {
        return;
    }
    let Ok(rd) = temp.read_dir() else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.to_string_lossy().to_lowercase();
        if !slugs.iter().any(|s| name.contains(s.as_str())) {
            continue;
        }
        if !is_safe_fs(&p) {
            continue;
        }
        items.push(CleanupItem {
            path: p.to_string_lossy().to_string(),
            kind: if p.is_dir() {
                ItemKind::Dir
            } else {
                ItemKind::File
            },
            score: 30,
            confidence: Confidence::Suspected,
            risk: RiskLevel::Medium,
            reason: "TEMP name match".into(),
            evidence: vec![Evidence {
                code: "recent_temp_match".into(),
                label: "TEMP folder/file name match".into(),
                weight: 30,
                detail: p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            }],
            shared: false,
            user_data: false,
            size_kb: None,
            bucket: None,
        });
    }
}
