//! AI orchestration: config, sanitize, OpenAI-compatible chat, cache.
//! AI never deletes; callers treat all output as advisory.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
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
    /// Allow sanitized paths from recognized local folders. Default omits path fields.
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

/// Stable credential binding key: credentials belong to a provider and origin,
/// never to a UI preset or an arbitrary path spelling.
fn credential_source(provider: &str, base_url: &str) -> Option<String> {
    let base = secure_base_url(base_url, provider).ok()?;
    let url = tauri::Url::parse(&base).ok()?;
    let provider = provider.trim().to_ascii_lowercase();
    Some(format!("{provider}|{}", url.origin().ascii_serialization()))
}

/// None retains an existing credential only at the same valid provider/origin;
/// Some("") explicitly clears it. Called inside the config transaction before
/// changing provider/base_url, so the comparison always uses the stored source.
pub fn replace_credential(
    config: &mut AiConfig,
    provider: &str,
    base_url: &str,
    key: Option<zeroize::Zeroizing<String>>,
) {
    let old_source = credential_source(&config.provider, &config.base_url);
    let new_source = credential_source(provider, base_url);
    match key {
        Some(key) => {
            config.api_key.zeroize();
            config.api_key = key.trim().to_string();
        }
        None if old_source.is_none() || old_source != new_source => {
            config.api_key.zeroize();
        }
        None => {}
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

fn config_path() -> Result<PathBuf, String> {
    // Test-only redirect: the IPC tests exercise `save_ai_config` for real, and
    // must never read or rewrite the user's actual AI settings. Compiled out of
    // the shipped binary.
    #[cfg(test)]
    if let Ok(dir) = std::env::var("REMOVA_AI_CONFIG_DIR") {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir).join("ai-config.json"));
        }
    }
    // Known Folder, not LOCALAPPDATA: an elevated process must not read or write
    // a path an environment variable can point at a network share.
    #[cfg(windows)]
    let base = crate::sysops::known_folder(&windows::Win32::UI::Shell::FOLDERID_LocalAppData)
        .ok_or_else(|| "ai:config_dir_unavailable".to_string())?;
    #[cfg(not(windows))]
    let base = {
        let raw = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| "ai:config_dir_unavailable".to_string())?;
        PathBuf::from(raw)
    };
    if !crate::fsutil::is_local_drive_path(&base.to_string_lossy()) {
        return Err("ai:config_dir_unavailable".into());
    }
    Ok(base.join("Remova").join("ai-config.json"))
}

static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn load_config() -> Result<AiConfig, String> {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    load_config_file(&config_path()?)
}

fn open_config_file(p: &Path) -> io::Result<std::fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };

        // Open via the verified ancestor handles. A successful path lookup
        // alone cannot authorize a subsequent read of a replaceable name.
        let file = crate::fsutil::open_local_regular_file(p)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(io::Error::other)?;
        if info.nNumberOfLinks != 1 {
            return Err(io::Error::other("AI settings file has multiple links"));
        }
        Ok(file)
    }
    #[cfg(not(windows))]
    {
        crate::fsutil::ensure_no_reparse_ancestors(p)?;
        let file = std::fs::File::open(p)?;
        if !file.metadata()?.is_file() || crate::fsutil::has_multiple_links(p) {
            return Err(io::Error::other("AI settings require an ordinary file"));
        }
        Ok(file)
    }
}

fn load_config_file(p: &Path) -> Result<AiConfig, String> {
    let mut file = match open_config_file(p) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(AiConfig::default()),
        Err(e) => return Err(format!("ai:config_read_failed::{e}")),
    };
    let mut s = zeroize::Zeroizing::new(String::new());
    file.read_to_string(&mut s)
        .map_err(|e| format!("ai:config_read_failed::{e}"))?;
    // The read handle denies replacement until closed. Migration uses the
    // same guarded writer as an explicit settings change.
    drop(file);
    let mut c: AiConfig =
        serde_json::from_str(&s).map_err(|e| format!("ai:config_read_failed::{e}"))?;
    // take ownership of the stored bytes so the pre-image is wiped
    // explicitly instead of being dropped by the field assignment.
    let mut raw_key = std::mem::take(&mut c.api_key);
    let decrypted = decrypt_stored_key(&raw_key);
    // migrate a legacy plaintext key to DPAPI at rest on first load.
    let migrate = !raw_key.is_empty() && !raw_key.starts_with(KEY_PREFIX);
    // the ciphertext copy leaves no residue either.
    raw_key.zeroize();
    c.api_key = decrypted?;
    if migrate {
        // The wrap must not silently fail: a swallowed error leaves the
        // plaintext key on disk while the UI reports it protected.
        if let Err(e) = save_config_file(p, &c) {
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
                let _ = write_config_file(p, &s);
            }
            let mut key = std::mem::take(&mut c.api_key);
            key.zeroize();
            return Err(e);
        }
    }
    Ok(c)
}

