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
    /// Resolved executable path (empty when the tool is absent from PATH).
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
/// Hard ceiling on entries visited per search, so a huge tree cannot make the
/// bounded search effectively unbounded.
const CONSUMER_VISIT_BUDGET: usize = 200_000;

#[cfg(windows)]
fn vswhere_path() -> Option<std::path::PathBuf> {
    let root = crate::sysops::known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramFilesX86)?;
    Some(root.join(r"Microsoft Visual Studio\Installer\vswhere.exe"))
}

#[cfg(not(windows))]
fn vswhere_path() -> Option<std::path::PathBuf> {
    None
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

/// Prune names are matched against the lower-cased directory name, so every
/// entry here must be lower-case: `AppData` never matched and the search
/// descended into the whole profile AppData tree.
fn should_prune(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | ".git"
            | "target"
            | ".venv"
            | "venv"
            | "cache"
            | "appdata"
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
    // Total entries visited across every root. A user-chosen profile can hold
    // hundreds of thousands of entries at depth 7; without a budget the search
    // keeps walking long after the hit cap made the extra work pointless.
    let mut visited = 0usize;
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
                if visited >= CONSUMER_VISIT_BUDGET {
                    result.omitted += 1;
                    result.completeness.status = crate::scan_contract::ScanStatus::Partial;
                    result.completeness.roots_scanned = roots_scanned;
                    result.completeness.failed_roots = failed;
                    result.hits.sort_by(|a, b| a.path.cmp(&b.path));
                    return result;
                }
                // Cancellation must land per entry, not only per directory: one
                // very wide directory would otherwise delay it for a long time.
                if visited.is_multiple_of(256) && crate::scan_task::cancelled() {
                    result.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
                    result.completeness.roots_scanned = roots_scanned;
                    result.completeness.failed_roots = failed;
                    result.hits.sort_by(|a, b| a.path.cmp(&b.path));
                    return result;
                }
                visited += 1;
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let name_lower = name.to_lowercase();
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    // A junction under a user-chosen root may point at a UNC
                    // share: descending would authenticate to it. Same rule
                    // as dirsize/shellassoc.
                    if depth < CONSUMER_DEPTH
                        && !should_prune(&name_lower)
                        && !crate::fsutil::is_reparse_point(&path)
                    {
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

/// The SDK audit reports the resolved local file without starting it.
#[cfg(windows)]
fn probe_sdk(tool: &str, dirs: impl IntoIterator<Item = std::path::PathBuf>) -> SdkProbe {
    let executable = format!("{tool}.exe");
    let found = dirs.into_iter().find_map(|dir| {
        let candidate = dir.join(&executable);
        let _pins = crate::fsutil::pin_local_scan_path(&candidate).ok()?;
        candidate.is_file().then_some(candidate)
    });
    SdkProbe {
        tool: tool.into(),
        present: found.is_some(),
        detail: found
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// Read-only Visual Studio family audit. Notes carry the evidence quality.
pub fn audit_toolchains() -> ToolchainAuditResult {
    let mut result = ToolchainAuditResult::default();
    if crate::scan_task::cancelled() {
        result.completeness = crate::scan_contract::ScanCompleteness::cancelled(0);
        return result;
    }
    #[cfg(windows)]
    {
        let path = std::env::var_os("PATH").unwrap_or_default();
        result.sdks = ["dotnet", "rustup", "java"]
            .into_iter()
            .map(|tool| probe_sdk(tool, std::env::split_paths(&path)))
            .collect();
    }
    let Some((vswhere, _vswhere_pins)) = vswhere_path().and_then(|path| {
        let pins = crate::fsutil::pin_local_scan_path(&path).ok()?;
        path.is_file().then_some((path, pins))
    }) else {
        result.notes.push("vswhere_missing".into());
        result.completeness.status = crate::scan_contract::ScanStatus::Partial;
        result.completeness.failed_roots.push("vswhere".into());
        return result;
    };
    // The binary resolved: the audit can enumerate instances. Without this the
    // panel showed the "vswhere missing" warning even next to a full list.
    let mut vswhere_cmd = std::process::Command::new(&vswhere);
    vswhere_cmd
        .current_dir(vswhere.parent().expect("VS installer directory"))
        .env_remove("PATH")
        .args(["-all", "-prerelease", "-products", "*", "-format", "json"]);
    let output = crate::bounded_cmd::run_bounded(
        vswhere_cmd,
        crate::bounded_cmd::PROBE_TIMEOUT,
        crate::bounded_cmd::PROBE_OUTPUT_CAP,
    );
    let (vs_stdout, vs_success, vs_code) = match output {
        crate::bounded_cmd::BoundedOutput::Finished {
            status_code,
            success,
            stdout,
            ..
        } => (stdout, success, status_code),
        crate::bounded_cmd::BoundedOutput::TimedOut => {
            result.notes.push("vswhere_timeout".into());
            result.completeness.status = crate::scan_contract::ScanStatus::Partial;
            result.completeness.failed_roots.push("vswhere".into());
            return result;
        }
        crate::bounded_cmd::BoundedOutput::Failed(_) => {
            result.notes.push("vswhere_failed".into());
            result.completeness.status = crate::scan_contract::ScanStatus::Partial;
            result.completeness.failed_roots.push("vswhere".into());
            return result;
        }
    };
    if !vs_success {
        result
            .notes
            .push(format!("vswhere_exit:{}", vs_code.unwrap_or(-1)));
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
    let parsed: Vec<VsWhereInstance> = match serde_json::from_str(&vs_stdout) {
        Ok(list) => list,
        Err(e) => {
            result.notes.push(format!("vswhere_parse:{e}"));
            result.completeness.status = crate::scan_contract::ScanStatus::Partial;
            result.completeness.failed_roots.push("vswhere".into());
            return result;
        }
    };
    // Only flag the tool as available after it has actually answered — a
    // vswhere that resolved but failed to run would leave the flag true with
    // zero instances, and the panel would show a contradictory "no instances
    // found" alongside the failure notice.
    result.vswhere_available = true;
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
    // M4E.6 acceptance: the shared Package Cache backs repair/modify/uninstall
    // of EVERY remaining instance — report its footprint (a capped walk is a
    // lower bound, marked as such) so "drop the IDE" can be weighed against a
    // tight drive. Only reclaimable when no instance is left.
    let (cache_kb, cache_capped) =
        crate::dirsize::walk_size_kb_capped(std::path::Path::new(r"C:\ProgramData\Package Cache"));
    result.package_cache_kb = cache_kb;
    result.package_cache_capped = cache_capped;
    result.completeness.status = crate::scan_contract::ScanStatus::Complete;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn sdk_probe_reports_local_path_without_running_it() {
        let tmp = crate::test_support::TempDir::new("sdk-probe");
        let candidate = tmp.path().join("dotnet.exe");
        std::fs::write(&candidate, b"metadata fixture").unwrap();
        let sdk = probe_sdk("dotnet", [std::path::PathBuf::from("."), tmp.path().into()]);
        assert!(sdk.present);
        assert_eq!(std::path::Path::new(&sdk.detail), candidate);
        let missing = probe_sdk("java", [tmp.path().into()]);
        assert!(!missing.present);
        assert!(missing.detail.is_empty());
    }

    #[test]
    fn consumer_prune_list_blocks_dependency_and_vcs_dirs() {
        for name in ["node_modules", ".git", "target", ".venv", "appdata"] {
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
