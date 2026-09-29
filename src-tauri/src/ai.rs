//! AI orchestration: config, sanitize, OpenAI-compatible chat, cache.
//! AI never deletes; callers treat all output as advisory.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use zeroize::{Zeroize, ZeroizeOnDrop};

const CACHE_TTL_SECS: u64 = 7 * 24 * 3600;
const HTTP_TIMEOUT_SECS: u64 = 12;

// a config leaving scope wipes its API-key bytes from memory.
// no derived Debug — that would print the raw key into logs.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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

impl std::fmt::Debug for AiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiConfig")
            .field("enabled", &self.enabled)
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    "<empty>"
                } else {
                    "<redacted>"
                },
            )
            .field("model", &self.model)
            .field("allow_cloud_paths", &self.allow_cloud_paths)
            .finish()
    }
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

/// Apply provider preset defaults (base_url + model) when the user switches providers.
pub fn provider_preset(provider: &str) -> (&'static str, &'static str) {
    match provider {
        "anthropic" => ("https://api.anthropic.com/v1", "claude-sonnet-4-5"),
        "ollama" => ("http://127.0.0.1:11434/v1", "llama3.2"),
        _ => ("https://api.openai.com/v1", "gpt-4o-mini"),
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
    // never fall back to C:\Users\Public (shared writable). Prefer per-user TEMP.
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        let temp =
            std::env::var("TEMP").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into());
        format!("{temp}\\Remova-{}", std::process::id())
    });
    PathBuf::from(base).join("Remova").join("ai-config.json")
}

pub fn load_config() -> AiConfig {
    let p = config_path();
    let Ok(s) = std::fs::read_to_string(p) else {
        return AiConfig::default();
    };
    let mut c: AiConfig = serde_json::from_str(&s).unwrap_or_default();
    // take ownership of the stored bytes so the pre-image is wiped
    // explicitly instead of being dropped by the field assignment.
    let mut raw_key = std::mem::take(&mut c.api_key);
    c.api_key = decrypt_stored_key(&raw_key);
    // migrate a legacy plaintext key to DPAPI at rest on first load.
    let migrate = !raw_key.is_empty() && !raw_key.starts_with(KEY_PREFIX);
    // the ciphertext copy leaves no residue either.
    raw_key.zeroize();
    if migrate {
        // The wrap must not silently fail: a swallowed error leaves the
        // plaintext key on disk while the UI reports it protected.
        if let Err(e) = save_config(&c) {
            eprintln!("ai config migration failed: {e}");
            // Scrub the plaintext from disk (best effort, atomic) and from the
            // returned config so `has_api_key` no longer claims protection —
            // the user re-enters the key once DPAPI works again.
            let out = AiConfig {
                enabled: c.enabled,
                provider: c.provider.clone(),
                base_url: c.base_url.clone(),
                api_key: String::new(),
                model: c.model.clone(),
                allow_cloud_paths: c.allow_cloud_paths,
            };
            if let Ok(s) = serde_json::to_string_pretty(&out) {
                let _ = write_config_file(&config_path(), &s);
            }
            let mut key = std::mem::take(&mut c.api_key);
            key.zeroize();
        }
    }
    c
}

/// Atomic config write (tmp + rename): a crash mid-write must not destroy the
/// DPAPI-wrapped key or a half-migrated file. Serialized in-process so a
/// concurrent save/migrate cannot interleave on a shared tmp name.
fn write_config_file(p: &std::path::Path, s: &str) -> Result<(), String> {
    static CONFIG_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = CONFIG_WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut tmp_name = p.as_os_str().to_owned();
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp_name);
    std::fs::write(&tmp, s).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    std::fs::rename(&tmp, p).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

pub fn save_config(c: &AiConfig) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // never clone the plaintext key into `out` — build the write
    // payload without it and store only the ciphertext.
    let mut out = AiConfig {
        enabled: c.enabled,
        provider: c.provider.clone(),
        base_url: c.base_url.clone(),
        api_key: String::new(),
        model: c.model.clone(),
        allow_cloud_paths: c.allow_cloud_paths,
    };
    out.api_key = encrypt_stored_key(&c.api_key)?;
    let s = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
    write_config_file(&p, &s)
}

const KEY_PREFIX: &str = "dpapi:";

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

