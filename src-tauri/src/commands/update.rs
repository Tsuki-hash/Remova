//! Update check: GitHub latest release resolved on the Rust side to avoid WebView CORS/CSP issues.

use std::io::Read;

const INSTALL_CHANNEL: &str = "nsis-current-user-v1";

const NSIS_FIRSTHEADER_MAGIC: &[u8] = b"NullsoftInst";

/// Misidentification guard for the in-app updater: an NSIS install carries a
/// real NSIS uninstaller (firstheader magic near the file head) plus our
/// channel marker. This is NOT the trust boundary — updater payloads are
/// signature-verified fail-closed — it only keeps portable/dev/elevated
/// layouts from enabling the NSIS updater flow.
fn is_nsis_install(dir: &std::path::Path) -> bool {
    let uninstaller = dir.join("uninstall.exe");
    if !uninstaller.is_file() {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(&uninstaller) else {
        return false;
    };
    let mut head = vec![0u8; 65_536];
    let n = f.read(&mut head).unwrap_or(0);
    head.truncate(n);
    if !head
        .windows(NSIS_FIRSTHEADER_MAGIC.len())
        .any(|w| w == NSIS_FIRSTHEADER_MAGIC)
    {
        return false;
    }
    std::fs::read_to_string(dir.join(".remova-install-channel"))
        .is_ok_and(|s| s.trim() == INSTALL_CHANNEL)
}

/// Pure core of `online_update_supported` (fail-closed on every input).
fn update_support_decision(
    pubkey: Option<&str>,
    elevated: bool,
    exe_dir: Option<&std::path::Path>,
) -> bool {
    let configured = pubkey.is_some_and(|s| !s.trim().is_empty());
    configured && !elevated && exe_dir.map(is_nsis_install).unwrap_or(false)
}

/// Never run an NSIS updater for MSI, portable, development, or elevated launches.
#[tauri::command]
pub fn online_update_supported(app: tauri::AppHandle) -> bool {
    let pubkey = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|v| v.get("pubkey"))
        .and_then(|v| v.as_str());
    let elevated = crate::is_elevated().unwrap_or(true);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    update_support_decision(pubkey, elevated, exe_dir.as_deref())
}

/// Pick the NSIS setup asset for this build: the download must be anchored to
/// this architecture and release version, never just "any setup.exe".
fn pick_setup_asset(assets: &[serde_json::Value], version: &str) -> Option<String> {
    if version.is_empty() {
        return None;
    }
    let version_low = version.to_lowercase();
    for a in assets {
        let name = a
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_lowercase();
        let link = a
            .get("browser_download_url")
            .and_then(|n| n.as_str())
            .unwrap_or("");
        if link.is_empty() || !name.ends_with(".exe") || !name.contains("setup") {
            continue;
        }
        let arch_ok = name.contains("x86_64") || name.contains("x64");
        if arch_ok && name.contains(&version_low) {
            return Some(link.to_string());
        }
    }
    None
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LatestReleaseInfo {
    pub version: String,
    pub url: String,
    pub download_url: Option<String>,
}

/// Fetch GitHub latest release from the Rust side (avoids WebView CORS / CSP issues).
#[tauri::command]
pub async fn check_github_latest() -> Result<Option<LatestReleaseInfo>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(8))
            .build();
        let resp = agent
            .get(crate::constants::GITHUB_LATEST_RELEASE_API)
            .set("Accept", "application/vnd.github+json")
            .set("User-Agent", "Remova")
            .call()
            .map_err(|_| "update:http_failed".to_string())?;
        let body = resp
            .into_string()
            .map_err(|_| "update:read_body_failed".to_string())?;
        let v: serde_json::Value =
            serde_json::from_str(&body).map_err(|_| "update:parse_failed".to_string())?;
        let tag = v
            .get("tag_name")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string();
        if tag.is_empty() {
            return Ok(None);
        }
        let url = v
            .get("html_url")
            .and_then(|t| t.as_str())
            .unwrap_or(crate::constants::GITHUB_RELEASES_PAGE)
            .to_string();
        let mut download_url = None;
        // MSI and portable users choose the matching package on the release page.
        let nsis = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(is_nsis_install))
            .unwrap_or(false);
        if let Some(assets) = v.get("assets").and_then(|a| a.as_array()).filter(|_| nsis) {
            download_url = pick_setup_asset(assets, &tag);
        }
        Ok(Some(LatestReleaseInfo {
            version: tag,
            url,
            download_url,
        }))
    })
    .await
    .map_err(|_| "update:task_failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, link: &str) -> serde_json::Value {
        serde_json::json!({ "name": name, "browser_download_url": link })
    }

    #[test]
    fn pick_setup_asset_anchors_arch_and_version() {
        let assets = vec![
            asset("Remova-setup.exe", "https://x/1"), // no arch/version
            asset("Remova_1.2.0_x64-setup.exe", "https://x/2"), // wrong version
            asset("Remova_1.3.0_arm64-setup.exe", "https://x/3"), // wrong arch
            asset("Remova_1.3.0_x64-setup.exe", "https://x/4"),
            asset("Remova_1.3.0.msi", "https://x/5"), // not the NSIS exe
        ];
        assert_eq!(
            pick_setup_asset(&assets, "1.3.0").as_deref(),
            Some("https://x/4")
        );
        // Unanchored candidates never win by themselves.
        let loose = vec![asset("Remova-setup.exe", "https://x/1")];
        assert_eq!(pick_setup_asset(&loose, "1.3.0"), None);
        // Empty version must never match anything.
        assert_eq!(pick_setup_asset(&assets, ""), None);
    }

    fn temp_nsis_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "remova_upd_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn is_nsis_install_requires_uninstaller_magic_and_channel_marker() {
        let dir = temp_nsis_dir("nsis");
        // No uninstaller at all.
        assert!(!is_nsis_install(&dir));
        // Uninstaller without the NSIS firstheader magic.
        std::fs::write(dir.join("uninstall.exe"), b"MZ fake binary").unwrap();
        assert!(!is_nsis_install(&dir));
        // Magic but wrong channel marker.
        let mut stub = b"MZ".to_vec();
        stub.extend_from_slice(b"....NullsoftInst....");
        std::fs::write(dir.join("uninstall.exe"), &stub).unwrap();
        std::fs::write(dir.join(".remova-install-channel"), "some-other-channel").unwrap();
        assert!(!is_nsis_install(&dir));
        // Magic + exact channel marker.
        std::fs::write(
            dir.join(".remova-install-channel"),
            format!("{INSTALL_CHANNEL}\n"),
        )
        .unwrap();
        assert!(is_nsis_install(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_support_decision_fails_closed() {
        let dir = temp_nsis_dir("gate");
        let mut stub = b"MZ".to_vec();
        stub.extend_from_slice(b"....NullsoftInst....");
        std::fs::write(dir.join("uninstall.exe"), &stub).unwrap();
        std::fs::write(dir.join(".remova-install-channel"), INSTALL_CHANNEL).unwrap();
        // Missing pubkey.
        assert!(!update_support_decision(None, false, Some(&dir)));
        assert!(!update_support_decision(Some("  "), false, Some(&dir)));
        // Elevated launch.
        assert!(!update_support_decision(Some("key"), true, Some(&dir)));
        // Unknown exe location.
        assert!(!update_support_decision(Some("key"), false, None));
        // All conditions met.
        assert!(update_support_decision(Some("key"), false, Some(&dir)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
