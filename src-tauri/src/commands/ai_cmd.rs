//! AI command layer. AI only explains; it never deletes.

use crate::ai;

#[tauri::command]
pub fn get_ai_config() -> Result<ai::AiConfigView, String> {
    Ok(ai::AiConfigView::from(&ai::load_config()?))
}

/// Save AI settings. An omitted key keeps the stored key only when the
/// provider and normalized endpoint remain the same. Changing either source
/// requires a new key or an explicit empty value to clear it.
#[tauri::command]
pub fn save_ai_config(
    enabled: bool,
    provider: String,
    base_url: String,
    model: String,
    allow_cloud_paths: bool,
    api_key: Option<String>,
) -> Result<ai::AiConfigView, String> {
    let api_key = api_key.map(zeroize::Zeroizing::new);
    ai::update_config(move |c| {
        ai::replace_credential(c, &provider, &base_url, api_key);
        c.enabled = enabled;
        c.provider = provider;
        c.base_url = base_url;
        c.model = model;
        c.allow_cloud_paths = allow_cloud_paths;
    })
}

#[tauri::command]
pub async fn ai_risk_brief(
    request: ai::RiskBriefInput,
    locale: Option<String>,
) -> Result<Option<String>, String> {
    let cfg = ai::load_config()?;
    if !cfg.enabled {
        return Ok(None);
    }
    let locale = locale.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || ai::risk_brief(&cfg, &request, &locale))
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
    locale: Option<String>,
) -> Result<Vec<ai::ExplainOutput>, String> {
    let cfg = ai::load_config()?;
    if !cfg.enabled {
        return Ok(vec![]);
    }
    let locale = locale.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        ai::explain_items(&cfg, &app_name, &publisher, &items, &locale)
    })
    .await
    .map_err(|e| e.to_string())?
    // stable code + underlying reason (see ai_risk_brief).
    .map_err(|e| format!("ai:explain_failed::{e}"))
}

#[tauri::command]
pub async fn ai_summarize_report(
    request: ai::ReportBriefInput,
    locale: Option<String>,
) -> Result<Option<String>, String> {
    let cfg = ai::load_config()?;
    if !cfg.enabled {
        return Ok(None);
    }
    let locale = locale.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || ai::summarize_report(&cfg, &request, &locale))
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
        // stable code + underlying reason (see ai_risk_brief).
        .map_err(|e| format!("ai:summarize_failed::{e}"))
}

#[tauri::command]
pub async fn ai_parse_intent(
    text: String,
    app_names: Vec<String>,
    locale: Option<String>,
) -> Result<ai::NlIntent, String> {
    let cfg = ai::load_config()?;
    if !cfg.enabled {
        return Err("ai:parse_intent_failed::ai disabled".into());
    }
    let locale = locale.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        ai::parse_nl_intent(&cfg, &text, &app_names, &locale)
    })
    .await
    .map_err(|e| format!("ai:parse_intent_failed::{e}"))?
    .map_err(|e| format!("ai:parse_intent_failed::{e}"))
}