/// Atomic config write (tmp + rename): a crash mid-write must not destroy the
/// DPAPI-wrapped key or a half-migrated file. The caller holds CONFIG_LOCK
/// for the complete read/migrate/update transaction.
fn write_config_file(p: &Path, s: &str) -> Result<(), String> {
    let write = || -> io::Result<()> {
        let parent = p
            .parent()
            .ok_or_else(|| io::Error::other("AI settings path has no parent"))?;
        crate::fsutil::ensure_no_reparse_ancestors(p)?;
        let _pins = crate::fsutil::create_dirs_pinned(parent)?;
        // Guard the parent through publication, and require any existing
        // config to be readable and free of links before replacing it.
        match open_config_file(p) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        crate::fsutil::write_bytes_atomic(p, s.as_bytes())
    };
    write().map_err(|e| format!("ai:config_write_failed::{e}"))
}

pub fn update_config(f: impl FnOnce(&mut AiConfig)) -> Result<AiConfigView, String> {
    update_config_file(&config_path()?, f)
}

fn update_config_file(p: &Path, f: impl FnOnce(&mut AiConfig)) -> Result<AiConfigView, String> {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = load_config_file(p)?;
    f(&mut c);
    save_config_file(p, &c)?;
    Ok(AiConfigView::from(&c))
}

fn save_config_file(p: &Path, c: &AiConfig) -> Result<(), String> {
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
    write_config_file(p, &s)
}

const KEY_PREFIX: &str = "dpapi:";

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
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
        // CryptProtectData allocates pbData via LocalAlloc.
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
fn decrypt_stored_key(stored: &str) -> Result<String, String> {
    let Some(hex) = stored.strip_prefix(KEY_PREFIX) else {
        return Ok(stored.to_string());
    };
    let Some(raw) = from_hex(hex) else {
        return Err("ai:config_read_failed::invalid encrypted key".into());
    };
    if raw.is_empty() {
        return Err("ai:config_read_failed::empty encrypted key".into());
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
            return Err("ai:config_read_failed::cannot decrypt key".into());
        }
        let dec = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        // CryptUnprotectData allocates pbData via LocalAlloc.
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            out_blob.pbData as *mut core::ffi::c_void,
        ));
        // a failed UTF-8 decode still holds key bytes — wipe them.
        match String::from_utf8(dec) {
            Ok(s) => Ok(s),
            Err(e) => {
                let mut bad = e.into_bytes();
                bad.zeroize();
                Err("ai:config_read_failed::invalid decrypted key".into())
            }
        }
    }
}

#[cfg(not(windows))]
fn decrypt_stored_key(stored: &str) -> Result<String, String> {
    Ok(stored.to_string())
}