/// DPAPI-protect the API key at rest (SEC-A1).
/// Returns `Err` instead of silently persisting a plaintext key.
#[cfg(windows)]
fn encrypt_stored_key(key: &str) -> Result<String, String> {
    let raw = key.as_bytes();
    if raw.is_empty() {
        return Ok(String::new());
    }
    if key.starts_with(KEY_PREFIX) {
        return Ok(key.to_string());
    }
    unsafe {
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB as DATA_BLOB,
        };
        let in_blob = DATA_BLOB {
            cbData: raw.len() as u32,
            pbData: raw.as_ptr() as _,
        };
        let mut out_blob = DATA_BLOB::default();
        let ok = CryptProtectData(
            &in_blob,
            windows::core::w!("Remova AI key"),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        );
        if ok.is_err() {
            return Err("ai:encrypt_failed".to_string());
        }
        let enc = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        // CryptProtectData allocates pbData via LocalAlloc ().
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            out_blob.pbData as *mut core::ffi::c_void,
        ));
        Ok(format!("{KEY_PREFIX}{}", to_hex(&enc)))
    }
}

#[cfg(not(windows))]
fn encrypt_stored_key(key: &str) -> Result<String, String> {
    Ok(key.to_string())
}

#[cfg(windows)]
fn decrypt_stored_key(stored: &str) -> String {
    let Some(hex) = stored.strip_prefix(KEY_PREFIX) else {
        return stored.to_string();
    };
    let Some(raw) = from_hex(hex) else {
        return String::new();
    };
    if raw.is_empty() {
        return String::new();
    }
    unsafe {
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB as DATA_BLOB,
        };
        let in_blob = DATA_BLOB {
            cbData: raw.len() as u32,
            pbData: raw.as_ptr() as _,
        };
        let mut out_blob = DATA_BLOB::default();
        let ok = CryptUnprotectData(
            &in_blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        );
        if ok.is_err() {
            return String::new();
        }
        let dec = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        // CryptUnprotectData allocates pbData via LocalAlloc ().
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            out_blob.pbData as *mut core::ffi::c_void,
        ));
        // a failed UTF-8 decode still holds key bytes — wipe them.
        match String::from_utf8(dec) {
            Ok(s) => s,
            Err(e) => {
                let mut bad = e.into_bytes();
                bad.zeroize();
                String::new()
            }
        }
    }
}

#[cfg(not(windows))]
fn decrypt_stored_key(stored: &str) -> String {
    stored.to_string()
}

/// replace the Windows profile-name segment after `Users\` with `*`.
/// ASCII case-insensitive so index alignment with the original string is preserved.
pub fn mask_profile_usernames(s: &str) -> String {
    let sc: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < sc.len() {
        // Path-segment `users` (leading or after a separator).
        let at_users = i + 5 <= sc.len()
            && sc[i..i + 5]
                .iter()
                .zip("users".chars())
                .all(|(a, b)| a.to_ascii_lowercase() == b)
            && (i == 0 || sc[i - 1] == '\\' || sc[i - 1] == '/')
            && (i + 5 >= sc.len() || sc[i + 5] == '\\' || sc[i + 5] == '/');
        if !at_users {
            out.push(sc[i]);
            i += 1;
            continue;
        }
        // Keep the `Users` segment and its separator; mask the next segment.
        out.extend(sc[i..i + 5].iter());
        if i + 5 < sc.len() {
            out.push(sc[i + 5]);
            let mut j = i + 6;
            while j < sc.len() && sc[j] != '\\' && sc[j] != '/' {
                j += 1;
            }
            if j > i + 6 {
                out.push('*');
            }
            i = j;
        } else {
            i += 5;
        }
    }
    out
}

