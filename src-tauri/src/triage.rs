//! Space triage for the C-drive overview (SOP judgement table, read-only).
//!
//! Every entry gets one disposition class and a stable reason code; the scan
//! never deletes, migrates, or probes anything beyond a bounded size walk.
//! The deletion tier (regenerable layouts) and the migration tier are later
//! milestones — this module only explains what the user is looking at.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ENTRY_CAP: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageClass {
    /// Regenerable system temp/log/cache — the delete tier (M6.2).
    DeleteRegenerable,
    /// WinSxS — DISM is the only sanctioned tool.
    ViaDism,
    /// User data / program body — the migration tier (M6.3).
    MigrateCandidate,
    /// Installer source caches, the system tree, and everything unrated.
    Leave,
    /// hiberfil/pagefile/swapfile: sized for display, never touched here.
    ShowOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriageEntry {
    pub path: String,
    pub class: TriageClass,
    /// Stable reason code the UI localises.
    pub reason: String,
    #[serde(default)]
    pub size_kb: Option<i64>,
    #[serde(default)]
    pub size_capped: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriageScanResult {
    pub entries: Vec<TriageEntry>,
    #[serde(default)]
    pub omitted: u32,
}

/// The roots classification anchors on, resolved once per scan and passed in
/// lowercase so the rule table is testable without the environment.
#[derive(Debug, Clone, Default)]
pub struct TriageRoots {
    pub windows: PathBuf,
    pub program_files: PathBuf,
    pub program_files_x86: Option<PathBuf>,
    pub program_data: PathBuf,
    pub local_app_data: PathBuf,
    pub roaming_app_data: PathBuf,
    pub user_profile: PathBuf,
}

pub fn roots_from_env() -> TriageRoots {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    TriageRoots {
        windows: var("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows")),
        program_files: var("ProgramFiles").unwrap_or_else(|| PathBuf::from(r"C:\Program Files")),
        program_files_x86: var("ProgramFiles(x86)"),
        program_data: var("ProgramData").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData")),
        local_app_data: var("LOCALAPPDATA").unwrap_or_default(),
        roaming_app_data: var("APPDATA").unwrap_or_default(),
        user_profile: var("USERPROFILE").unwrap_or_default(),
    }
}

