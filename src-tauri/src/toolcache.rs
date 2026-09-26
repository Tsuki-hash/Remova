//! Specialty tool-cache scan (dev / game / browser). Fixed allow-list roots only.
//! Never touches game saves, browser profiles, or package store indexes that break tooling.

use crate::constants::SPECIALTY_RESULT_CAP;
use crate::scan_allow::{self, AllowScope};
use crate::scanner::{
    fill_item_buckets, fill_item_sizes, CleanupItem, Confidence, Evidence, ItemKind, RiskLevel,
    SCORE_CONFIRMED,
};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDomain {
    Dev,
    Game,
    Browser,
}

impl ToolDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolDomain::Dev => "dev",
            ToolDomain::Game => "game",
            ToolDomain::Browser => "browser",
        }
    }
}

fn local(low: &str) -> Option<PathBuf> {
    let v = std::env::var_os(low)?;
    Some(PathBuf::from(v))
}

fn userprofile() -> Option<PathBuf> {
    local("USERPROFILE")
}

/// Fixed relative roots per domain (joined with env bases). Missing paths are skipped.
fn tool_roots() -> Vec<(ToolDomain, PathBuf, &'static str)> {
    let mut out = Vec::new();
    if let Some(up) = userprofile() {
        // REV-SUP-13: nested `join()` — `/` segments are not portable separators.
        let appdata = up.join("AppData");
        let local = appdata.join("Local");
        out.push((ToolDomain::Dev, local.join("npm-cache"), "npm-cache"));
        out.push((
            ToolDomain::Dev,
            local.join("pip").join("cache"),
            "pip-cache",
        ));
        out.push((
            ToolDomain::Dev,
            local.join("NuGet").join("v3-cache"),
            "nuget-cache",
        ));
        out.push((
            ToolDomain::Dev,
            local.join("Yarn").join("Cache"),
            "yarn-cache",
        ));
        out.push((
            ToolDomain::Dev,
            local.join("pnpm").join("cache"),
            "pnpm-cache",
        ));
        out.push((
            ToolDomain::Dev,
            up.join(".cargo").join("registry").join("cache"),
            "cargo-cache",
        ));
        out.push((
            ToolDomain::Dev,
            local.join("gradle").join("caches"),
            "gradle-cache",
        ));
        out.push((
            ToolDomain::Browser,
            local
                .join("Google")
                .join("Chrome")
                .join("User Data")
                .join("Default")
                .join("Cache"),
            "chrome-cache",
        ));
        out.push((
            ToolDomain::Browser,
            local
                .join("Microsoft")
                .join("Edge")
                .join("User Data")
                .join("Default")
                .join("Cache"),
            "edge-cache",
        ));
        out.push((
            ToolDomain::Browser,
            local.join("Mozilla").join("Firefox").join("Profiles"),
            "firefox-profiles-skip",
        ));
    }
    if let Some(pf) =
        std::env::var_os("ProgramFiles(x86)").or_else(|| std::env::var_os("ProgramFiles"))
    {
        out.push((
            ToolDomain::Game,
            PathBuf::from(&pf)
                .join("Steam")
                .join("steamapps")
                .join("shadercache"),
            "steam-shadercache",
        ));
    }
    // Firefox profiles are user data — listed for completeness but never auto-picked.
    out.retain(|(_, p, _)| p.exists());
    out
}

fn push_item(
    out: &mut Vec<CleanupItem>,
    scanned: &mut HashSet<String>,
    path: PathBuf,
    domain: ToolDomain,
    label: &str,
) {
    if out.len() >= SPECIALTY_RESULT_CAP {
        return;
    }
    // Non-UTF-8 never becomes a delete candidate (fail-closed).
    let Some(p) = crate::fsutil::path_utf8(&path) else {
        return;
    };
    // Profiles / saves are never default targets.
    let high_touch = label.contains("profile") || label.contains("save");
    let risk = if high_touch {
        RiskLevel::High
    } else {
        RiskLevel::Low
    };
    let score = if high_touch { 0 } else { SCORE_CONFIRMED };
    out.push(CleanupItem {
        path: p.clone(),
        kind: ItemKind::Dir,
        score,
        confidence: if high_touch {
            Confidence::Suspected
        } else {
            Confidence::Confirmed
        },
        risk,
        reason: format!("tool cache ({})", domain.as_str()),
        evidence: vec![Evidence {
            code: "tool_cache".into(),
            label: domain.as_str().into(),
            weight: if high_touch { 0 } else { 50 },
            detail: label.into(),
        }],
        shared: false,
        user_data: crate::safety::is_user_data_path(&p),
        user_library: crate::safety::is_user_library_path(&p)
            && !crate::safety::is_user_data_path(&p),
        size_kb: None,
        bucket: Some(domain.as_str().into()),
    });
    scanned.insert(p);
}

/// Scan specialty tool caches. Results feed the toolcache allow-list.
pub fn scan_tool_caches() -> Vec<CleanupItem> {
    let mut out = Vec::new();
    let mut scanned = HashSet::new();
    for (domain, path, label) in tool_roots() {
        if label.contains("skip") || label.contains("profile") {
            // Surface as keep-only high-risk if we ever list it; currently skip entirely.
            continue;
        }
        push_item(&mut out, &mut scanned, path, domain, label);
        if out.len() >= SPECIALTY_RESULT_CAP {
            break;
        }
    }
    fill_item_sizes(&mut out);
    fill_item_buckets(&mut out, "");
    scan_allow::remember(AllowScope::ToolCache, &scanned);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_label() {
        assert_eq!(ToolDomain::Dev.as_str(), "dev");
        assert_eq!(ToolDomain::Game.as_str(), "game");
        assert_eq!(ToolDomain::Browser.as_str(), "browser");
    }

    #[test]
    fn scan_runs_under_cap() {
        let items = scan_tool_caches();
        assert!(items.len() <= SPECIALTY_RESULT_CAP);
        for it in &items {
            assert!(!it.path.is_empty());
            // Never default-allow user_data red lines as cleanable freebies.
            if it.user_data {
                assert_eq!(it.score, 0);
            }
        }
    }
}
