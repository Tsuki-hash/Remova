//! Update check: GitHub latest release resolved on the Rust side to avoid WebView CORS/CSP issues.

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
            .map_err(|e| format!("GitHub 请求失败: {e}"))?;
        let body = resp
            .into_string()
            .map_err(|e| format!("读取响应失败: {e}"))?;
        let v: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| format!("解析 JSON 失败: {e}"))?;
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
        if let Some(assets) = v.get("assets").and_then(|a| a.as_array()) {
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
    .map_err(|e| e.to_string())?
}
