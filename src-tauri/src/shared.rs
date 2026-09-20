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
pub fn is_hard_shared_item(name: &str, path: &str, reason: &str) -> bool {
    let blob = format!("{}\n{}\n{}", lower(name), lower(path), lower(reason));
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
/// S-7B: bare `\common files\` vendor subpaths are NOT hard-shared; policy decides via association.
pub fn is_shared_item(name: &str, path: &str, reason: &str) -> bool {
    is_hard_shared_item(name, path, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // S-7B: vendor subpath — policy uses association, not blanket shared flag.
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
}
