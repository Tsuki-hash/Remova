//! Explicit, bounded, read-only deep registry check (M5-4, SOP §9).
//!
//! The normal analyze pass only looks at exact `SOFTWARE\<slug>` keys. This
//! check lets the user ask "did the vendor bury the product one level deeper?"
//! without ever offering a delete: bounded by a key budget and a wall-clock
//! deadline, matched only on name-boundary keywords from the app name.

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryCheckHit {
    pub key: String,
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryCheckResult {
    pub hits: Vec<RegistryCheckHit>,
    pub scanned: u32,
    pub truncated: bool,
    pub timed_out: bool,
}

const ROOTS: [&str; 3] = [r"HKLM64\SOFTWARE", r"HKLM32\SOFTWARE", r"HKCU\SOFTWARE"];
const SKIP_VENDORS: &[&str] = &[
    "CLASSES",
    "POLICIES",
    "WOW6432NODE",
    "REGISTEREDAPPLICATIONS",
    "CLIENTS",
    "WINDOWS NT",
    "SYSTEM",
];
const MAX_KEYS: u32 = 4_000;
const MAX_CHILDREN_PER_VENDOR: usize = 64;
const BUDGET: Duration = Duration::from_millis(2_000);

pub fn deep_registry_check(app_name: &str) -> Result<RegistryCheckResult, String> {
    if app_name.trim().is_empty() || app_name.chars().count() > 200 {
        return Err("deep_registry_invalid_name".into());
    }
    crate::fsutil::validate_native_name(app_name)?;
    let keywords = keywords(app_name);
    if keywords.is_empty() {
        return Err("deep_registry_too_generic".into());
    }
    let start = std::time::Instant::now();
    Ok(walk_with(
        crate::regscan::list_subkeys,
        || start.elapsed(),
        &keywords,
    ))
}

#[derive(Debug, Default)]
struct Keywords {
    /// Whole-product variants; used for a vendor-level key.
    strong: Vec<String>,
    /// Individual meaningful words; used only for a child key such as
    /// `Microsoft\Office`, never to report a vendor root.
    weak: Vec<String>,
}

impl Keywords {
    fn is_empty(&self) -> bool {
        self.strong.is_empty() && self.weak.is_empty()
    }
}

fn keywords(app_name: &str) -> Keywords {
    const STOPWORDS: &[&str] = &[
        "update",
        "updater",
        "client",
        "server",
        "tools",
        "tool",
        "suite",
        "studio",
        "driver",
        "drivers",
        "runtime",
        "library",
        "service",
        "desktop",
        "cloud",
        "player",
        "editor",
        "manager",
        "console",
        "launcher",
        "windows",
        "software",
        "application",
        "app",
        "setup",
        "installer",
        "uninstall",
    ];
    let mut out = Keywords::default();
    for slug in crate::scanner::slugify(app_name) {
        let slug = slug.trim().to_lowercase();
        if slug.chars().count() < 4 || out.strong.contains(&slug) {
            continue;
        }
        out.strong.push(slug);
    }
    out.strong.truncate(8);
    for token in app_name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        let token = token.to_lowercase();
        if token.chars().count() < 4
            || STOPWORDS.contains(&token.as_str())
            || out.strong.contains(&token)
            || out.weak.contains(&token)
        {
            continue;
        }
        out.weak.push(token);
    }
    out
}

fn walk_with(
    mut list: impl FnMut(&str) -> Vec<String>,
    mut elapsed: impl FnMut() -> Duration,
    keywords: &Keywords,
) -> RegistryCheckResult {
    let mut result = RegistryCheckResult {
        hits: Vec::new(),
        scanned: 0,
        truncated: false,
        timed_out: false,
    };
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    'roots: for root in ROOTS {
        for vendor in list(root) {
            if elapsed() >= BUDGET {
                result.timed_out = true;
                break 'roots;
            }
            if result.scanned >= MAX_KEYS {
                result.truncated = true;
                break 'roots;
            }
            result.scanned += 1;
            let vendor_path = format!("{root}\\{vendor}");
            if SKIP_VENDORS
                .iter()
                .any(|skip| skip.eq_ignore_ascii_case(&vendor))
            {
                continue;
            }
            let vendor_hit = leaf_matches(&vendor, &keywords.strong);
            let hits_before = result.hits.len();
            let children = list(&vendor_path);
            if children.len() > MAX_CHILDREN_PER_VENDOR {
                result.truncated = true;
            }
            for child in children.into_iter().take(MAX_CHILDREN_PER_VENDOR) {
                if elapsed() >= BUDGET {
                    result.timed_out = true;
                    break 'roots;
                }
                if result.scanned >= MAX_KEYS {
                    result.truncated = true;
                    break 'roots;
                }
                result.scanned += 1;
                let strong_hit = leaf_matches(&child, &keywords.strong);
                let weak_hit = !strong_hit && leaf_matches(&child, &keywords.weak);
                if strong_hit || weak_hit {
                    let child_path = format!("{vendor_path}\\{child}");
                    push_hit(
                        &mut result,
                        &mut seen,
                        &child_path,
                        &child,
                        if weak_hit { "nested_weak" } else { "nested" },
                    );
                }
            }
            // A vendor key that matches only because the product name starts
            // with the vendor name (`Acme` → `Acme Tool`) must not hide the
            // concrete child key; report the vendor itself only when no child
            // matched.
            if vendor_hit && result.hits.len() == hits_before {
                push_hit(&mut result, &mut seen, &vendor_path, &vendor, "top_level");
            }
        }
    }
    result
}

