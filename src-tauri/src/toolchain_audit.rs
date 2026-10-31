//! Toolchain / IDE audit (M4E.6, read-only) — evidence for "is this big
//! install still used?". Visual Studio family first: enumerate EVERY instance
//! (vswhere `-all`), probe workloads on the filesystem, and find project
//! consumers under user-chosen roots.
//!
//! The functional build probe is deliberately not implemented here: running a
//! compiler is a user-confirmed action with minutes of cost, and the
//! enumeration + consumer evidence answers most decisions. It stays on the
//! plan as a follow-up gated behind an explicit confirmation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VsInstance {
    pub display_name: String,
    pub installation_path: String,
    pub installation_version: String,
    pub product_id: String,
    /// Filesystem workload probes (installer JSON does not list workloads).
    pub has_msvc: bool,
    pub has_msbuild: bool,
    pub has_ide: bool,
    #[serde(default)]
    pub size_kb: i64,
    #[serde(default)]
    pub size_capped: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolchainAuditResult {
    pub vswhere_available: bool,
    pub instances: Vec<VsInstance>,
    /// Read-only SDK presence probes: the tool resolved on PATH, or a stable
    /// "absent". Absence is only ever claimed from PATH resolution.
    #[serde(default)]
    pub sdks: Vec<SdkProbe>,
    /// Shared `C:\ProgramData\Package Cache` footprint (lower bound when
    /// capped): it backs repair/uninstall of every remaining instance and is
    /// only reclaimable when none are left.
    #[serde(default)]
    pub package_cache_kb: i64,
    #[serde(default)]
    pub package_cache_capped: bool,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub completeness: crate::scan_contract::ScanCompleteness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SdkProbe {
    /// `dotnet` | `rustup` | `java`
    pub tool: String,
    pub present: bool,
    /// First line of the tool's own version output (empty when absent).
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerHit {
    pub root: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsumerSearchResult {
    pub hits: Vec<ConsumerHit>,
    #[serde(default)]
    pub omitted: u32,
    #[serde(default)]
    pub completeness: crate::scan_contract::ScanCompleteness,
}

const CONSUMER_CAP: usize = 200;
const CONSUMER_DEPTH: usize = 7;

fn vswhere_path() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let pf86 = std::env::var_os("ProgramFiles(x86)")
            .map(std::path::PathBuf::from)
            .or_else(|| Some(std::path::PathBuf::from(r"C:\Program Files (x86)")))?;
        let p = pf86.join(r"Microsoft Visual Studio\Installer\vswhere.exe");
        p.is_file().then_some(p)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// `is_dir` for directory workloads (MSVC/MSBuild); devenv.exe is a file, so
/// the IDE probe is an existence check — `is_dir` would always answer "no"
/// for every full IDE.
fn probe_path(instance: &str, rel: &str, want_file: bool) -> bool {
    let joined = std::path::Path::new(instance).join(rel);
    if want_file {
        joined.is_file()
    } else {
        joined.is_dir()
    }
}

fn should_prune(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | ".git"
            | "target"
            | ".venv"
            | "venv"
            | "cache"
            | "AppData"
            | ".cargo"
            | ".rustup"
    )
}

fn is_consumer_file(name_lower: &str) -> bool {
    name_lower.ends_with(".sln")
        || name_lower.ends_with(".csproj")
        || name_lower.ends_with(".vcxproj")
        || name_lower.ends_with(".fsproj")
        || name_lower.ends_with(".slnx")
        || name_lower.ends_with("cargo.toml")
}

/// Bounded consumer search: depth 7, prune dirs, cap hits, count the rest.
pub fn find_consumers(roots: &[String]) -> ConsumerSearchResult {
    let mut result = ConsumerSearchResult::default();
    if crate::scan_task::cancelled() {
        result.completeness = crate::scan_contract::ScanCompleteness::cancelled(0);
        return result;
    }
    let mut failed = Vec::new();
    let mut roots_scanned = 0u32;
    for raw in roots {
        let root = raw.trim();
        if root.is_empty() {
            continue;
        }
        if !crate::fsutil::is_local_drive_path(root) || !std::path::Path::new(root).is_dir() {
            failed.push(root.to_string());
            continue;
        }
        roots_scanned += 1;
        let mut stack = vec![(std::path::PathBuf::from(root), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if crate::scan_task::cancelled() {
                result.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
                result.completeness.roots_scanned = roots_scanned;
                result.completeness.failed_roots = failed;
                return result;
            }
            let Ok(read) = std::fs::read_dir(&dir) else {
                failed.push(dir.to_string_lossy().into_owned());
                continue;
            };
            for entry in read {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let name_lower = name.to_lowercase();
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    if depth < CONSUMER_DEPTH && !should_prune(&name_lower) {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                if !is_consumer_file(&name_lower) {
                    continue;
                }
                if result.hits.len() >= CONSUMER_CAP {
                    result.omitted += 1;
                    continue;
                }
                result.hits.push(ConsumerHit {
                    root: root.to_string(),
                    path: path.to_string_lossy().into_owned(),
                });
            }
        }
    }
    result.hits.sort_by(|a, b| a.path.cmp(&b.path));
    result.completeness.roots_scanned = roots_scanned;
    result.completeness.failed_roots = failed;
    result.completeness.status = if result.completeness.failed_roots.is_empty() {
        crate::scan_contract::ScanStatus::Complete
    } else {
        crate::scan_contract::ScanStatus::Partial
    };
    result
}

/// Probe one SDK tool via PATH resolution (`where`) and its own version flag.
/// Read-only; a tool that resolves but fails to run reports "present" with
/// the error as detail — absence is only ever claimed from PATH resolution.
#[cfg(windows)]
fn probe_sdk(tool: &str, version_arg: &str) -> SdkProbe {
    let resolved = std::process::Command::new(crate::regops::sys_tool("where.exe"))
        .arg(tool)
        .output();
    let Ok(output) = resolved else {
        return SdkProbe {
            tool: tool.into(),
            present: false,
            detail: String::new(),
        };
    };
    if !output.status.success() {
        return SdkProbe {
            tool: tool.into(),
            present: false,
            detail: String::new(),
        };
    }
    let first = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if first.is_empty() {
        return SdkProbe {
            tool: tool.into(),
            present: false,
            detail: String::new(),
        };
    }
    let run = std::process::Command::new(&first).arg(version_arg).output();
    let detail = match run {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
        Err(e) => format!("resolved at {first}; run failed: {e}"),
    };
    SdkProbe {
        tool: tool.into(),
        present: true,
        detail,
    }
}

#[cfg(not(windows))]
fn probe_sdk(tool: &str, _version_arg: &str) -> SdkProbe {
    SdkProbe {
        tool: tool.into(),
        present: false,
        detail: String::new(),
    }
}

/// Read-only Visual Studio family audit. Notes carry the evidence quality.
pub fn audit_toolchains() -> ToolchainAuditResult {
    let mut result = ToolchainAuditResult::default();
    if crate::scan_task::cancelled() {
        result.completeness = crate::scan_contract::ScanCompleteness::cancelled(0);
        return result;
    }
    let Some(vswhere) = vswhere_path() else {
        result.notes.push("vswhere_missing".into());
        result.completeness.status = crate::scan_contract::ScanStatus::Partial;
        result.completeness.failed_roots.push("vswhere".into());
        return result;
    };
    let output = std::process::Command::new(&vswhere)
        .args(["-all", "-prerelease", "-products", "*", "-format", "json"])
        .output();
    let Ok(output) = output else {
        result.notes.push("vswhere_failed".into());
        result.completeness.status = crate::scan_contract::ScanStatus::Partial;
        result.completeness.failed_roots.push("vswhere".into());
        return result;
    };
    if !output.status.success() {
        result.notes.push(format!(
            "vswhere_exit:{}",
            output.status.code().unwrap_or(-1)
        ));
        result.completeness.status = crate::scan_contract::ScanStatus::Partial;
        result.completeness.failed_roots.push("vswhere".into());
        return result;
    }
    result.completeness.roots_scanned += 1;
    #[derive(serde::Deserialize)]
    struct VsWhereInstance {
        #[serde(rename = "displayName")]
        display_name: Option<String>,
        #[serde(rename = "installationPath")]
        installation_path: Option<String>,
        #[serde(rename = "installationVersion")]
        installation_version: Option<String>,
        #[serde(rename = "productId")]
        product_id: Option<String>,
    }
    let parsed: Vec<VsWhereInstance> = match serde_json::from_slice(&output.stdout) {
        Ok(list) => list,
        Err(e) => {
            result.notes.push(format!("vswhere_parse:{e}"));
            result.completeness.status = crate::scan_contract::ScanStatus::Partial;
            result.completeness.failed_roots.push("vswhere".into());
            return result;
        }
    };
    for instance in parsed {
        let path = instance.installation_path.unwrap_or_default();
        if path.is_empty() {
            continue;
        }
        let (size_kb, size_capped) =
            crate::dirsize::walk_size_kb_capped(std::path::Path::new(&path));
        result.instances.push(VsInstance {
            display_name: instance
                .display_name
                .unwrap_or_else(|| "Visual Studio".into()),
            installation_path: path.clone(),
            installation_version: instance.installation_version.unwrap_or_default(),
            product_id: instance.product_id.unwrap_or_default(),
            has_msvc: probe_path(&path, r"VC\Tools\MSVC", false),
            has_msbuild: probe_path(&path, r"MSBuild\Current\Bin", false),
            has_ide: probe_path(&path, r"Common7\IDE\devenv.exe", true),
            size_kb,
            size_capped,
        });
    }
    for (tool, arg) in [
        ("dotnet", "--list-sdks"),
        ("rustup", "show"),
        ("java", "-version"),
    ] {
        if crate::scan_task::cancelled() {
            result.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
            return result;
        }
        result.sdks.push(probe_sdk(tool, arg));
    }
    result.completeness.status = crate::scan_contract::ScanStatus::Complete;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_prune_list_blocks_dependency_and_vcs_dirs() {
        for name in ["node_modules", ".git", "target", ".venv", "AppData"] {
            assert!(should_prune(name), "{name} must be pruned");
        }
        assert!(!should_prune("projects"));
    }

    #[test]
    fn consumer_search_bounds_hits_and_reports_omitted() {
        let tmp = crate::test_support::TempDir::new("tool-audit");
        let root = tmp.path().to_string_lossy().into_owned();
        for i in 0..5 {
            std::fs::write(tmp.path().join(format!("p{i}.sln")), b"sln").unwrap();
        }
        std::fs::create_dir_all(tmp.path().join("node_modules").join("deep")).unwrap();
        std::fs::write(
            tmp.path()
                .join("node_modules")
                .join("deep")
                .join("nested.sln"),
            b"hidden",
        )
        .unwrap();
        let result = find_consumers(&[root]);
        assert_eq!(result.hits.len(), 5, "pruned dir must not be searched");
        assert_eq!(result.omitted, 0);
        assert_eq!(
            result.completeness.status,
            crate::scan_contract::ScanStatus::Complete
        );
    }

    #[test]
    fn consumer_search_marks_missing_roots_partial() {
        let result = find_consumers(&[r"C:\definitely-not-a-root-xyz".into()]);
        assert_eq!(
            result.completeness.status,
            crate::scan_contract::ScanStatus::Partial
        );
        assert_eq!(result.completeness.failed_roots.len(), 1);
    }
}
