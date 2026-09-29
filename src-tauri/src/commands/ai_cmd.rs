//! AI command layer. AI only explains; it never deletes.

use crate::ai;
use zeroize::Zeroize;

#[tauri::command]
pub fn get_ai_config() -> Result<ai::AiConfigView, String> {
    Ok(ai::AiConfigView::from(&ai::load_config()))
}

/// Save AI settings. Empty `api_key` keeps the stored key unchanged.
#[tauri::command]
pub fn save_ai_config(
    enabled: bool,
    provider: String,
    base_url: String,
    model: String,
    allow_cloud_paths: bool,
    api_key: Option<String>,
) -> Result<ai::AiConfigView, String> {
    let mut c = ai::load_config();
    c.enabled = enabled;
    c.provider = provider;
    c.base_url = base_url;
    c.model = model;
    c.allow_cloud_paths = allow_cloud_paths;
    if let Some(mut k) = api_key {
        let trimmed = k.trim().to_string();
        // The IPC copy (possibly padded) holds the secret too — wipe it.
        k.zeroize();
        if !trimmed.is_empty() {
            // assignment would drop the old decrypted key without wipe.
            c.api_key.zeroize();
            c.api_key = trimmed;
        }
    }
    ai::save_config(&c)?;
    Ok(ai::AiConfigView::from(&c))
}

#[tauri::command]
pub async fn ai_risk_brief(request: ai::RiskBriefInput) -> Result<Option<String>, String> {
    let cfg = ai::load_config();
    if !cfg.enabled {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || ai::risk_brief(&cfg, &request))
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
        // keep the stable code, pass the underlying reason through —
        // `code::detail` (formatError maps the code, the tail carries the cause).
        .map_err(|e| format!("ai:risk_brief_failed::{e}"))
}

#[tauri::command]
pub async fn ai_explain_items(
    app_name: String,
    publisher: String,
    items: Vec<ai::ExplainInput>,
) -> Result<Vec<ai::ExplainOutput>, String> {
    let cfg = ai::load_config();
    if !cfg.enabled {
        return Ok(vec![]);
    }
    tauri::async_runtime::spawn_blocking(move || {
        ai::explain_items(&cfg, &app_name, &publisher, &items)
    })
    .await
    .map_err(|e| e.to_string())?
    // stable code + underlying reason (see ai_risk_brief).
    .map_err(|e| format!("ai:explain_failed::{e}"))
}

#[tauri::command]
pub async fn ai_summarize_report(request: ai::ReportBriefInput) -> Result<Option<String>, String> {
    let cfg = ai::load_config();
    if !cfg.enabled {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || ai::summarize_report(&cfg, &request))
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
        // stable code + underlying reason (see ai_risk_brief).
        .map_err(|e| format!("ai:summarize_failed::{e}"))
}

#[tauri::command]
pub async fn ai_parse_intent(text: String, app_names: Vec<String>) -> Result<ai::NlIntent, String> {
    let cfg = ai::load_config();
    if !cfg.enabled {
        return Err("ai:parse_intent_failed::ai disabled".into());
    }
    tauri::async_runtime::spawn_blocking(move || ai::parse_nl_intent(&cfg, &text, &app_names))
        .await
        .map_err(|e| format!("ai:parse_intent_failed::{e}"))?
        .map_err(|e| format!("ai:parse_intent_failed::{e}"))
}
