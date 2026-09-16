//! Orphan leftover directory scan (P2-2) — no matching installed app.

use crate::apps::InstalledApp;
use crate::scanner::{CleanupItem, Confidence, Evidence, ItemKind, RiskLevel, SCORE_SUSPECTED_MIN};

fn slug_tokens(name: &str) -> Vec<String> {
    crate::scanner::slugify(name)
}

/// Heuristic: directory looks like an app leftover if it has an exe or many files.
fn looks_like_app_dir(p: &std::path::Path) -> bool {
    let Ok(rd) = std::fs::read_dir(p) else {
        return false;
    };
    let mut files = 0;
    let mut has_exe = false;
    for e in rd.flatten().take(40) {
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n.ends_with(".exe") || n.ends_with(".msi") {
            has_exe = true;
        }
        if e.path().is_file() {
            files += 1;
        }
    }
    has_exe || files >= 3
}

fn match_installed(installed: &[InstalledApp], dir: &std::path::Path) -> bool {
    let d = dir.to_string_lossy().replace('/', "\\").to_lowercase();
    let leaf = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for app in installed {
        let loc = app.install_location.replace('/', "\\").to_lowercase();
        if !loc.is_empty() && (d.starts_with(&loc) || loc.starts_with(&d)) {
            return true;
        }
        if leaf.is_empty() {
            continue;
        }
        let tokens = slug_tokens(&app.name);
        if tokens
            .iter()
            .any(|t| t.len() >= 4 && leaf.contains(t.as_str()))
        {
            return true;
        }
    }
    false
}

pub fn scan_orphans(installed: &[InstalledApp]) -> Vec<CleanupItem> {
    let mut roots: Vec<std::path::PathBuf> = vec![];
    for env in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
        "PROGRAMDATA",
    ] {
        if let Ok(v) = std::env::var(env) {
            roots.push(std::path::PathBuf::from(v));
        }
    }
    let mut out = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            // skip obvious system / installer cache folders (FUNC-8)
            if matches!(
                name.to_lowercase().as_str(),
                "windows"
                    | "windowsapps"
                    | "common files"
                    | "microsoft"
                    | "reference assemblies"
                    | "package cache"
                    | "installshield installation information"
                    | "windows kits"
                    | "internet explorer"
                    | "windows defender"
                    | "windows security"
                    | "windows powershell"
                    | "dotnet"
                    | "msbuild"
                    | "nuget"
            ) {
                continue;
            }
            if match_installed(installed, &p) {
                continue;
            }
            if !looks_like_app_dir(&p) {
                continue;
            }
            let score = SCORE_SUSPECTED_MIN + 10;
            out.push(CleanupItem {
                path: p.to_string_lossy().to_string(),
                kind: ItemKind::Dir,
                score,
                confidence: Confidence::Suspected,
                risk: RiskLevel::Medium,
                reason: "Orphan app-like folder (no matching uninstall entry)".into(),
                evidence: vec![Evidence {
                    code: "orphan_dir".into(),
                    label: "Folder not matched to installed software".into(),
                    weight: score,
                    detail: name,
                }],
                shared: false,
                user_data: false,
            });
            if out.len() >= 80 {
                return out;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::InstalledApp;

    fn app(name: &str, loc: &str) -> InstalledApp {
        InstalledApp {
            name: name.into(),
            version: String::new(),
            publisher: String::new(),
            install_location: loc.into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKCU".into(),
            registry_key: "k".into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        }
    }

    #[test]
    fn match_installed_by_path() {
        let installed = [app("Foo", r"C:\Program Files\FooApp")];
        let p = std::path::Path::new(r"C:\Program Files\FooApp");
        assert!(match_installed(&installed, p));
        let p2 = std::path::Path::new(r"C:\Program Files\OtherThing");
        assert!(!match_installed(&installed, p2));
    }

    #[cfg(windows)]
    #[test]
    fn scan_orphans_runs() {
        let apps = crate::apps::scan_installed_apps();
        let items = scan_orphans(&apps);
        for it in &items {
            assert_eq!(it.kind, ItemKind::Dir);
            assert!(it.path.len() > 3);
        }
    }
}
