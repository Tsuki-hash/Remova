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

const SHARED_PATH_MARKERS: &[&str] = &[
    "\\windows\\system32\\",
    "\\windows\\syswow64\\",
    "\\common files\\",
    "\\microsoft shared\\",
    "\\windows\\assembly\\",
    "\\windows\\microsoft.net\\",
    "\\dotnet\\",
    "\\vcredist\\",
];

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// Heuristic: path/name looks like a shared runtime another app may need.
pub fn is_shared_item(name: &str, path: &str, reason: &str) -> bool {
    let blob = format!("{}\n{}\n{}", lower(name), lower(path), lower(reason));
    if SHARED_NAME_TOKENS.iter().any(|t| blob.contains(t)) {
        return true;
    }
    let p = lower(&path.replace('/', "\\"));
    if SHARED_PATH_MARKERS.iter().any(|m| p.contains(m)) {
        // System32 only when reason/evidence already suggests leftover association
        // (avoid flagging random System32 hits from aggressive scanners).
        if p.contains("\\windows\\system32\\") || p.contains("\\windows\\syswow64\\") {
            return reason.to_lowercase().contains("install")
                || reason.to_lowercase().contains("product")
                || name.to_lowercase().contains("c++")
                || name.to_lowercase().contains("redistributable");
        }
        return true;
    }
    false
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
    fn flags_common_files() {
        assert!(is_shared_item(
            "Shared Component",
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
