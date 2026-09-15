//! AI orchestration: config, sanitize, OpenAI-compatible chat, cache.
//! AI never deletes; callers treat all output as advisory.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

const CACHE_TTL_SECS: u64 = 7 * 24 * 3600;
const HTTP_TIMEOUT_SECS: u64 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub enabled: bool,
    /// "openai" | "ollama"
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    /// Allow sending desensitized install paths (default false → path only as vendor/product tokens).
    #[serde(default)]
    pub allow_cloud_paths: bool,
}

fn default_provider() -> String {
    "openai".into()
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_provider(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            model: "gpt-4o-mini".into(),
            allow_cloud_paths: false,
        }
    }
}

/// Config view for UI — never returns the raw API key.
#[derive(Debug, Clone, Serialize)]
pub struct AiConfigView {
    pub enabled: bool,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub allow_cloud_paths: bool,
    pub has_api_key: bool,
}

impl From<&AiConfig> for AiConfigView {
    fn from(c: &AiConfig) -> Self {
        Self {
            enabled: c.enabled,
            provider: c.provider.clone(),
            base_url: c.base_url.clone(),
            model: c.model.clone(),
            allow_cloud_paths: c.allow_cloud_paths,
            has_api_key: !c.api_key.trim().is_empty(),
        }
    }
}

fn config_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| r"C:\Users\Public".into());
    PathBuf::from(base).join("Remova").join("ai-config.json")
}

pub fn load_config() -> AiConfig {
    let p = config_path();
    let Ok(s) = std::fs::read_to_string(p) else {
        return AiConfig::default();
    };
    serde_json::from_str(&s).unwrap_or_default()
}

pub fn save_config(c: &AiConfig) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(c).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| e.to_string())
}

/// Reduce a path to vendor/product-ish tokens; drop usernames and deep trees.
pub fn sanitize_path(path: &str, allow_full: bool) -> String {
    let p = path.replace('/', "\\");
    let low = p.to_lowercase();
    if allow_full {
        if let Some(idx) = low.find("\\users\\") {
            let rest = &p[idx + 7..];
            if let Some(slash) = rest.find('\\') {
                return format!("C:\\Users\\*\\{}", &rest[slash + 1..]);
            }
            return "C:\\Users\\*".into();
        }
        return p;
    }
    for marker in [
        "\\appdata\\local\\",
        "\\appdata\\locallow\\",
        "\\appdata\\roaming\\",
    ] {
        if let Some(i) = low.find(marker) {
            let after = &p[i + marker.len()..];
            let take: Vec<&str> = after
                .split('\\')
                .filter(|s| !s.is_empty())
                .take(2)
                .collect();
            if !take.is_empty() {
                return format!("AppData\\{}", take.join("\\"));
            }
        }
    }
    for marker in ["\\program files\\", "\\program files (x86)\\"] {
        if let Some(i) = low.find(marker) {
            let after = &p[i + marker.len()..];
            if let Some(first) = after.split('\\').find(|s| !s.is_empty()) {
                return first.to_string();
            }
        }
    }
    let parts: Vec<&str> = p.split('\\').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        format!("{}\\{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else if let Some(last) = parts.last() {
        (*last).to_string()
    } else {
        "…".into()
    }
}

fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

struct CacheEntry {
    value: String,
    at: u64,
}

static CACHE: Mutex<Option<HashMap<u64, CacheEntry>>> = Mutex::new(None);

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_get(key: u64) -> Option<String> {
    let mut g = CACHE.lock().ok()?;
    let map = g.get_or_insert_with(HashMap::new);
    let now = now_secs();
    map.retain(|_, e| now.saturating_sub(e.at) < CACHE_TTL_SECS);
    map.get(&key).map(|e| e.value.clone())
}

fn cache_put(key: u64, value: String) {
    if let Ok(mut g) = CACHE.lock() {
        let map = g.get_or_insert_with(HashMap::new);
        map.insert(
            key,
            CacheEntry {
                value,
                at: now_secs(),
            },
        );
        if map.len() > 400 {
            map.clear();
        }
    }
}

fn normalize_base_url(u: &str) -> String {
    let u = u.trim().trim_end_matches('/').to_string();
    if u.is_empty() {
        return "https://api.openai.com/v1".into();
    }
    u
}