/// Free text can include a path with spaces or punctuation. Withhold the
/// whole field if it contains path syntax, regardless of the path setting.
fn scrub_cloud_text(text: &str) -> &str {
    let bytes = text.as_bytes();
    if text.contains(['\\', '/'])
        || bytes.windows(3).enumerate().any(|(i, part)| {
            (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
                && part[0].is_ascii_alphabetic()
                && part[1] == b':'
                && !part[2].is_ascii_whitespace()
        })
    {
        "[path]"
    } else {
        text
    }
}

/// Opt-in only: recognize standard local roots, mask the user profile
/// component, and withhold network, device, and unfamiliar local paths.
fn cloud_path(path: &str, allowed: bool) -> Option<String> {
    if !allowed {
        return None;
    }
    let normalized = path.replace('/', "\\");
    let path = normalized.as_str();
    let [drive, b':', b'\\', ..] = path.as_bytes() else {
        return None;
    };
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    let mut parts: Vec<&str> = path[3..].trim_end_matches('\\').split('\\').collect();
    if parts.iter().any(|part| {
        matches!(*part, "" | "." | "..")
            || part.contains([':', '%', '*', '?', '"', '<', '>', '|'])
            || part.chars().any(char::is_control)
    }) {
        return None;
    }
    let root = parts.first()?;
    if root.eq_ignore_ascii_case("Users") {
        *parts.get_mut(1)? = "*";
    } else if ![
        "Program Files",
        "Program Files (x86)",
        "ProgramData",
        "Windows",
    ]
    .iter()
    .any(|standard| root.eq_ignore_ascii_case(standard))
    {
        return None;
    }
    Some(format!("{}{}", &path[..3], parts.join("\\")))
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

/// Parsed pieces of a configured endpoint, enough to police where a stored
/// secret may be sent. Deliberately strict: anything unparseable is refused.
struct Endpoint {
    scheme: String,
    host: String,
    has_userinfo: bool,
}

fn parse_endpoint(raw: &str) -> Result<Endpoint, String> {
    let text = raw.trim();
    if !text.contains("://") || text.contains(char::from(92)) || text.chars().any(char::is_control)
    {
        return Err("ai endpoint must be an absolute http(s) URL".into());
    }
    let url = tauri::Url::parse(text).map_err(|_| "ai endpoint invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("ai endpoint scheme not allowed".into());
    }
    let host = url.host_str().ok_or("ai endpoint has no host")?;
    let authority = text
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    Ok(Endpoint {
        scheme: url.scheme().to_string(),
        host: host.to_ascii_lowercase(),
        has_userinfo: authority.contains('@'),
    })
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// A base URL is only usable when it cannot leak the API key in clear text.
///
/// Remote endpoints must be HTTPS. Plain HTTP is accepted only for an exact
/// loopback host, which is how local model servers are addressed. Rejecting
/// here — before any request is built — means a refused endpoint performs no
/// network I/O at all.
fn secure_base_url(raw: &str, provider: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    let effective = if trimmed.is_empty() {
        provider_preset(provider).0.to_string()
    } else {
        trimmed
    };
    let endpoint = parse_endpoint(&effective)?;
    if endpoint.has_userinfo {
        return Err("ai endpoint must not contain user information".into());
    }
    if endpoint.scheme == "http" && !is_loopback_host(&endpoint.host) {
        return Err("ai endpoint must use https (http is only allowed for localhost)".into());
    }
    Ok(effective)
}

fn build_agent() -> ureq::Agent {
    // AI requests carry credentials. Do not let ureq follow a redirect and
    // decide which destination receives provider-specific authentication.
    // The response checker below reports 3xx without exposing Location.
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
}

fn require_chat_response(response: ureq::Response) -> Result<ureq::Response, String> {
    if (300..400).contains(&response.status()) {
        return Err("ai redirect refused".into());
    }
    Ok(response)
}

/// Anthropic Messages API (`POST /v1/messages`).
fn chat_anthropic(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    let key = cfg.api_key.trim();
    if key.is_empty() {
        return Err("anthropic api key empty".into());
    }
    // The key is about to be sent: refuse a cleartext remote endpoint before
    // building the request.
    let base = secure_base_url(&cfg.base_url, "anthropic")?;
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
    let v: serde_json::Value = require_chat_response(resp)?
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
    let base = secure_base_url(&cfg.base_url, &cfg.provider)?;
    let endpoint = parse_endpoint(&base)?;
    let needs_key = cfg.provider != "ollama" && !is_loopback_host(&endpoint.host);
    if needs_key && cfg.api_key.trim().is_empty() {
        return Err("ai api key empty".into());
    }
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
    let v: serde_json::Value = require_chat_response(resp)?
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

#[derive(Deserialize)]
struct ModelExplanation {
    id: usize,
    summary: String,
    suggest_check: bool,
}

const EXPLAIN_SYSTEM: &str =
    "你是 Windows 深度卸载助手。根据给定的残留候选证据，用中文写 1-2 句说明：\
这是什么、为何像残留、删除的常见影响。不要编造给定证据以外的事实。\
suggest_check 仅当 confidence=confirmed 且 risk!=high 时可为 true。\
严格输出 JSON 数组，元素字段：id, summary, suggest_check。";

const EXPLAIN_SYSTEM_EN: &str =
    "You are a Windows deep-uninstall assistant. Using the supplied leftover \
evidence, write 1-2 sentences in English per item: what it is, why it looks \
like a leftover, and the usual impact of deleting it. Never invent facts \
beyond the given evidence. Set suggest_check true only when confidence=confirmed \
and risk!=high. Output a strict JSON array with fields: id, summary, \
suggest_check.";

/// Normalize an IPC-supplied locale to the two supported UI languages. Anything
/// unrecognized falls back to Chinese, which is the product default.
fn normalize_locale(locale: &str) -> &'static str {
    if locale.trim().to_ascii_lowercase().starts_with("en") {
        "en"
    } else {
        "zh"
    }
}

/// Pick the prompt matching the active UI language, so an English UI never gets
/// a reply the prompt forced into Chinese.
fn localized_system<'a>(locale: &str, zh: &'a str, en: &'a str) -> &'a str {
    if normalize_locale(locale) == "en" {
        en
    } else {
        zh
    }
}

pub fn explain_items(
    cfg: &AiConfig,
    app_name: &str,
    publisher: &str,
    items: &[ExplainInput],
    locale: &str,
) -> Result<Vec<ExplainOutput>, String> {
    explain_items_with_completion(cfg, app_name, publisher, items, locale, |user| {
        chat_completion(
            cfg,
            localized_system(locale, EXPLAIN_SYSTEM, EXPLAIN_SYSTEM_EN),
            user,
        )
    })
}

fn explain_items_with_completion(
    cfg: &AiConfig,
    app_name: &str,
    publisher: &str,
    items: &[ExplainInput],
    locale: &str,
    mut complete: impl FnMut(&str) -> Result<String, String>,
) -> Result<Vec<ExplainOutput>, String> {
    if items.is_empty() {
        return Ok(vec![]);
    }
    // results are placed back at the caller's index, so cache hits and
    // chunked completions cannot reorder the output relative to the input.
    let mut slots: Vec<Option<ExplainOutput>> = (0..items.len()).map(|_| None).collect();
    let mut pending = Vec::new();

    for (idx, it) in items.iter().enumerate() {
        // Include the permission and evidence so a previous prompt's verdict
        // cannot be reused when the evidence or sharing setting changes.
        let key = fnv1a64(
            &serde_json::json!((
                "explain",
                app_name,
                publisher,
                cfg.allow_cloud_paths,
                normalize_locale(locale),
                it
            ))
            .to_string(),
        );
        if let Some(cached) = cache_get(key) {
            if let Ok(parsed) = serde_json::from_str::<ExplainOutput>(&cached) {
                slots[idx] = Some(parsed);
                continue;
            }
        }
        pending.push((idx, it, key));
    }

    // Bound each request, while processing every uncached item in this call.
    for chunk in pending.chunks(crate::constants::AI_EXPLAIN_MAX_ITEMS) {
        let payload: Vec<serde_json::Value> = chunk
            .iter()
            .enumerate()
            .map(|(id, (_, it, _))| {
                let mut candidate = serde_json::json!({
                    "id": id + 1,
                    "kind": scrub_cloud_text(&it.kind),
                    "confidence": scrub_cloud_text(&it.confidence),
                    "risk": scrub_cloud_text(&it.risk),
                    "reason": scrub_cloud_text(&it.reason),
                    "evidence": it.evidence_labels.iter().map(|e| scrub_cloud_text(e)).collect::<Vec<_>>(),
                });
                if let Some(path) = cloud_path(&it.path, cfg.allow_cloud_paths) {
                    candidate["path"] = path.into();
                }
                candidate
            })
            .collect();
        let payload_json = serde_json::to_string_pretty(&payload).unwrap_or_default();
        // Product names can themselves contain a path/time-looking token; the
        // cloud payload is scrubbed as a whole, like every other free-text field.
        let app = scrub_cloud_text(app_name);
        let vendor = scrub_cloud_text(publisher);
        let user = if normalize_locale(locale) == "en" {
            format!(
                "App: {app}\nPublisher: {vendor}\nCandidates:\n{payload_json}\nReturn a JSON array with the same numeric candidate IDs."
            )
        } else {
            format!(
                "软件: {app}\n发布者: {vendor}\n候选:\n{payload_json}\n请输出 JSON 数组，每个对象须使用候选项原有的数字 ID。"
            )
        };

        let text = complete(&user)?;
        let cleaned = strip_code_fence(&text);
        let parsed: Vec<ModelExplanation> =
            serde_json::from_str(&cleaned).map_err(|e| format!("ai json parse failed::{e}"))?;

        for explanation in parsed {
            let Some(&(index, original, key)) =
                explanation.id.checked_sub(1).and_then(|id| chunk.get(id))
            else {
                continue;
            };
            if slots[index].is_some() {
                continue;
            }
            let item = ExplainOutput {
                path: original.path.clone(),
                summary: explanation.summary,
                suggest_check: explanation.suggest_check
                    && original.confidence == "confirmed"
                    && original.risk != "high",
            };
            if let Ok(s) = serde_json::to_string(&item) {
                cache_put(key, s);
            }
            slots[index] = Some(item);
        }
    }
    Ok(slots.into_iter().flatten().collect())
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

const RISK_SYSTEM_EN: &str = "You are a Windows uninstall risk explainer. Write 2-3 sentences in \
English: what this operation does, what it may affect, and which items are \
never deleted by default. Keep the tone measured and actionable. Do not emit \
Markdown headings.";

const REPORT_SYSTEM_EN: &str =
    "You are a Windows uninstall report explainer. Write 3-5 sentences in \
English: what was deleted, what to do about failures, whether a restart is \
advised, and whether the backup can restore it. Keep the tone measured. Do not \
emit Markdown headings.";

fn risk_user_prompt(input: &RiskBriefInput, locale: &str) -> String {
    let app = scrub_cloud_text(&input.app_name);
    let vendor = scrub_cloud_text(&input.publisher);
    let action = scrub_cloud_text(&input.action);
    let kinds: Vec<_> = input
        .kind_counts
        .iter()
        .map(|(kind, count)| (scrub_cloud_text(kind), count))
        .collect();
    if normalize_locale(locale) == "en" {
        format!(
            "App: {} ({})\nAction: {}\nCandidates: {}\nKind counts: {:?}\nHas service: {}\nHas startup item: {}\nSuspected shared component: {}",
            app,
            vendor,
            action,
            input.item_count,
            kinds,
            input.has_service,
            input.has_run_key,
            input.has_shared_hint
        )
    } else {
        format!(
            "软件: {} ({})\n动作: {}\n候选项: {}\n类型分布: {:?}\n含服务: {}\n含启动项: {}\n疑似共享组件: {}",
            app,
            vendor,
            action,
            input.item_count,
            kinds,
            input.has_service,
            input.has_run_key,
            input.has_shared_hint
        )
    }
}

pub fn risk_brief(cfg: &AiConfig, input: &RiskBriefInput, locale: &str) -> Result<String, String> {
    // publisher is part of the prompt — without it two products from
    // different vendors with the same shape share a 7-day cache entry.
    let key = fnv1a64(&format!(
        "risk|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        input.app_name,
        input.publisher,
        input.action,
        input.item_count,
        input.has_service,
        input.has_run_key,
        input.has_shared_hint,
        serde_json::to_string(&input.kind_counts).unwrap_or_default(),
        normalize_locale(locale)
    ));
    if let Some(c) = cache_get(key) {
        return Ok(c);
    }
    let user = risk_user_prompt(input, locale);
    let text = chat_completion(
        cfg,
        localized_system(locale, RISK_SYSTEM, RISK_SYSTEM_EN),
        &user,
    )?;
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

fn report_user_prompt(cfg: &AiConfig, input: &ReportBriefInput, locale: &str) -> String {
    use std::fmt::Write;

    let english = normalize_locale(locale) == "en";
    let mut prompt = if english {
        format!(
            "App: {}\nDeleted: {}\nFailed: {}\nSkipped: {}\nAborted: {}\nBackup recorded: {}\nRestore point: {}",
            scrub_cloud_text(&input.app_name), input.deleted, input.failed, input.skipped,
            input.aborted, !input.backup_dir.is_empty(), input.restore_point_ok
        )
    } else {
        format!(
            "软件: {}\n删除: {}\n失败: {}\n跳过: {}\n中止: {}\n有备份记录: {}\n还原点: {}",
            scrub_cloud_text(&input.app_name),
            input.deleted,
            input.failed,
            input.skipped,
            input.aborted,
            !input.backup_dir.is_empty(),
            input.restore_point_ok
        )
    };
    if let Some(backup) = cloud_path(&input.backup_dir, cfg.allow_cloud_paths) {
        let label = if english {
            "Backup dir"
        } else {
            "备份目录"
        };
        let _ = write!(prompt, "\n{label}: {backup}");
    }
    let failed = input
        .top_failed
        .iter()
        .filter_map(|path| cloud_path(path, cfg.allow_cloud_paths))
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        let label = if english {
            "Failed samples"
        } else {
            "失败样例"
        };
        let _ = write!(prompt, "\n{label}: {}", failed.join(" | "));
    }
    prompt
}

pub fn summarize_report(
    cfg: &AiConfig,
    input: &ReportBriefInput,
    locale: &str,
) -> Result<String, String> {
    let user = report_user_prompt(cfg, input, locale);
    // Cache exactly the privacy-aware payload; changing permission invalidates
    // it, and so does switching the UI language.
    let key = fnv1a64(&format!(
        "report|{}|{}|{user}",
        cfg.allow_cloud_paths,
        normalize_locale(locale)
    ));
    if let Some(c) = cache_get(key) {
        return Ok(c);
    }
    let text = chat_completion(
        cfg,
        localized_system(locale, REPORT_SYSTEM, REPORT_SYSTEM_EN),
        &user,
    )?;
    cache_put(key, text.clone());
    Ok(text)
}

// ── NL intent (Copilot) ────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NlFilter {
    pub name_like: Option<String>,
    pub publisher: Option<String>,
    #[serde(alias = "size_gt_KB")]
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

const INTENT_SYSTEM_EN: &str = "You are the intent parser for a Windows uninstall assistant. \
Parse the user's instruction (Chinese or English) into JSON. Only these action \
values are allowed: list|analyze|batch_uninstall|force_clean. Optional filter \
fields: name_like (keyword), publisher, size_gt_kb (integer), installed_after \
(YYYY-MM-DD). Parse dangerous actions normally; the app layer confirms them \
separately. Output exactly one JSON object with no explanation. Fields: action, \
filter, include_leftovers, note. Write note in one English sentence.";

fn intent_user_prompt(instruction: &str, names: &str, locale: &str) -> String {
    if normalize_locale(locale) == "en" {
        format!(
            "User instruction: {instruction}\nInstalled app samples (for matching only): {names}"
        )
    } else {
        format!("用户指令：{instruction}\n本机已装软件样例（仅供参考匹配）：{names}")
    }
}

pub fn parse_nl_intent(
    cfg: &AiConfig,
    user_text: &str,
    app_names: &[String],
    locale: &str,
) -> Result<NlIntent, String> {
    let scrubbed_text = scrub_cloud_text(user_text.trim());
    let names: Vec<_> = app_names
        .iter()
        .take(40)
        .map(|name| scrub_cloud_text(name))
        .collect();
    let user = intent_user_prompt(scrubbed_text, &names.join(" | "), locale);
    let text = chat_completion(
        cfg,
        localized_system(locale, INTENT_SYSTEM, INTENT_SYSTEM_EN),
        &user,
    )?;
    let cleaned = strip_code_fence(&text);
    let mut intent: NlIntent =
        serde_json::from_str(&cleaned).map_err(|e| format!("ai intent parse failed::{e}"))?;
    // Clamp dangerous defaults — never auto-run.
    match intent.action.as_str() {
        "list" | "analyze" | "batch_uninstall" | "force_clean" => {}
        _ => intent.action = "list".into(),
    }
    Ok(intent)
}

#[cfg(test)]
mod tests;
