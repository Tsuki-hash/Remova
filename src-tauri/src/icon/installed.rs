//! Icon sources for the installed-software list, resolved from scanned records.

use super::{extract_icon_png, looks_like_png};
use crate::apps::{parse_display_icon, InstalledApp};
use crate::fsutil::pin_local_scan_path;
use crate::scanner::normalize_for_match;
use std::collections::HashMap;
use std::io::Read;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_PublicDesktop,
};

type Icon = (&'static str, Vec<u8>);
type ShortcutIndex = HashMap<String, Vec<PathBuf>>;
static START_MENU: Mutex<Option<Arc<ShortcutIndex>>> = Mutex::new(None);
static ICON_APPS: Mutex<Option<HashMap<String, InstalledApp>>> = Mutex::new(None);
const MAX_IMAGE_BYTES: usize = 512 * 1024;

pub(crate) fn remember_icon_apps(apps: &[InstalledApp]) {
    let next = apps
        .iter()
        .map(|app| (crate::apps::app_key(app), app.clone()))
        .collect();
    if let Ok(mut current) = ICON_APPS.lock() {
        *current = Some(next);
    }
    if let Ok(mut index) = START_MENU.lock() {
        *index = None;
    }
}

pub(crate) fn installed_icon(key: &str) -> Option<Icon> {
    let app = ICON_APPS.lock().ok()?.as_ref()?.get(key)?.clone();
    resolve_icon(&app)
}

fn resolve_icon(app: &InstalledApp) -> Option<Icon> {
    if app.source == crate::apps::STORE_INVENTORY_ROOT {
        return app
            .registry_key
            .strip_prefix("Store\\")
            .and_then(|full| store_icon(full, &app.name));
    }
    if let Some(icon) = file_icon(&app.display_icon) {
        return Some(icon);
    }
    if let Some(icon) = msi_icon(app).and_then(|path| file_icon(&path)) {
        return Some(icon);
    }
    let name = normalize_for_match(product_name_for_icon(&app.name, &app.version));
    if name.is_empty() {
        return None;
    }
    let root = PathBuf::from(crate::regops::expand_env_string(
        app.install_location.trim().trim_matches('"'),
    ));
    let _root_pins = pin_local_scan_path(&root).ok().filter(|_| {
        root.parent()
            .is_some_and(|parent| parent.parent().is_some())
            && root.is_dir()
    });
    let root = _root_pins.as_ref().map(|_| root.as_path());
    shortcut_icon(&name, root, app.install_location.trim().is_empty())
        .or_else(|| root.and_then(|root| main_executable_icon(&name, root)))
}

fn product_name_for_icon<'a>(name: &'a str, version: &str) -> &'a str {
    // Uninstall records can append the installed version or architecture to
    // the product name shown in the Start menu.
    let name = name.trim();
    let name = [" (64-bit)", " (32-bit)", " (x64)", " (x86)", " (arm64)"]
        .into_iter()
        .find_map(|suffix| {
            let prefix = name.get(..name.len().checked_sub(suffix.len())?)?;
            name[prefix.len()..]
                .eq_ignore_ascii_case(suffix)
                .then_some(prefix)
        })
        .unwrap_or(name);
    let version = version.trim();
    name.strip_suffix(version)
        .and_then(|prefix| prefix.strip_suffix(' '))
        .filter(|_| !version.is_empty())
        .unwrap_or(name)
}

fn file_icon(raw: &str) -> Option<Icon> {
    let (path, index) = parse_display_icon(raw);
    let path = crate::regops::expand_env_string(&path);
    let source = Path::new(&path);
    let extension = source.extension()?.to_str()?.to_ascii_lowercase();
    if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "ico") && index == 0 {
        let _pins = pin_local_scan_path(source).ok()?;
        let mut bytes = Vec::new();
        std::fs::File::open(source)
            .ok()?
            .take(MAX_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        return raster_icon(bytes);
    }
    extract_icon_png(&format!("{path},{index}"), true).map(|bytes| ("image/png", bytes))
}

fn raster_icon(bytes: Vec<u8>) -> Option<Icon> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mime = if looks_like_png(&bytes) {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        "image/bmp"
    } else if bytes.starts_with(&[0, 0, 1, 0]) && bytes.len() >= 22 {
        "image/x-icon"
    } else {
        return None;
    };
    Some((mime, bytes))
}