fn lower(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

fn under(path_lower: &str, root_lower: &str) -> Option<String> {
    let rel = path_lower.strip_prefix(root_lower)?;
    let rel = rel.trim_start_matches(['/', '\\']);
    if rel.is_empty() {
        None
    } else {
        Some(rel.to_string())
    }
}

/// One classified path. Pure: the rule table lives here and nowhere else.
pub fn classify(path: &Path, roots: &TriageRoots) -> Option<(TriageClass, &'static str)> {
    let p = lower(path);
    let file_name = path.file_name()?.to_string_lossy().to_lowercase();
    if matches!(
        file_name.as_str(),
        "hiberfil.sys" | "pagefile.sys" | "swapfile.sys"
    ) {
        return Some((TriageClass::ShowOnly, "system_swap_file"));
    }
    if let Some(rel) = under(&p, &lower(&roots.windows)) {
        return Some(match rel.as_str() {
            r"softwaredistribution\download" => {
                (TriageClass::DeleteRegenerable, "win_update_download")
            }
            "cbstemp" => (TriageClass::DeleteRegenerable, "cbs_temp"),
            "temp" => (TriageClass::DeleteRegenerable, "win_temp"),
            "minidump" => (TriageClass::DeleteRegenerable, "crash_dumps"),
            "winsxs" => (TriageClass::ViaDism, "winsxs_dism_only"),
            r"installer" => (TriageClass::Leave, "installer_source_cache"),
            _ => (TriageClass::Leave, "system_tree"),
        });
    }
    if let Some(rel) = under(&p, &lower(&roots.program_data)) {
        if rel == r"package cache" {
            return Some((TriageClass::Leave, "installer_source_cache"));
        }
        if rel == r"microsoft\windows\wer" {
            return Some((TriageClass::DeleteRegenerable, "wer_reports"));
        }
        return Some((TriageClass::MigrateCandidate, "programdata_app"));
    }
    for pf in [Some(&roots.program_files), roots.program_files_x86.as_ref()]
        .into_iter()
        .flatten()
    {
        if under(&p, &lower(pf)).is_some() {
            return Some((TriageClass::MigrateCandidate, "program_body"));
        }
    }
    if let Some(rel) = under(&p, &lower(&roots.local_app_data)) {
        return Some(match rel.as_str() {
            "temp" => (TriageClass::DeleteRegenerable, "user_temp"),
            "crashdumps" => (TriageClass::DeleteRegenerable, "crash_dumps"),
            "d3dscache" => (TriageClass::DeleteRegenerable, "shader_cache"),
            _ => (TriageClass::MigrateCandidate, "app_data"),
        });
    }
    if under(&p, &lower(&roots.roaming_app_data)).is_some() {
        return Some((TriageClass::MigrateCandidate, "app_data"));
    }
    if let Some(rel) = under(&p, &lower(&roots.user_profile)) {
        if rel.starts_with('.') {
            return Some((TriageClass::MigrateCandidate, "user_data"));
        }
    }
    None
}

fn push_entry(out: &mut Vec<TriageEntry>, path: &Path, roots: &TriageRoots) {
    if out.len() >= ENTRY_CAP {
        return;
    }
    let Some((class, reason)) = classify(path, roots) else {
        return;
    };
    let (size_kb, size_capped) = if class == TriageClass::ShowOnly {
        (
            std::fs::metadata(path)
                .ok()
                .map(|m| (m.len() / 1024) as i64),
            false,
        )
    } else {
        let (kb, capped) = crate::dirsize::walk_size_kb_capped(path);
        (Some(kb), capped)
    };
    out.push(TriageEntry {
        path: path.to_string_lossy().into_owned(),
        class,
        reason: reason.into(),
        size_kb,
        size_capped,
    });
}

/// Enumerate the anchored roots' direct children plus the swap files, classify
/// each, size with the bounded walker, and sort biggest-first.
pub fn scan_space_triage() -> TriageScanResult {
    let roots = roots_from_env();
    let mut entries: Vec<TriageEntry> = Vec::new();
    // Fixed layouts first so the delete tier is always represented.
    let win = &roots.windows;
    let fixed = [
        win.join(r"SoftwareDistribution\Download"),
        win.join("CbsTemp"),
        win.join("Temp"),
        win.join("Minidump"),
        win.join("WinSxS"),
        win.join("Installer"),
        roots.program_data.join("Package Cache"),
        roots.program_data.join(r"Microsoft\Windows\WER"),
        roots.local_app_data.join("Temp"),
        roots.local_app_data.join("CrashDumps"),
        roots.local_app_data.join("D3DSCache"),
    ];
    let mut omitted: u32 = 0;
    for path in fixed {
        if path.as_os_str().is_empty() {
            continue;
        }
        if path.try_exists().unwrap_or(false) {
            if entries.len() >= ENTRY_CAP {
                omitted += 1;
            } else {
                push_entry(&mut entries, &path, &roots);
            }
        }
    }
    // Swap files on the system drive root.
    let drive_root = roots.windows.parent().map(Path::to_path_buf);
    if let Some(drive) = drive_root {
        for name in ["hiberfil.sys", "pagefile.sys", "swapfile.sys"] {
            let p = drive.join(name);
            if entries.len() >= ENTRY_CAP {
                omitted += 1;
            } else if p.try_exists().unwrap_or(false) {
                push_entry(&mut entries, &p, &roots);
            }
        }
    }
    // Direct children of the anchor roots (top-level apps / dot-dirs).
    let anchors = [
        Some(&roots.program_files),
        roots.program_files_x86.as_ref(),
        Some(&roots.program_data),
        Some(&roots.local_app_data),
        Some(&roots.roaming_app_data),
        Some(&roots.user_profile),
    ];
    for anchor in anchors.into_iter().flatten() {
        if anchor.as_os_str().is_empty() {
            continue;
        }
        let Ok(read) = std::fs::read_dir(anchor) else {
            continue;
        };
        for e in read.flatten() {
            if entries.len() >= ENTRY_CAP {
                omitted += 1;
                break;
            }
            let p = e.path();
            // Only directories classify here; files under profile roots (ntuser.dat…) stay out.
            if e.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                push_entry(&mut entries, &p, &roots);
            }
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.size_kb.unwrap_or(0)));
    TriageScanResult { entries, omitted }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> TriageRoots {
        TriageRoots {
            windows: PathBuf::from(r"C:\Win"),
            program_files: PathBuf::from(r"C:\PF"),
            program_files_x86: Some(PathBuf::from(r"C:\PF86")),
            program_data: PathBuf::from(r"C:\PD"),
            local_app_data: PathBuf::from(r"C:\U\a\AppData\Local"),
            roaming_app_data: PathBuf::from(r"C:\U\a\AppData\Roaming"),
            user_profile: PathBuf::from(r"C:\U\a"),
        }
    }

    #[test]
    fn classification_follows_the_judgement_table() {
        let r = roots();
        let cases: Vec<(&str, TriageClass, &str)> = vec![
            (
                r"C:\Win\SoftwareDistribution\Download",
                TriageClass::DeleteRegenerable,
                "win_update_download",
            ),
            (
                r"c:\win\CbsTemp",
                TriageClass::DeleteRegenerable,
                "cbs_temp",
            ),
            (r"C:\Win\WinSxS", TriageClass::ViaDism, "winsxs_dism_only"),
            (
                r"C:\Win\Installer",
                TriageClass::Leave,
                "installer_source_cache",
            ),
            (r"C:\Win\System32", TriageClass::Leave, "system_tree"),
            (
                r"C:\PD\Package Cache",
                TriageClass::Leave,
                "installer_source_cache",
            ),
            (
                r"C:\PD\Microsoft\Windows\WER",
                TriageClass::DeleteRegenerable,
                "wer_reports",
            ),
            (
                r"C:\PF\Vendor App",
                TriageClass::MigrateCandidate,
                "program_body",
            ),
            (
                r"C:\PF86\Tool",
                TriageClass::MigrateCandidate,
                "program_body",
            ),
            (
                r"C:\U\a\AppData\Local\Temp",
                TriageClass::DeleteRegenerable,
                "user_temp",
            ),
            (
                r"C:\U\a\AppData\Local\D3DSCache",
                TriageClass::DeleteRegenerable,
                "shader_cache",
            ),
            (
                r"C:\U\a\AppData\Local\Slack",
                TriageClass::MigrateCandidate,
                "app_data",
            ),
            (
                r"C:\U\a\AppData\Roaming\Slack",
                TriageClass::MigrateCandidate,
                "app_data",
            ),
            (r"C:\U\a\.cargo", TriageClass::MigrateCandidate, "user_data"),
            (
                r"C:\hiberfil.sys",
                TriageClass::ShowOnly,
                "system_swap_file",
            ),
            (
                r"C:\pagefile.sys",
                TriageClass::ShowOnly,
                "system_swap_file",
            ),
        ];
        for (path, class, reason) in cases {
            let got = classify(Path::new(path), &r);
            assert_eq!(got, Some((class, reason)), "case {path}");
        }
    }

    #[test]
    fn unrated_paths_and_profile_files_stay_out() {
        let r = roots();
        assert_eq!(classify(Path::new(r"C:\Games"), &r), None);
        // A file (not dir) under the profile: ntuser.dat must not classify.
        assert_eq!(classify(Path::new(r"C:\U\a\ntuser.dat"), &r), None);
    }
}
