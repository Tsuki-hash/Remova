//! Update check: GitHub latest release resolved on the Rust side to avoid WebView CORS/CSP issues.

const INSTALL_CHANNEL: &str = "nsis-current-user-v1";

fn is_nsis_install(dir: &std::path::Path) -> bool {
    dir.join("uninstall.exe").is_file()
        && std::fs::read_to_string(dir.join(".remova-install-channel"))
            .is_ok_and(|s| s.trim() == INSTALL_CHANNEL)
}

/// Never run an NSIS updater for MSI, portable, development, or elevated launches.
#[tauri::command]
pub fn online_update_supported(app: tauri::AppHandle) -> bool {
    let configured = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|v| v.get("pubkey"))
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    configured
        && !crate::is_elevated().unwrap_or(true)
        && std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(is_nsis_install))
            .unwrap_or(false)
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
                if name.ends_with(".exe") && name.contains("setup") && !link.is_empty() {
                    download_url = Some(link.to_string());
                    break;
                }
            }
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
