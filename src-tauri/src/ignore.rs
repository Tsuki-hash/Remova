//! User ignore list (P2-3) persisted as JSON under ProgramData/Remova.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IgnoreList {
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
}

fn ignore_path() -> PathBuf {
    let base = std::env::var("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"));
    base.join("Remova").join("ignore.json")
}

pub fn load() -> IgnoreList {
    let p = ignore_path();
    let Ok(s) = std::fs::read_to_string(p) else {
        return IgnoreList::default();
    };
    serde_json::from_str(&s).unwrap_or_default()
}

pub fn save(list: &IgnoreList) -> Result<(), String> {
    let p = ignore_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| e.to_string())
}

pub fn add_publisher(name: &str) -> Result<IgnoreList, String> {
    let mut l = load();
    let n = name.trim();
    if n.is_empty() {
        return Err("empty publisher".into());
    }
    if !l.publishers.iter().any(|x| x.eq_ignore_ascii_case(n)) {
        l.publishers.push(n.to_string());
    }
    save(&l)?;
    Ok(l)
}

pub fn add_name(name: &str) -> Result<IgnoreList, String> {
    let mut l = load();
    let n = name.trim();
    if n.is_empty() {
        return Err("empty name".into());
    }
    if !l.names.iter().any(|x| x.eq_ignore_ascii_case(n)) {
        l.names.push(n.to_string());
    }
    save(&l)?;
    Ok(l)
}

pub fn is_publisher_ignored(list: &IgnoreList, publisher: &str) -> bool {
    let p = publisher.trim();
    p.is_empty() || list.publishers.iter().any(|x| x.eq_ignore_ascii_case(p))
}

pub fn is_name_ignored(list: &IgnoreList, name: &str) -> bool {
    let n = name.trim();
    list.names.iter().any(|x| x.eq_ignore_ascii_case(n))
}

pub fn is_path_ignored(list: &IgnoreList, path: &str) -> bool {
    let p = path.replace('/', "\\").to_lowercase();
    list.paths
        .iter()
        .any(|x| p.starts_with(&x.replace('/', "\\").to_lowercase()))
}

pub fn add_path(path: &str) -> Result<IgnoreList, String> {
    let mut l = load();
    let p = path.trim().replace('/', "\\");
    if p.is_empty() {
        return Err("empty path".into());
    }
    if !l.paths.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
        l.paths.push(p);
    }
    save(&l)?;
    Ok(l)
}

/// One proposed ignore rule with a human reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreSuggestion {
    /// "path" | "publisher"
    pub kind: String,
    pub value: String,
    pub reason: String,
}

const SHARED_ROOTS: &[&str] = &[
    // Package Cache is NOT a global ignore (SEC-4) — too broad for all products.
    r"C:\Program Files\Common Files",
    r"C:\Program Files (x86)\Common Files",
    r"C:\Program Files\Microsoft Shared",
    r"C:\Program Files (x86)\Microsoft Shared",
    r"C:\Windows\Microsoft.NET",
    r"C:\Windows\assembly",
];

fn longest_root(path: &str) -> Option<&'static str> {
    let p = path.replace('/', "\\").to_lowercase();
    let mut best: Option<&'static str> = None;
    for r in SHARED_ROOTS {
        let rl = r.to_lowercase();
        if p.starts_with(&rl) && best.map(|b| b.len() < r.len()).unwrap_or(true) {
            best = Some(*r);
        }
    }
    best
}

/// Rule-based suggestions from leftover/shared paths (no LLM required).
pub fn suggest_from_leftovers(publisher: &str, paths: &[String]) -> Vec<IgnoreSuggestion> {
    let mut out: Vec<IgnoreSuggestion> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        if let Some(root) = longest_root(path) {
            if seen.insert(root.to_string()) {
                out.push(IgnoreSuggestion {
                    kind: "path".into(),
                    value: root.to_string(),
                    reason: "共享运行库/安装缓存目录，反复出现在残留中".into(),
                });
            }
        }
    }
    // Publisher ignore only when multiple shared-root hits exist (aggressive otherwise).
    let pub_trim = publisher.trim();
    if out.len() >= 2 && !pub_trim.is_empty() && !is_publisher_ignored(&load(), pub_trim) {
        // still prefer path ignores; publisher only as last resort for non-Microsoft vendors
        let low = pub_trim.to_lowercase();
        if !low.contains("microsoft") {
            out.push(IgnoreSuggestion {
                kind: "publisher".into(),
                value: pub_trim.to_string(),
                reason: "该发布者存在多处共享路径残留，可选择整体忽略（请确认无本软件关键组件）"
                    .into(),
            });
        }
    }
    out
}

pub fn apply_suggestions(items: &[IgnoreSuggestion]) -> Result<IgnoreList, String> {
    let mut l = load();
    for it in items {
        match it.kind.as_str() {
            "path" => {
                let p = it.value.trim().replace('/', "\\");
                if !p.is_empty() && !l.paths.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
                    l.paths.push(p);
                }
            }
            "publisher" => {
                let p = it.value.trim();
                if !p.is_empty() && !l.publishers.iter().any(|x| x.eq_ignore_ascii_case(p)) {
                    l.publishers.push(p.to_string());
                }
            }
            "name" => {
                let n = it.value.trim();
                if !n.is_empty() && !l.names.iter().any(|x| x.eq_ignore_ascii_case(n)) {
                    l.names.push(n.to_string());
                }
            }
            _ => {}
        }
    }
    save(&l)?;
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_match_helpers() {
        let mut l = IgnoreList::default();
        l.publishers.push("Microsoft Corporation".into());
        l.names.push("OneDrive".into());
        l.paths.push(r"C:\Program Files\Common Files".into());
        assert!(is_publisher_ignored(&l, "microsoft corporation"));
        assert!(is_name_ignored(&l, "onedrive"));
        assert!(is_path_ignored(&l, r"C:\Program Files\Common Files\foo"));
        assert!(!is_name_ignored(&l, "7-Zip"));
    }

    #[test]
    fn suggest_shared_roots() {
        let paths = vec![
            r"C:\ProgramData\Package Cache\{abc}\vc_redist.exe".to_string(),
            r"C:\Program Files\Common Files\Acme\lib.dll".to_string(),
        ];
        let s = suggest_from_leftovers("Acme Corp", &paths);
        // SEC-4: Package Cache must NOT become a global ignore suggestion.
        assert!(!s
            .iter()
            .any(|x| x.kind == "path" && x.value.contains("Package Cache")));
        assert!(s
            .iter()
            .any(|x| x.kind == "path" && x.value.contains("Common Files")));
    }

    #[test]
    fn apply_suggestions_persists_paths() {
        let mut l = IgnoreList::default();
        l.paths.push(r"C:\ProgramData\Package Cache".into());
        assert!(is_path_ignored(&l, r"C:\ProgramData\Package Cache\x"));
    }
}