fn leaf_matches(leaf: &str, keywords: &[String]) -> bool {
    keywords
        .iter()
        .any(|keyword| crate::scanner::name_boundary_hit(leaf, keyword))
}

fn push_hit(
    result: &mut RegistryCheckResult,
    seen: &mut std::collections::HashSet<String>,
    path: &str,
    name: &str,
    detail: &str,
) {
    let dedupe = path.to_lowercase();
    if !seen.insert(dedupe) {
        return;
    }
    result.hits.push(RegistryCheckHit {
        key: path.to_string(),
        name: name.to_string(),
        detail: detail.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(vendor: &str, children: &[&str]) -> Vec<String> {
        let mut out = vec![vendor.to_string()];
        out.extend(children.iter().map(|s| s.to_string()));
        out
    }

    #[test]
    fn reports_only_name_boundary_hits_and_never_the_shared_root() {
        let fake = |key: &str| match key {
            k if k == ROOTS[0] => tree("Acme", &["AcmeTool", "OtherTool"]),
            k if k == format!("{}\\Acme", ROOTS[0]) => {
                vec!["AcmeTool".into(), "OtherTool".into()]
            }
            _ => vec![],
        };
        let result = walk_with(fake, || Duration::ZERO, &keywords("Acme Tool"));
        let keys: Vec<&str> = result.hits.iter().map(|h| h.key.as_str()).collect();
        assert!(keys.contains(&r"HKLM64\SOFTWARE\Acme\AcmeTool"));
        assert!(!keys.iter().any(|k| k.contains("OtherTool")));
        assert!(!keys
            .iter()
            .any(|k| k.eq_ignore_ascii_case(r"HKLM64\SOFTWARE\Acme")));
        assert!(!result.truncated && !result.timed_out);
    }

    #[test]
    fn vendor_key_itself_is_reported_when_no_child_matches() {
        let fake = |key: &str| {
            if key == ROOTS[0] {
                vec!["AcmeTool".into()]
            } else {
                vec![]
            }
        };
        let result = walk_with(fake, || Duration::ZERO, &keywords("AcmeTool"));
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].key, r"HKLM64\SOFTWARE\AcmeTool");
    }

    #[test]
    fn deadline_and_key_budget_stop_the_walk() {
        let many = |key: &str| {
            if key == ROOTS[0] {
                (0..MAX_KEYS + 10).map(|n| format!("Vendor{n}")).collect()
            } else {
                vec![]
            }
        };
        let capped = walk_with(many, || Duration::ZERO, &keywords("AcmeTool"));
        assert!(capped.truncated);
        assert_eq!(capped.scanned, MAX_KEYS);

        let slow = walk_with(
            |key| {
                if key == ROOTS[0] {
                    vec!["Vendor1".into(), "Vendor2".into()]
                } else {
                    vec![]
                }
            },
            {
                let mut ticks = 0u32;
                move || {
                    ticks += 1;
                    Duration::from_millis(u64::from(ticks) * 1_100)
                }
            },
            &keywords("AcmeTool"),
        );
        assert!(slow.timed_out);
        assert!(slow.scanned < 2);
    }

    #[test]
    fn microsoft_vendor_children_are_searchable_without_flagging_the_vendor() {
        let fake = |key: &str| {
            if key == ROOTS[0] {
                vec!["Microsoft".into()]
            } else if key == r"HKLM64\SOFTWARE\Microsoft" {
                vec!["Office".into(), "Windows".into()]
            } else {
                vec![]
            }
        };
        let result = walk_with(fake, || Duration::ZERO, &keywords("Microsoft Office"));
        let keys: Vec<&str> = result.hits.iter().map(|h| h.key.as_str()).collect();
        assert!(keys.contains(&r"HKLM64\SOFTWARE\Microsoft\Office"));
        assert!(!keys
            .iter()
            .any(|k| k.eq_ignore_ascii_case(r"HKLM64\SOFTWARE\Microsoft")));
        assert_eq!(
            result
                .hits
                .iter()
                .find(|h| h.key.ends_with(r"\Microsoft\Office"))
                .map(|h| h.detail.as_str()),
            Some("nested_weak")
        );
    }

    #[test]
    fn generic_names_are_refused_before_any_registry_read() {
        assert!(deep_registry_check("QQ").is_err());
        assert!(keywords("QQ").is_empty());
        assert!(!keywords("Acme Tool").is_empty());
    }
}