/// Free text (reason / evidence) scrubbed before any cloud upload ():
/// profile names masked, absolute path tokens reduced via [`sanitize_path`].
pub fn scrub_cloud_text(s: &str) -> String {
    let masked = mask_profile_usernames(s);
    let sc: Vec<char> = masked.chars().collect();
    let mut out = String::with_capacity(masked.len());
    let mut i = 0usize;
    while i < sc.len() {
        let is_path_start = (sc[i].is_ascii_alphabetic()
            && i + 2 < sc.len()
            && sc[i + 1] == ':'
            && (sc[i + 2] == '\\' || sc[i + 2] == '/'))
            || (sc[i] == '\\' && i + 1 < sc.len() && sc[i + 1] == '\\');
        if !is_path_start {
            out.push(sc[i]);
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i;
        while j < sc.len()
            && !sc[j].is_whitespace()
            && !matches!(
                sc[j],
                ',' | ';' | '"' | '\'' | ')' | ']' | '}' | '（' | '）'
            )
        {
            j += 1;
        }
        // Trim trailing sentence punctuation from the token.
        while j > start && matches!(sc[j - 1], '.' | ',' | ';' | ':' | ')' | ']' | '}') {
            j -= 1;
        }
        let token: String = sc[start..j].iter().collect();
        if token.contains('\\') || token.contains('/') {
            out.push_str(&sanitize_path(&token, false));
        } else {
            out.push_str(&token);
        }
        i = j;
    }
    out
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
    // Fallback last-segment reduction must never surface a profile name ().
    let masked = mask_profile_usernames(&p);
    let parts: Vec<&str> = masked.split('\\').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        format!("{}\\{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else if let Some(last) = parts.last() {
        (*last).to_string()
    } else {
        "…".into()
    }
}

fn fnv1a64(s: &str) -> u64 {
    crate::fsutil::fnv1a64(s)
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
        // evict oldest ~25% instead of wiping the whole cache.
        if map.len() > 400 {
            let mut by_age: Vec<(u64, u64)> = map.iter().map(|(k, e)| (e.at, *k)).collect();
            by_age.sort_unstable();
            let drop = by_age.len() / 4;
            for (_, k) in by_age.into_iter().take(drop) {
                map.remove(&k);
            }
        }
    }
}

fn normalize_base_url(u: &str, provider: &str) -> String {
    let u = u.trim().trim_end_matches('/').to_string();
    if u.is_empty() {
        let (base, _) = provider_preset(provider);
        return base.into();
    }
    u
}

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
}

/// Anthropic Messages API (`POST /v1/messages`).
fn chat_anthropic(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    let key = cfg.api_key.trim();
    if key.is_empty() {
        return Err("anthropic api key empty".into());
    }
    let base = normalize_base_url(&cfg.base_url, "anthropic");
    // Accept either ".../v1" or full host.
    let url = if base.ends_with("/v1") {
        format!("{base}/messages")
    } else {
        format!("{base}/v1/messages")
    };
    let body = serde_json::json!({
        "model": cfg.model.trim(),
        "max_tokens": 400,
        "temperature": 0.2,
        "system": system,
        "messages": [{"role": "user", "content": user}],
    });
    let resp = build_agent()
        .post(&url)
        .set("Content-Type", "application/json")
        .set("x-api-key", key)
        .set("anthropic-version", "2023-06-01")
        .send_json(body)
        // transport detail survives to the command layer.
        .map_err(|e| format!("ai http failed: {e}"))?;
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("ai parse failed: {e}"))?;
    if let Some(arr) = v["content"].as_array() {
        let text: String = arr
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();
        if !text.is_empty() {
            return Ok(text);
        }
    }
    Err("anthropic empty response".into())
}

/// OpenAI-compatible `/chat/completions`. Ollama uses the same shape.
fn chat_openai_compat(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    let needs_key = cfg.provider != "ollama"
        && !cfg.base_url.contains("127.0.0.1")
        && !cfg.base_url.contains("localhost");
    if needs_key && cfg.api_key.trim().is_empty() {
        return Err("ai api key empty".into());
    }
    let base = normalize_base_url(&cfg.base_url, &cfg.provider);
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
    let mut req = build_agent()
        .post(&url)
        .set("Content-Type", "application/json");
    if !cfg.api_key.trim().is_empty() {
        // the header copy wipes from memory when it leaves scope.
        let auth = zeroize::Zeroizing::new(format!("Bearer {}", cfg.api_key.trim()));
        req = req.set("Authorization", &auth);
    }
    let resp = req
        .send_json(body)
        // transport detail survives to the command layer.
        .map_err(|e| format!("ai http failed: {e}"))?;
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("ai parse failed: {e}"))?;
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

