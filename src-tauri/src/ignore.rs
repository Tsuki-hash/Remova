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
    p.is_empty()
        || list
            .publishers
            .iter()
            .any(|x| x.eq_ignore_ascii_case(p))
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
        assert!(is_path_ignored(
            &l,
            r"C:\Program Files\Common Files\foo"
        ));
        assert!(!is_name_ignored(&l, "7-Zip"));
    }
}
