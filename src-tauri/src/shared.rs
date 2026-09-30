//! Shared runtime / redistributable heuristics (local rules, no AI required).

/// Known shared frameworks and redistributables users should not casually delete.
const SHARED_NAME_TOKENS: &[&str] = &[
    "visual c++",
    "visual cplusplus",
    "vcredist",
    "vc_redist",
    "vcruntime",
    "microsoft visual c++",
    "microsoft .net",
    ".net runtime",
    ".net desktop",
    ".net core",
    "asp.net",
    "directx",
    "opengl",
    "openal",
    "vulkan",
    "python",
    "node.js",
    "nodejs",
    "java runtime",
    "jre ",
    "jdk ",
    "adobe air",
    "adobe flash",
    "silverlight",
    "unity",
    "unreal engine",
    "microsoft edge webview",
    "webview2",
    "microsoft games",
    "xna framework",
    "physx",
    "nvapi",
    "microsoft power query",
    "microsoft access database",
    "microsoft sql server compact",
    "microsoft redistributable",
];

/// Path markers that always mean shared/system — never treat as vendor-owned leftovers.
const HARD_PATH_MARKERS: &[&str] = &[
    r"\windows\system32\",
    r"\windows\syswow64\",
    r"\microsoft shared\",
    r"\windows\assembly\",
    r"\windows\microsoft.net\",
    r"\dotnet\",
    r"\vcredist\",
];

fn lower(s: &str) -> String {
    s.to_lowercase()
}

fn norm_path(path: &str) -> String {
    lower(&path.replace('/', "\\"))
}

/// First valid GUID (`{36 hex/dash chars}`) across ALL brace groups,
/// ASCII-folded to lowercase. Only ASCII is folded, so slicing the folded
/// string is byte-safe (no Unicode indices touch the original `String`).
/// `8-4-4-4-12` hex with hyphens at the canonical positions only.
fn is_guid_shape(body: &str) -> bool {
    if body.len() != 36 {
        return false;
    }
    for (i, b) in body.bytes().enumerate() {
        match (i, b) {
            (8 | 13 | 18 | 23, b'-') => {}
            (_, b) if b.is_ascii_hexdigit() => {}
            _ => return false,
        }
    }
    true
}

pub fn first_guid(s: &str) -> Option<String> {
    let low = s.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(rel) = low[from..].find('{') {
        let start = from + rel;
        let Some(end_rel) = low[start..].find('}') else {
            break;
        };
        let end = start + end_rel;
        let body = &low[start + 1..end];
        if is_guid_shape(body) {
            return Some(low[start..=end].to_string());
        }
        from = start + 1;
    }
    None
}

/// True when path is an exact Common Files root (last segment).
pub fn is_common_files_root(path: &str) -> bool {
    let trimmed = norm_path(path).trim_end_matches('\\').to_string();
    trimmed.ends_with(r"\common files")
}

/// True when path is under Microsoft Shared (hard red line subtree).
pub fn is_microsoft_shared_path(path: &str) -> bool {
    let p = norm_path(path);
    p.contains(r"\microsoft shared")
}

/// Vendor-owned-looking Common Files subpath (not root, not Microsoft Shared).
pub fn is_common_files_vendor_path(path: &str) -> bool {
    let p = norm_path(path);
    if !p.contains(r"\common files\") {
        return false;
    }
    if is_common_files_root(path) || is_microsoft_shared_path(path) {
        return false;
    }
    true
}

/// Hard shared: exact CF roots, Microsoft Shared, name tokens, non-CF hard path markers.
/// Matching is **name + path only** — free-text `reason` must not force shared.
pub fn is_hard_shared_item(name: &str, path: &str, reason: &str) -> bool {
    let _ = reason;
    let blob = format!("{}\n{}", lower(name), lower(path));
    if SHARED_NAME_TOKENS.iter().any(|t| blob.contains(t)) {
        return true;
    }
    if is_common_files_root(path) || is_microsoft_shared_path(path) {
        return true;
    }
    let p = norm_path(path);
    HARD_PATH_MARKERS.iter().any(|m| p.contains(m))
}

/// Heuristic: path/name looks like a shared runtime another app may need.
/// : bare `\common files\` vendor subpaths are NOT hard-shared; policy decides via association.
pub fn is_shared_item(name: &str, path: &str, reason: &str) -> bool {
    is_hard_shared_item(name, path, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forged_reason_cannot_mark_shared() {
        // reason is free text from scan — must not flip the shared flag.
        assert!(!is_shared_item(
            "DemoApp",
            r"C:\Program Files\DemoApp\bin",
            "visual c++ redistributable shared runtime"
        ));
    }

    #[test]
    fn flags_vcredist() {
        assert!(is_shared_item(
            "Microsoft Visual C++ 2015-2022 Redistributable",
            r"C:\ProgramData\Package Cache\{guid}\vc_redist.x64.exe",
            "install dir match"
        ));
    }

    #[test]
    fn flags_exact_common_files_roots() {
        assert!(is_shared_item("", r"C:\Program Files\Common Files", ""));
        assert!(is_shared_item(
            "",
            r"C:\Program Files (x86)\Common Files",
            ""
        ));
        assert!(is_shared_item(
            "",
            r"C:\Program Files\Common Files\Microsoft Shared",
            ""
        ));
        assert!(is_shared_item(
            "",
            r"C:\Program Files\Common Files\Microsoft Shared\",
            ""
        ));
        assert!(is_common_files_root(r"C:\Program Files\Common Files\"));
        assert!(!is_common_files_root(
            r"C:\Program Files\Common Files\Acme\lib.dll"
        ));
    }

    #[test]
    fn vendor_common_files_subpath_is_not_hard_shared() {
        //vendor subpath — policy uses association, not blanket shared flag.
        assert!(!is_shared_item(
            "",
            r"C:\Program Files\Common Files\Acme\lib.dll",
            ""
        ));
        assert!(is_common_files_vendor_path(
            r"C:\Program Files\Common Files\Acme\lib.dll"
        ));
        // Name tokens still hard-shared even under CF vendor path.
        assert!(is_shared_item(
            "vcredist helper",
            r"C:\Program Files\Common Files\Acme\lib.dll",
            ""
        ));
    }

    #[test]
    fn does_not_flag_random_app_cache() {
        assert!(!is_shared_item(
            "Foo",
            r"C:\Users\a\AppData\Local\Foo\Cache\x.dat",
            "temp"
        ));
    }

    #[test]
    fn first_guid_scans_all_brace_groups_and_validates_shape() {
        let g = "{12345678-1234-1234-1234-123456789ABC}";
        // The only brace group is a valid GUID (case-folded to lowercase).
        assert_eq!(first_guid(g).as_deref(), Some(g.to_lowercase().as_str()));
        // A short first group must not stop the scan.
        assert_eq!(
            first_guid("{bad} tail {12345678-1234-1234-1234-123456789ABC}").as_deref(),
            Some("{12345678-1234-1234-1234-123456789abc}")
        );
        // Shape validation: wrong length / non-hex bodies are not GUIDs.
        assert_eq!(first_guid("{123}"), None);
        assert_eq!(first_guid("{ZZZZZZZZ-1234-1234-1234-123456789ABC}"), None);
        assert_eq!(first_guid("no braces at all"), None);
        // Hyphens must sit at 8-4-4-4-12, not any 36-char hex/dash mix.
        assert_eq!(first_guid("{----1234-1234-1234-123456789ABCDEF}"), None);
        assert_eq!(first_guid("{123456781234-1234-1234-123456789ABC}"), None);
    }
}