/// Unified chat entry: anthropic | openai | ollama (openai-compatible).
fn chat_completion(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    if !cfg.enabled {
        return Err("ai disabled".into());
    }
    if cfg.model.trim().is_empty() {
        return Err("ai model empty".into());
    }
    match cfg.provider.as_str() {
        "anthropic" => chat_anthropic(cfg, system, user),
        _ => chat_openai_compat(cfg, system, user),
    }
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

const EXPLAIN_SYSTEM: &str =
    "你是 Windows 深度卸载助手。根据给定的残留候选证据，用中文写 1-2 句说明：\
这是什么、为何像残留、删除的常见影响。不要编造路径外的事实。\
suggest_check 仅当 confidence=confirmed 且 risk!=high 时可为 true。\
严格输出 JSON 数组，元素字段：path, summary, suggest_check。";

pub fn explain_items(
    cfg: &AiConfig,
    app_name: &str,
    publisher: &str,
    items: &[ExplainInput],
) -> Result<Vec<ExplainOutput>, String> {
    explain_items_with_completion(cfg, app_name, publisher, items, |user| {
        chat_completion(cfg, EXPLAIN_SYSTEM, user)
    })
}

fn explain_items_with_completion(
    cfg: &AiConfig,
    app_name: &str,
    publisher: &str,
    items: &[ExplainInput],
    mut complete: impl FnMut(&str) -> Result<String, String>,
) -> Result<Vec<ExplainOutput>, String> {
    if items.is_empty() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    let mut pending: Vec<ExplainInput> = Vec::new();

    for it in items {
        // Cache key includes risk + publisher: a verdict cached for a low-risk
        // classification (or another publisher's identical path) must never be
        // replayed for a high-risk one.
        let key = fnv1a64(&format!(
            "{app_name}|{publisher}|{}|{}|{}|{}|{}",
            it.path, it.kind, it.confidence, it.risk, it.reason
        ));
        if let Some(cached) = cache_get(key) {
            if let Ok(mut parsed) = serde_json::from_str::<ExplainOutput>(&cached) {
                // Re-run the freshness gate on hits too — a cached
                // `suggest_check` from a lower-risk classification must not
                // surface on a high-risk item.
                if parsed.suggest_check && it.risk.as_str() == "high" {
                    parsed.suggest_check = false;
                }
                out.push(parsed);
                continue;
            }
        }
        pending.push(it.clone());
    }

    if pending.is_empty() {
        return Ok(out);
    }

    // Bound each request, while processing every uncached item in this call.
    for chunk in pending.chunks(crate::constants::AI_EXPLAIN_MAX_ITEMS) {
        let batch: Vec<&ExplainInput> = chunk.iter().collect();
        let payload: Vec<serde_json::Value> = batch
            .iter()
            .map(|it| {
                serde_json::json!({
                    "path": sanitize_path(&it.path, cfg.allow_cloud_paths),
                    "kind": it.kind,
                    "confidence": it.confidence,
                    "risk": it.risk,
                    // free text never goes to the cloud raw (usernames / deep paths).
                    "reason": scrub_cloud_text(&it.reason),
                    "evidence": it
                        .evidence_labels
                        .iter()
                        .map(|e| scrub_cloud_text(e))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        let user = format!(
        "软件: {}\n发布者: {}\n候选(脱敏路径):\n{}\n请输出 JSON 数组；每个对象必须包含与输入相同的 path 字段（原样回显）。",
        app_name,
        publisher,
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    );

        let text = complete(&user)?;
        let cleaned = strip_code_fence(&text);
        let parsed: Vec<ExplainOutput> =
            serde_json::from_str(&cleaned).map_err(|_| "ai json parse failed".to_string())?;

        // Match back by the exact strings SENT to the model ( + ):
        // the prompt asks the model to echo the sanitized path, and `sanitize_path`
        // is NOT idempotent — re-sanitizing the echo lost every multi-segment
        // AppData path. Never rely on array index alone; identical sanitized paths
        // are indistinguishable to the model, so duplicates attribute in payload
        // order among unconsumed indices.
        let mut sent_map: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (idx, b) in batch.iter().enumerate() {
            sent_map
                .entry(sanitize_path(&b.path, cfg.allow_cloud_paths))
                .or_default()
                .push(idx);
        }
        let mut consumed: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for p in parsed.iter() {
            let candidates = match sent_map.get(p.path.trim()) {
                Some(v) if v.len() == 1 => v.clone(),
                Some(v) => v
                    .iter()
                    .copied()
                    .filter(|i| !consumed.contains(i))
                    .collect(),
                // Model echoed an original (unsanitized) path verbatim — last resort.
                None => batch
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.path == p.path)
                    .map(|(i, _)| i)
                    .collect(),
            };
            let Some(idx) = candidates.iter().copied().find(|i| !consumed.contains(i)) else {
                continue;
            };
            consumed.insert(idx);
            let orig = &batch[idx];
            let item = ExplainOutput {
                path: orig.path.clone(),
                summary: p.summary.clone(),
                suggest_check: p.suggest_check && orig.risk.as_str() != "high",
            };
            let key = fnv1a64(&format!(
                "{app_name}|{publisher}|{}|{}|{}|{}|{}",
                item.path, orig.kind, orig.confidence, orig.risk, orig.reason
            ));
            if let Ok(s) = serde_json::to_string(&item) {
                cache_put(key, s);
            }
            out.push(item);
        }
    }
    Ok(out)
}

// ── Risk brief ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

const RISK_SYSTEM: &str =
    "你是 Windows 卸载风险说明助手。用中文写 2-3 句：这次操作做什么、可能影响什么、\
哪些项默认不会删。语气克制、可执行。不要输出 Markdown 标题。";

const REPORT_SYSTEM: &str =
    "你是 Windows 卸载报告解读助手。用中文写 3-5 句：删了什么、失败怎么办、\
是否建议重启、能否从备份还原。语气克制。不要输出 Markdown 标题。";

pub fn risk_brief(cfg: &AiConfig, input: &RiskBriefInput) -> Result<String, String> {
    // publisher is part of the prompt — without it two products from
    // different vendors with the same shape share a 7-day cache entry.
    let key = fnv1a64(&format!(
        "risk|{}|{}|{}|{}|{}|{}|{}|{}",
        input.app_name,
        input.publisher,
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
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    // backup_dir / top_failed are rendered into the prompt — they
    // must break the cache or the UI shows the previous run's path/failures.
    let key = fnv1a64(&format!(
        "report|{}|{}|{}|{}|{}|{}|{}|{}",
        input.app_name,
        input.deleted,
        input.failed,
        input.skipped,
        input.aborted,
        input.restore_point_ok,
        input.backup_dir,
        serde_json::to_string(&input.top_failed).unwrap_or_default()
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

// ── NL intent (Copilot) ────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NlFilter {
    pub name_like: Option<String>,
    pub publisher: Option<String>,
    pub size_gt_kb: Option<i64>,
    pub installed_after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NlIntent {
    /// list | analyze | batch_uninstall | force_clean
    pub action: String,
    pub filter: NlFilter,
    pub include_leftovers: bool,
    pub note: String,
}

impl Default for NlIntent {
    fn default() -> Self {
        Self {
            action: "list".into(),
            filter: NlFilter::default(),
            include_leftovers: true,
            note: String::new(),
        }
    }
}

const INTENT_SYSTEM: &str = "你是 Windows 卸载助手的意图解析器。把用户中文/英文指令解析成 JSON，\
仅允许 action 取值 list|analyze|batch_uninstall|force_clean。\
filter 字段可选：name_like（关键词）、publisher、size_gt_kb（整数）、installed_after（YYYY-MM-DD）。\
危险动作也要照常解析，由应用层二次确认。\
只输出一个 JSON 对象，不要解释。字段：action, filter, include_leftovers, note。\
note 用一句话复述计划（中文）。";

pub fn parse_nl_intent(
    cfg: &AiConfig,
    user_text: &str,
    app_names: &[String],
) -> Result<NlIntent, String> {
    let names: Vec<&str> = app_names.iter().take(40).map(|s| s.as_str()).collect();
    let user = format!(
        "用户指令：{}\n本机已装软件样例（仅供参考匹配）：{}",
        user_text.trim(),
        names.join(" | ")
    );
    let text = chat_completion(cfg, INTENT_SYSTEM, &user)?;
    let cleaned = strip_code_fence(&text);
    let mut intent: NlIntent =
        serde_json::from_str(&cleaned).map_err(|_| "ai intent parse failed".to_string())?;
    // Clamp dangerous defaults — never auto-run.
    match intent.action.as_str() {
        "list" | "analyze" | "batch_uninstall" | "force_clean" => {}
        _ => intent.action = "list".into(),
    }
    Ok(intent)
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
    fn sanitize_fallback_never_leaks_username() {
        // last-segment reduction used to surface `alice\secret`.
        for p in [
            r"C:\Users\alice\secret",
            r"C:\Users\alice",
            r"C:/Users/alice/secret",
            r"\\?\C:\Users\alice\Desktop\tool",
            r"C:\Users\alice\Documents\MyApp\data",
        ] {
            let s = sanitize_path(p, false);
            assert!(!s.contains("alice"), "{p} → {s}");
            let s = sanitize_path(p, true);
            assert!(!s.contains("alice"), "full {p} → {s}");
        }
    }

    #[test]
    fn mask_profile_usernames_keeps_rest() {
        assert_eq!(
            mask_profile_usernames(r"C:\Users\bob\AppData\Local"),
            r"C:\Users\*\AppData\Local"
        );
        assert_eq!(mask_profile_usernames(r"C:\Users\bob"), r"C:\Users\*");
        assert_eq!(
            mask_profile_usernames(r"see C:\Users\bob\x and D:\Work"),
            r"see C:\Users\*\x and D:\Work"
        );
        // A `Users` path segment still masks the next name (privacy-safe).
        assert_eq!(
            mask_profile_usernames(r"C:\Corp\Users\Public"),
            r"C:\Corp\Users\*"
        );
        // `Users` glued into another token is left alone.
        assert_eq!(
            mask_profile_usernames(r"C:\EndUsers\Public"),
            r"C:\EndUsers\Public"
        );
    }

    #[test]
    fn scrub_cloud_text_masks_paths_and_names() {
        let s = scrub_cloud_text(
            r"Product dir under C:\Users\alice\AppData\Local\Acme\App deep path; also E:\Vendor\Tool\bin\x.dll",
        );
        assert!(!s.contains("alice"), "{s}");
        assert!(!s.contains("AppData\\Local\\Acme\\App"), "{s}");
        assert!(
            s.contains("AppData") || s.contains("Tool") || s.contains("App"),
            "{s}"
        );
        // Non-path free text is preserved.
        assert_eq!(
            scrub_cloud_text("Shell/Classes leftover: Foo"),
            "Shell/Classes leftover: Foo"
        );
    }

    #[test]
    fn provider_presets() {
        assert!(provider_preset("anthropic").0.contains("anthropic"));
        assert!(provider_preset("ollama").0.contains("11434"));
        assert!(provider_preset("openai").0.contains("openai"));
    }

    #[test]
    fn hex_roundtrip() {
        let b = b"sk-test";
        let h = to_hex(b);
        assert_eq!(from_hex(&h).unwrap(), b);
        assert!(from_hex("zz").is_none());
    }

    #[test]
    fn encrypt_key_wraps_or_fails_without_plaintext_write() {
        // on failure the caller gets Err (never a silently plaintext-encrypted blob).
        let e = encrypt_stored_key("sk-abc");
        match e {
            Ok(wrapped) => {
                assert!(!wrapped.is_empty());
                assert_eq!(decrypt_stored_key(&wrapped), "sk-abc");
            }
            Err(code) => assert_eq!(code, "ai:encrypt_failed"),
        }
        assert_eq!(encrypt_stored_key("").unwrap(), "");
    }

    #[test]
    fn config_view_hides_key() {
        // Struct-update syntax would move fields out of a Drop type (ZeroizeOnDrop).
        let mut c = AiConfig::default();
        c.api_key = "sk-secret".into();
        let v = AiConfigView::from(&c);
        assert!(v.has_api_key);
        assert!(!serde_json::to_string(&v).unwrap().contains("sk-secret"));
    }

    /// Debug must never print the API key.
    #[test]
    fn config_debug_redacts_key() {
        let mut c = AiConfig::default();
        c.api_key = "sk-secret".into();
        let s = format!("{c:?}");
        assert!(!s.contains("sk-secret"), "Debug leaked api_key: {s}");
        assert!(s.contains("<redacted>"));
    }
    /// `sanitize_path` is deliberately NOT idempotent for the
    /// common AppData shape — the explain rematch must therefore consult the
    /// strings actually sent to the model instead of re-sanitizing the echo.
    #[test]
    fn sanitize_path_is_not_idempotent_for_appdata_shapes() {
        let once = sanitize_path(r"C:\Users\a\AppData\Local\Vendor\Product", false);
        assert_eq!(once, r"AppData\Vendor\Product");
        assert_ne!(sanitize_path(&once, false), once);
    }

    #[cfg(windows)]
    #[test]
    fn config_write_failure_removes_existing_tmp() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("remova-r23-ai-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ai.json");
        let mut tmp_name = path.as_os_str().to_owned();
        tmp_name.push(format!(".{}.tmp", std::process::id()));
        let tmp = std::path::PathBuf::from(tmp_name);
        std::fs::write(&tmp, "old partial").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x4)
            .open(&tmp)
            .unwrap();
        assert!(write_config_file(&path, "new data").is_err());
        assert!(!tmp.exists());
        assert!(!path.exists());
        drop(held);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explain_batches_rematch_appdata_echoes_and_cache_every_result() {
        let cfg = AiConfig::default();
        let inputs: Vec<ExplainInput> = (0..25)
            .map(|i| ExplainInput {
                path: format!(r"C:\Users\a\AppData\Local\Vendor\R23Product{i}"),
                kind: "dir".into(),
                confidence: "confirmed".into(),
                risk: if i == 0 { "high" } else { "medium" }.into(),
                reason: "R23 regression".into(),
                evidence_labels: vec![],
            })
            .collect();
        let mut batches = vec![];
        let out = explain_items_with_completion(
            &cfg,
            "R23-batching-cache-test",
            "Vendor",
            &inputs,
            |user| {
                let json = user
                    .split_once("候选(脱敏路径):\n")
                    .unwrap()
                    .1
                    .rsplit_once("\n请输出")
                    .unwrap()
                    .0;
                let payload: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
                batches.push(payload.len());
                // The model can reorder results, but still echoes payload paths.
                let echoed: Vec<ExplainOutput> = payload
                    .iter()
                    .rev()
                    .map(|item| {
                        let path = item["path"].as_str().unwrap().to_string();
                        ExplainOutput {
                            summary: path.clone(),
                            path,
                            suggest_check: true,
                        }
                    })
                    .collect();
                Ok(serde_json::to_string(&echoed).unwrap())
            },
        )
        .unwrap();
        assert_eq!(batches, [12, 12, 1]);
        assert_eq!(out.len(), inputs.len());
        for input in &inputs {
            let result = out.iter().find(|item| item.path == input.path).unwrap();
            assert_eq!(result.summary, sanitize_path(&input.path, false));
            assert_eq!(result.suggest_check, input.risk != "high");
        }
        let cached = explain_items_with_completion(
            &cfg,
            "R23-batching-cache-test",
            "Vendor",
            &inputs,
            |_| panic!("all 25 explanations must hit cache, including the last batch"),
        )
        .unwrap();
        assert_eq!(cached.len(), inputs.len());
        assert!(cached
            .iter()
            .all(|item| out.iter().any(|v| v.path == item.path
                && v.summary == item.summary
                && v.suggest_check == item.suggest_check)));
    }

    #[test]
    fn explain_duplicate_sanitized_echoes_consume_originals_once() {
        let inputs: Vec<ExplainInput> = ["a", "b"]
            .iter()
            .map(|name| ExplainInput {
                path: format!(r"C:\Users\{name}\AppData\Local\Vendor\R23Collision"),
                kind: "dir".into(),
                confidence: "confirmed".into(),
                risk: "medium".into(),
                reason: "duplicate fixture".into(),
                evidence_labels: vec![],
            })
            .collect();
        let path = sanitize_path(&inputs[0].path, false);
        assert_eq!(path, sanitize_path(&inputs[1].path, false));
        let outputs: Vec<_> = (0..3)
            .map(|i| ExplainOutput {
                path: path.clone(),
                summary: format!("explanation-{i}"),
                suggest_check: false,
            })
            .collect();
        let result = explain_items_with_completion(
            &AiConfig::default(),
            "R23-collision-cache-test",
            "Vendor",
            &inputs,
            |_| Ok(serde_json::to_string(&outputs).unwrap()),
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, inputs[0].path);
        assert_eq!(result[1].path, inputs[1].path);
        assert_eq!(result[1].summary, "explanation-1");
    }
}