/// Call OpenAI-compatible `/chat/completions`. Ollama uses same shape with base_url `http://127.0.0.1:11434/v1`.
fn chat_completion(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    if !cfg.enabled {
        return Err("ai disabled".into());
    }
    if cfg.model.trim().is_empty() {
        return Err("ai model empty".into());
    }
    let needs_key = cfg.provider != "ollama" && !cfg.base_url.contains("127.0.0.1")
        && !cfg.base_url.contains("localhost");
    if needs_key && cfg.api_key.trim().is_empty() {
        return Err("ai api key empty".into());
    }

    let base = normalize_base_url(&cfg.base_url);
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": cfg.model.trim(),
        "temperature": 0.2,
        "max_tokens": 400,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    });

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build();

    let mut req = agent.post(&url).set("Content-Type", "application/json");
    if !cfg.api_key.trim().is_empty() {
        req = req.set("Authorization", &format!("Bearer {}", cfg.api_key.trim()));
    }

    let resp = req
        .send_json(body)
        .map_err(|e| format!("ai http: {e}"))?;

    let v: serde_json::Value = resp.into_json().map_err(|e| format!("ai parse: {e}"))?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("ai empty response".into());
    }
    Ok(text)
}

// ── Explain items ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainInput {
    pub path: String,
    pub kind: String,
    pub confidence: String,
    pub risk: String,
    pub reason: String,
    pub evidence_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainOutput {
    pub path: String,
    pub summary: String,
    pub suggest_check: bool,
}

const EXPLAIN_SYSTEM: &str = "你是 Windows 深度卸载助手。根据给定的残留候选证据，用中文写 1-2 句说明：\
这是什么、为何像残留、删除的常见影响。不要编造路径外的事实。\
suggest_check 仅当 confidence=confirmed 且 risk!=high 时可为 true。\
严格输出 JSON 数组，元素字段：path, summary, suggest_check。";

pub fn explain_items(
    cfg: &AiConfig,
    app_name: &str,
    publisher: &str,
    items: &[ExplainInput],
) -> Result<Vec<ExplainOutput>, String> {
    if items.is_empty() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    let mut pending: Vec<ExplainInput> = Vec::new();

    for it in items {
        let key = fnv1a64(&format!(
            "{app_name}|{}|{}|{}|{}",
            it.path, it.kind, it.confidence, it.reason
        ));
        if let Some(cached) = cache_get(key) {
            if let Ok(parsed) = serde_json::from_str::<ExplainOutput>(&cached) {
                out.push(parsed);
                continue;
            }
        }
        pending.push(it.clone());
        // keep key map parallel via path in output later
        let _ = key;
    }

    if pending.is_empty() {
        return Ok(out);
    }

    // Cap batch size for cost control
    let batch: Vec<&ExplainInput> = pending.iter().take(12).collect();
    let payload: Vec<serde_json::Value> = batch
        .iter()
        .map(|it| {
            serde_json::json!({
                "path": sanitize_path(&it.path, cfg.allow_cloud_paths),
                "kind": it.kind,
                "confidence": it.confidence,
                "risk": it.risk,
                "reason": it.reason,
                "evidence": it.evidence_labels,
            })
        })
        .collect();
    let user = format!(
        "软件: {}\n发布者: {}\n候选(脱敏路径):\n{}\n请输出 JSON 数组。",
        app_name,
        publisher,
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    );

    let text = chat_completion(cfg, EXPLAIN_SYSTEM, &user)?;
    let cleaned = strip_code_fence(&text);
    let parsed: Vec<ExplainOutput> =
        serde_json::from_str(&cleaned).map_err(|e| format!("ai json: {e} | {cleaned}"))?;

    // Map back to original paths by order / sanitized match
    for (i, p) in parsed.iter().enumerate() {
        let orig = batch.get(i).map(|x| x.path.clone()).unwrap_or_else(|| p.path.clone());
        let item = ExplainOutput {
            path: orig,
            summary: p.summary.clone(),
            suggest_check: p.suggest_check && batch.get(i).map(|b| b.risk.as_str()) != Some("high"),
        };
        let key = fnv1a64(&format!(
            "{app_name}|{}|{}|{}|{}",
            item.path,
            batch.get(i).map(|x| x.kind.as_str()).unwrap_or(""),
            batch.get(i).map(|x| x.confidence.as_str()).unwrap_or(""),
            batch.get(i).map(|x| x.reason.as_str()).unwrap_or(""),
        ));
        if let Ok(s) = serde_json::to_string(&item) {
            cache_put(key, s);
        }
        out.push(item);
    }
    Ok(out)
}