fn msi_icon(app: &InstalledApp) -> Option<String> {
    use windows::Win32::System::ApplicationInstallationAndServicing::{
        MsiGetProductInfoExW, INSTALLPROPERTY_PRODUCTICON, MSIINSTALLCONTEXT_MACHINE,
        MSIINSTALLCONTEXT_USERMANAGED, MSIINSTALLCONTEXT_USERUNMANAGED,
    };

    let code = crate::apps::msiexec_uninstall_guid_public(&app.uninstall_string)
        .or_else(|| crate::apps::msiexec_uninstall_guid_public(&app.quiet_uninstall_string))?;
    if !app
        .registry_key
        .rsplit('\\')
        .next()?
        .eq_ignore_ascii_case(&code)
    {
        return None;
    }
    let contexts = match app.source.as_str() {
        "HKCU" => &[
            MSIINSTALLCONTEXT_USERUNMANAGED,
            MSIINSTALLCONTEXT_USERMANAGED,
        ][..],
        "HKLM64" | "HKLM32" => &[MSIINSTALLCONTEXT_MACHINE],
        _ => return None,
    };
    let code = HSTRING::from(code);
    let mut buffer = [0u16; 1024];
    contexts.iter().find_map(|context| {
        let mut len = buffer.len() as u32;
        let result = unsafe {
            MsiGetProductInfoExW(
                &code,
                PCWSTR::null(),
                *context,
                INSTALLPROPERTY_PRODUCTICON,
                PWSTR(buffer.as_mut_ptr()),
                Some(&mut len),
            )
        };
        (result == 0 && len > 0).then(|| String::from_utf16_lossy(&buffer[..len as usize]))
    })
}

fn shortcut_index() -> Option<Arc<ShortcutIndex>> {
    let mut index = START_MENU.lock().ok()?;
    if let Some(existing) = index.as_ref() {
        return Some(existing.clone());
    }
    let mut entries = ShortcutIndex::new();
    let mut dirs: Vec<_> = [
        (FOLDERID_Programs, 0),
        (FOLDERID_CommonPrograms, 0),
        (FOLDERID_Desktop, 8),
        (FOLDERID_PublicDesktop, 8),
    ]
    .iter()
    .filter_map(|(id, depth)| crate::sysops::known_folder(id).map(|path| (path, *depth)))
    .collect();
    let mut remaining = 8192;
    while let Some((dir, depth)) = dirs.pop() {
        if remaining == 0 {
            break;
        }
        let Ok(_pins) = pin_local_scan_path(&dir) else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in files.take(remaining) {
            remaining -= 1;
            let Ok(file) = file else { continue };
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                continue;
            }
            let path = file.path();
            if metadata.is_dir() && depth < 8 {
                dirs.push((path, depth + 1));
            } else if metadata.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            {
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    entries
                        .entry(normalize_for_match(stem))
                        .or_default()
                        .push(path);
                }
            }
        }
    }
    let entries = Arc::new(entries);
    *index = Some(entries.clone());
    Some(entries)
}

fn shortcut_icon(name: &str, root: Option<&Path>, no_install_dir: bool) -> Option<Icon> {
    let index = shortcut_index()?;
    let prefix = root.and_then(Path::to_str).map(|path| {
        let path = path.replace('/', "\\").to_lowercase();
        format!("{}\\", path.trim_end_matches('\\'))
    });
    let mut candidates = Vec::new();
    for shortcut in index.get(name)? {
        let Some((Some(target), icon)) = crate::manage::presentation::shortcut(shortcut) else {
            continue;
        };
        let path = Path::new(&target);
        let target_key = target.replace('/', "\\").to_lowercase();
        let belongs = match &prefix {
            Some(prefix) => target_key.starts_with(prefix),
            None => no_install_dir && name.chars().count() >= 3 && executable_matches(path, name),
        };
        if !belongs || !is_app_executable(path) || pin_local_scan_path(path).is_err() {
            continue;
        }
        candidates.push((target_key, target, icon));
    }
    let key = &candidates.first()?.0;
    if candidates.iter().any(|entry| &entry.0 != key) {
        return None;
    }
    candidates.into_iter().find_map(|(_, target, icon)| {
        icon.and_then(|raw| file_icon(&raw))
            .or_else(|| file_icon(&target))
    })
}

fn executable_matches(path: &Path, name: &str) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| normalize_for_match(stem) == name)
}

fn is_app_executable(path: &Path) -> bool {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        return false;
    }
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let stem = normalize_for_match(stem);
    ![
        "uninst",
        "uninstall",
        "setup",
        "updater",
        "update",
        "installer",
        "msiexec",
    ]
    .iter()
    .any(|word| stem.starts_with(word) || stem.ends_with(word))
}

fn main_executable_icon(name: &str, root: &Path) -> Option<Icon> {
    // Match the registered product name inside its own install directory.
    // Shared directories can contain many other executables.
    let mut entries = std::fs::read_dir(root).ok()?;
    let matching = unique_main_executable(
        entries
            .by_ref()
            .take(1024)
            .flatten()
            .map(|entry| entry.path()),
        name,
    )?;
    // An incomplete enumeration cannot establish uniqueness.
    if entries.next().is_some() {
        return None;
    }
    file_icon(matching.to_str()?)
}

fn unique_main_executable(paths: impl IntoIterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    let mut matches = paths
        .into_iter()
        .filter(|path| is_app_executable(path) && executable_matches(path, name));
    let path = matches.next()?;
    matches.next().is_none().then_some(path)
}

