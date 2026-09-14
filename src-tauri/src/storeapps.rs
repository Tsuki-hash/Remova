//! Enumerate Microsoft Store / MSIX packages via WinRT PackageManager.

use crate::apps::InstalledApp;

/// Critical / framework package name prefixes that must never be offered for uninstall.
const BLOCKED_PREFIXES: &[&str] = &[
    "Microsoft.Windows.",
    "Microsoft.VCLibs.",
    "Microsoft.NET.",
    "Microsoft.UI.Xaml.",
    "Microsoft.Services.Store.Engagement",
    "Microsoft.WindowsStore",
    "Microsoft.DesktopAppInstaller",
    "Microsoft.WinAppRuntime",
    "Microsoft.WindowsAppRuntime",
    "MicrosoftCorporationII.WinAppRuntime",
    "Windows.",
];

fn is_blocked_package(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return true;
    }
    if BLOCKED_PREFIXES.iter().any(|p| n.starts_with(p)) {
        return true;
    }
    // System apps often use GUID-like package names.
    if n.len() == 36 && n.chars().filter(|c| *c == '-').count() == 4 {
        return true;
    }
    if n.starts_with("Microsoft.WinAppRuntime") || n.contains("WinAppRuntime") {
        return true;
    }
    false
}

fn is_system_install_path(path: &str) -> bool {
    let p = path.replace('/', "\\").to_lowercase();
    p.contains("\\windows\\systemapps\\") || p.contains("\\windows\\immersivecontrolpanel")
}

#[cfg(windows)]
pub fn scan_store_apps() -> Vec<InstalledApp> {
    use windows::Management::Deployment::PackageManager;

    // Console / test processes may lack WinRT init; Tauri already has it.
    unsafe {
        use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    let mut out = Vec::new();
    let Ok(pm) = PackageManager::new() else {
        return out;
    };
    // Empty user SID = current user (FindPackages() for all users needs admin).
    let packages = match pm.FindPackagesByUserSecurityId(&windows::core::HSTRING::new()) {
        Ok(p) => p,
        Err(_) => return out,
    };

    for pkg in packages {
        let Ok(id) = pkg.Id() else { continue };
        let Ok(name_h) = id.Name() else { continue };
        let name = name_h.to_string();
        if is_blocked_package(&name) {
            continue;
        }
        // Skip resource / framework packages when API exposes it
        if pkg.IsFramework().unwrap_or(false) {
            continue;
        }
        if pkg.IsResourcePackage().unwrap_or(false) {
            continue;
        }
        // Skip non-stable / bundle noise when possible
        let full = id.FullName().map(|s| s.to_string()).unwrap_or_default();
        if full.is_empty() {
            continue;
        }
        let publisher = id.Publisher().map(|s| s.to_string()).unwrap_or_default();
        let version = id
            .Version()
            .map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision))
            .unwrap_or_default();
        let install_location = pkg
            .InstalledLocation()
            .ok()
            .and_then(|loc| loc.Path().ok())
            .map(|p| p.to_string())
            .unwrap_or_default();
        if is_system_install_path(&install_location) {
            continue;
        }
        let install_date = pkg.InstalledDate().ok().and_then(|dt| {
            let unix_100ns = dt.UniversalTime as i128 - 116_444_736_000_000_000i128;
            if unix_100ns <= 0 {
                return None;
            }
            let secs = (unix_100ns / 10_000_000) as i64;
            let days = secs.div_euclid(86_400);
            Some(civil_from_days(days))
        });

        let uninstall_string = format!("remova-store:{full}");

        out.push(InstalledApp {
            name,
            version,
            publisher,
            install_location,
            uninstall_string: uninstall_string.clone(),
            quiet_uninstall_string: uninstall_string,
            source: "Store".into(),
            registry_key: format!("Store\\{full}"),
            estimated_size_kb: 0,
            install_date: install_date.unwrap_or_default(),
            display_icon: String::new(),
        });
    }
    out
}

#[cfg(not(windows))]
pub fn scan_store_apps() -> Vec<InstalledApp> {
    Vec::new()
}

/// Convert days since Unix epoch to `YYYY-MM-DD`.
fn civil_from_days(days: i64) -> String {
    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn blocked_prefixes() {
        assert!(super::is_blocked_package("Microsoft.WindowsStore"));
        assert!(super::is_blocked_package("Microsoft.Windows.ShellExperienceHost"));
        assert!(super::is_blocked_package("Microsoft.VCLibs.140.00"));
        assert!(super::is_blocked_package(
            "1527c705-839a-4832-9118-54d4Bd6a0c89"
        ));
        assert!(!super::is_blocked_package("Microsoft.WindowsCalculator"));
        assert!(!super::is_blocked_package("SpotifyAB.SpotifyMusic"));
        assert!(super::is_system_install_path(
            r"C:\Windows\SystemApps\Microsoft.Foo_cw5n1h2txyewy"
        ));
        assert!(!super::is_system_install_path(
            r"C:\Program Files\WindowsApps\SpotifyAB.SpotifyMusic_1.2.3.0_x64__ydafd2b3q0z0t"
        ));
    }

    #[test]
    fn civil_date() {
        // 1970-01-01 → 0
        assert_eq!(super::civil_from_days(0), "1970-01-01");
        // 2026-09-11 ≈ 20677 days (approximate: 2026-01-01 is 20454)
        let s = super::civil_from_days(20677);
        assert!(s.starts_with("2026-"), "got {s}");
    }

    #[cfg(windows)]
    #[test]
    fn scan_store_runs() {
        let apps = super::scan_store_apps();
        for a in &apps {
            assert_eq!(a.source, "Store");
            assert!(!a.registry_key.is_empty());
            assert!(!super::is_system_install_path(&a.install_location));
            assert!(!super::is_blocked_package(&a.name));
        }
    }
}