// ── Risk brief ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskBriefInput {
    pub app_name: String,
    pub publisher: String,
    pub action: String,
    pub item_count: usize,
    pub kind_counts: HashMap<String, usize>,
    pub has_service: bool,
    pub has_run_key: bool,
    pub has_shared_hint: bool,
}

const RISK_SYSTEM: &str = "你是 Windows 卸载风险说明助手。用中文写 2-3 句：这次操作做什么、可能影响什么、\
哪些项默认不会删。语气克制、可执行。不要输出 Markdown 标题。";

const REPORT_SYSTEM: &str = "你是 Windows 卸载报告解读助手。用中文写 3-5 句：删了什么、失败怎么办、\
是否建议重启、能否从备份还原。语气克制。不要输出 Markdown 标题。";

pub fn risk_brief(cfg: &AiConfig, input: &RiskBriefInput) -> Result<String, String> {
    let key = fnv1a64(&format!(
        "risk|{}|{}|{}|{}|{}|{}|{}",
        input.app_name,
        input.action,
        input.item_count,
        input.has_service,
        input.has_run_key,
        input.has_shared_hint,
        serde_json::to_string(&input.kind_counts).unwrap_or_default()
    ));
    if let Some(c) = cache_get(key) {
        return Ok(c);
    }
    let user = format!(
        "软件: {} ({})\n动作: {}\n候选项: {}\n类型分布: {:?}\n含服务: {}\n含启动项: {}\n疑似共享组件: {}",
        input.app_name,
        input.publisher,
        input.action,
        input.item_count,
        input.kind_counts,
        input.has_service,
        input.has_run_key,
        input.has_shared_hint
    );
    let text = chat_completion(cfg, RISK_SYSTEM, &user)?;
    cache_put(key, text.clone());
    Ok(text)
}

fn strip_code_fence(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportBriefInput {
    pub app_name: String,
    pub deleted: usize,
    pub failed: usize,
    pub skipped: usize,
    pub aborted: bool,
    pub backup_dir: String,
    pub restore_point_ok: bool,
    pub top_failed: Vec<String>,
}

pub fn summarize_report(cfg: &AiConfig, input: &ReportBriefInput) -> Result<String, String> {
    let key = fnv1a64(&format!(
        "report|{}|{}|{}|{}|{}|{}",
        input.app_name,
        input.deleted,
        input.failed,
        input.skipped,
        input.aborted,
        input.restore_point_ok
    ));
    if let Some(c) = cache_get(key) {
        return Ok(c);
    }
    let user = format!(
        "软件: {}\n删除: {}\n失败: {}\n跳过: {}\n中止: {}\n备份目录: {}\n还原点: {}\n失败样例: {}",
        input.app_name,
        input.deleted,
        input.failed,
        input.skipped,
        input.aborted,
        sanitize_path(&input.backup_dir, true),
        input.restore_point_ok,
        input.top_failed.join(" | ")
    );
    let text = chat_completion(cfg, REPORT_SYSTEM, &user)?;
    cache_put(key, text.clone());
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_username() {
        let p = r"C:\Users\alice\AppData\Local\Foo\App\bin";
        let s = sanitize_path(p, false);
        assert!(s.contains("Foo"), "{s}");
        assert!(!s.contains("alice"), "{s}");
    }

    #[test]
    fn sanitize_full_keeps_depth_masks_user() {
        let p = r"C:\Users\alice\AppData\Local\Foo\Bar\cache\x.dat";
        let s = sanitize_path(p, true);
        assert!(s.contains("Users\\*\\") || s.contains("Users\\*\\"), "{s}");
        assert!(!s.contains("alice"), "{s}");
    }

    #[test]
    fn config_view_hides_key() {
        let mut c = AiConfig::default();
        c.api_key = "sk-secret".into();
        let v = AiConfigView::from(&c);
        assert!(v.has_api_key);
        assert!(!serde_json::to_string(&v).unwrap().contains("sk-secret"));
    }
}