fn store_icon(package_full_name: &str, name: &str) -> Option<Icon> {
    use windows::Foundation::Size;
    use windows::Management::Deployment::PackageManager;
    use windows::Storage::Streams::{DataReader, RandomAccessStreamReference};
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

    struct WinRt(bool);
    impl Drop for WinRt {
        fn drop(&mut self) {
            if self.0 {
                unsafe { RoUninitialize() };
            }
        }
    }
    let init = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    if init
        .as_ref()
        .is_err_and(|err| err.code() != RPC_E_CHANGED_MODE)
    {
        return None;
    }
    let _winrt = WinRt(init.is_ok());
    let package = PackageManager::new()
        .ok()?
        .FindPackageByUserSecurityIdPackageFullName(
            &HSTRING::new(),
            &HSTRING::from(package_full_name),
        )
        .ok()?;
    let size = Size {
        Width: 64.0,
        Height: 64.0,
    };
    let read_logo = |reference: RandomAccessStreamReference| {
        let stream = reference.OpenReadAsync().ok()?.get().ok()?;
        let size = usize::try_from(stream.Size().ok()?).ok()?;
        if size == 0 || size > MAX_IMAGE_BYTES {
            return None;
        }
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).ok()?).ok()?;
        if reader.LoadAsync(size as u32).ok()?.get().ok()? != size as u32 {
            return None;
        }
        let mut bytes = vec![0; size];
        reader.ReadBytes(&mut bytes).ok()?;
        raster_icon(bytes)
    };
    let entries = package
        .GetAppListEntries()
        .or_else(|_| package.GetAppListEntriesAsync()?.get());
    if let Ok(entries) = entries {
        let mut logos = entries
            .into_iter()
            .filter_map(|entry| entry.DisplayInfo().ok());
        if let Some(first) = logos.next() {
            let is_match = |info: &windows::ApplicationModel::AppDisplayInfo| {
                info.DisplayName()
                    .is_ok_and(|label| label.to_string().eq_ignore_ascii_case(name))
            };
            let selected = match logos.next() {
                None => Some(first),
                Some(second) => [first, second].into_iter().chain(logos).find(is_match),
            };
            if let Some(icon) = selected
                .and_then(|info| info.GetLogo(size).ok())
                .and_then(read_logo)
            {
                return Some(icon);
            }
        }
    }
    package
        .GetLogoAsRandomAccessStreamReference(size)
        .ok()
        .and_then(read_logo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_only_a_unique_matching_main_executable() {
        let files = [
            "CC Switch.exe",
            "Update.exe",
            "uninstall.exe",
            "Bonjour.exe",
        ]
        .map(PathBuf::from);
        assert_eq!(
            unique_main_executable(files.clone(), "ccswitch"),
            Some(PathBuf::from("CC Switch.exe"))
        );
        assert!(unique_main_executable(files.clone(), "update").is_none());
        assert!(unique_main_executable(files, "uninstall").is_none());
        assert!(unique_main_executable(
            ["CC Switch.exe", "cc-switch.exe"].map(PathBuf::from),
            "ccswitch"
        )
        .is_none());
    }

    #[test]
    fn matches_start_menu_names_when_uninstall_labels_include_architecture_or_version() {
        assert_eq!(
            product_name_for_icon("Adobe Acrobat (64-bit)", "25.1"),
            "Adobe Acrobat"
        );
        assert_eq!(
            product_name_for_icon("Example Tool 5.2 (ARM64)", "5.2"),
            "Example Tool"
        );
        assert_eq!(
            product_name_for_icon("Example Tool 5.2", "6.0"),
            "Example Tool 5.2"
        );
    }

    #[test]
    fn rejects_oversized_and_non_image_payloads() {
        assert!(raster_icon(b"<svg></svg>".to_vec()).is_none());
        assert!(raster_icon(vec![0; MAX_IMAGE_BYTES + 1]).is_none());
    }

    #[test]
    #[ignore = "reads the current Windows user's installed app inventory"]
    fn inspect_installed_icons() {
        let apps = crate::apps::scan_installed_apps();
        let mut total = [0, 0];
        let mut previous = [0, 0];
        let mut recovered = [0, 0];
        let mut lost = [0, 0];
        for app in &apps {
            let store = usize::from(app.source == "Store");
            total[store] += 1;
            let original = extract_icon_png(&app.display_icon, false).is_some();
            let found = installed_icon(&crate::apps::app_key(app));
            previous[store] += usize::from(original);
            recovered[store] += usize::from(found.is_some());
            lost[store] += usize::from(original && found.is_none());
            if [
                "CC Switch",
                "ChatGPT",
                "Adobe Acrobat (64-bit)",
                "Bandizip",
                "Clash Verge",
                "Bonjour",
                "AppUp.IntelGraphicsExperience",
                "Clipchamp.Clipchamp",
                "Claude",
            ]
            .contains(&app.name.as_str())
            {
                println!(
                    "{} ({}): original={}, now={:?}",
                    app.name,
                    app.source,
                    original,
                    found.as_ref().map(|(mime, bytes)| (*mime, bytes.len()))
                );
            }
        }
        println!(
            "Desktop and Store counts: total={total:?} previous={previous:?} current={recovered:?} lost={lost:?}"
        );
    }
}
