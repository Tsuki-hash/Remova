//! Read-only directory references. Results never authorize cleanup.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{apps, fsutil, regops, regscan, scan_task};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Source {
    Installation,
    Shortcut,
    Service,
}

#[derive(Serialize)]
struct Reference {
    source: Source,
    name: String,
    location: String,
    target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_source: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FolderReferences {
    references: Vec<Reference>,
    incomplete: Vec<Source>,
    omitted: usize,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum InspectionOutcome {
    Complete(FolderReferences),
    Cancelled { checked: Vec<Source> },
}

// Compare recorded paths without resolving aliases or opening the targets.
fn key(path: &str) -> Option<String> {
    let raw = path.trim().trim_matches('"').replace('/', "\\");
    let (drive, rest) = raw.split_at_checked(3)?;
    if !drive.as_bytes()[0].is_ascii_alphabetic() || &drive.as_bytes()[1..] != b":\\" {
        return None;
    }
    for part in rest.trim_end_matches('\\').split('\\') {
        if [".", ".."].contains(&part)
            || part.contains([':', '%'])
            || part.chars().any(char::is_control)
        {
            return None;
        }
    }
    fsutil::path_key(Path::new(&raw))
}

fn contains(root: &str, value: &str) -> bool {
    value == root
        || value
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('\\'))
}

pub(crate) fn inspect_scoped(path: &str, scan_id: u32) -> Result<InspectionOutcome, String> {
    let mut checked = Vec::new();
    match scan_task::run_scoped(scan_id, "folder-references", || inspect(path, &mut checked)) {
        Ok(result) => result.map(InspectionOutcome::Complete),
        Err(error) if error == "scan:cancelled" => Ok(InspectionOutcome::Cancelled { checked }),
        Err(error) => Err(error),
    }
}

fn inspect(path: &str, checked: &mut Vec<Source>) -> Result<FolderReferences, String> {
    let path = path.trim();
    let root = key(path).ok_or("folder:unavailable")?;
    if !fsutil::local_scan_is_dir(Path::new(path)) {
        return Err("folder:unavailable".into());
    }
    let mut references = Vec::new();
    let mut incomplete = Vec::new();
    let mut omitted = 0;
    const MAX_PER_SOURCE: usize = 100;
    for source in [Source::Installation, Source::Service, Source::Shortcut] {
        if !scan_task::checkpoint("folder-references", references.len()) {
            return Err("scan:cancelled".into());
        }
        let mut count = 0;
        let mut add = |name, location, target: String, app_source| {
            let Some(target_key) = key(&target) else {
                return;
            };
            let related = contains(&root, &target_key)
                || matches!(source, Source::Installation) && contains(&target_key, &root);
            if !related {
                return;
            }
            count += 1;
            if count <= MAX_PER_SOURCE {
                references.push(Reference {
                    source,
                    name,
                    location,
                    target,
                    app_source,
                });
            }
        };
        let complete = match source {
            Source::Installation => {
                let inventory = apps::scan_install_owners();
                for app in inventory.apps {
                    add(
                        app.name,
                        app.registry_key,
                        app.install_location,
                        Some(app.source),
                    );
                }
                inventory.completeness.status == crate::scan_contract::ScanStatus::Complete
            }
            Source::Service => services(&mut add),
            Source::Shortcut => shortcuts(&mut add),
        };
        omitted += count.saturating_sub(MAX_PER_SOURCE);
        if complete {
            checked.push(source);
        } else {
            incomplete.push(source);
        }
    }
    if scan_task::cancelled() {
        return Err("scan:cancelled".into());
    }
    Ok(FolderReferences {
        references,
        incomplete,
        omitted,
    })
}

#[cfg(windows)]
fn services(add: &mut impl FnMut(String, String, String, Option<String>)) -> bool {
    const ROOT: &str = r"HKLM64\SYSTEM\CurrentControlSet\Services";
    let Ok((names, _)) = regscan::snapshot_names(ROOT) else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut complete = true;
    for name in names {
        if scan_task::cancelled() || Instant::now() >= deadline {
            return false;
        }
        let location = format!(r"{ROOT}\{name}");
        match regops::read_reg_value_bytes(&location, "ImagePath") {
            Ok(Some((data, 1 | 2))) if data.len() % 2 == 0 => {
                let command = fsutil::wstring_from_reg_data(&data);
                if let Some(program) = crate::scanner::reg_scans::service_image_exe(&command)
                    .and_then(|path| path.into_os_string().into_string().ok())
                {
                    let label = regscan::read_string(&location, "DisplayName")
                        .filter(|value| !value.is_empty() && !value.starts_with('@'))
                        .unwrap_or(name);
                    add(label, location, program, None);
                }
            }
            Ok(None) => (),
            _ => complete = false,
        }
    }
    complete
}

#[cfg(windows)]
fn shortcuts(add: &mut impl FnMut(String, String, String, Option<String>)) -> bool {
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_CommonStartMenu, FOLDERID_Desktop, FOLDERID_PublicDesktop, FOLDERID_StartMenu,
    };
    let mut complete = true;
    let mut dirs = Vec::new();
    for id in [
        FOLDERID_Desktop,
        FOLDERID_StartMenu,
        FOLDERID_PublicDesktop,
        FOLDERID_CommonStartMenu,
    ] {
        if let Some(path) = crate::sysops::known_folder(&id) {
            dirs.push((path, 0));
        } else {
            complete = false;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut remaining = 4096usize;
    let mut seen = std::collections::HashSet::new();
    while let Some((dir, depth)) = dirs.pop() {
        if scan_task::cancelled() || Instant::now() >= deadline || remaining == 0 {
            return false;
        }
        let Ok((entries, capped)) = fsutil::read_local_directory_entries(&dir, deadline) else {
            complete = false;
            continue;
        };
        complete &= !capped;
        for (name, attrs) in entries {
            if scan_task::cancelled() || Instant::now() >= deadline || remaining == 0 {
                return false;
            }
            remaining -= 1;
            let path = dir.join(name);
            if attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                complete = false;
            } else if attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
                if depth < 8 {
                    dirs.push((path, depth + 1));
                } else {
                    complete = false;
                }
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            {
                let Some(location) = path.to_str() else {
                    complete = false;
                    continue;
                };
                if !seen.insert(fsutil::path_key(&path)) {
                    continue;
                }
                if let Some(target) = crate::manage::presentation::shortcut_target(&path) {
                    let label = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    add(label, location.to_owned(), target, None);
                } else {
                    complete = false;
                }
            }
        }
    }
    complete
}

#[cfg(not(windows))]
fn services(_add: &mut impl FnMut(String, String, String, Option<String>)) -> bool {
    false
}

#[cfg(not(windows))]
fn shortcuts(_add: &mut impl FnMut(String, String, String, Option<String>)) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_reservation_reports_no_checked_sources() {
        let scan = scan_task::begin().unwrap();
        assert!(scan_task::cancel(scan).unwrap());
        let result = inspect_scoped("", scan).unwrap();
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::json!({ "checked": [] })
        );
    }

    #[test]
    fn references_require_an_absolute_local_path_and_a_directory_boundary() {
        let root = key(r"D:\Apps\Editor").unwrap();
        assert!(contains(
            &root,
            &key(r"d:/APPS/editor/updater.exe").unwrap()
        ));
        assert!(!contains(
            &root,
            &key(r"D:\Apps\Editor2\updater.exe").unwrap()
        ));
        for path in [
            r"\\server\share\Editor",
            r"D:\Apps\..\Editor",
            r"D:Editor",
            r"D:\Editor:stream",
            "目录\\编辑器",
        ] {
            assert!(key(path).is_none(), "{path}");
        }
    }
}
