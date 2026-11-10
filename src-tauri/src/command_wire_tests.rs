//! Command-layer wire tests: the exact JSON the frontend sends, parsed into the
//! real handler arguments, through the real command functions.
//!
//! These are deliberately *not* a webview round trip: spinning up Tauri's mock
//! runtime links the tray/menu code into the test binary, which then needs a
//! Common-Controls v6 manifest that conflicts with the application manifest at
//! link time. What is covered here is everything below the transport — the wire
//! shape, the command's own validation, and (for credentials) real DPAPI — and
//! the module name says so rather than claiming more.

use serde_json::{json, Value};

/// The arguments `save_ai_config` receives, deserialized the way Tauri does:
/// a JSON object with camelCase keys, one entry per argument.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveAiConfigArgs {
    enabled: bool,
    provider: String,
    base_url: String,
    model: String,
    allow_cloud_paths: bool,
    api_key: Option<String>,
}

fn save_ai_config_from_wire(body: Value) -> Result<Value, String> {
    let args: SaveAiConfigArgs = serde_json::from_value(body).map_err(|e| e.to_string())?;
    let view = crate::commands::ai_cmd::save_ai_config(
        args.enabled,
        args.provider,
        args.base_url,
        args.model,
        args.allow_cloud_paths,
        args.api_key,
    )?;
    serde_json::to_value(view).map_err(|e| e.to_string())
}

/// A wire payload that changes the endpoint without sending a new key
/// must not carry the old key across; the key is bound to (provider, origin).
#[test]
fn save_ai_config_wire_payload_binds_the_key_to_its_source() {
    let _lock = crate::test_support::EnvLock::new();
    let dir = crate::test_support::TempDir::new("wire_ai_config");
    let config_dir = dir.path().to_string_lossy().into_owned();
    let _env = crate::test_support::EnvVar::set("REMOVA_AI_CONFIG_DIR", &config_dir);

    let save = |provider: &str, base: &str, key: Option<&str>| {
        save_ai_config_from_wire(json!({
            "enabled": true,
            "provider": provider,
            "baseUrl": base,
            "model": "m",
            "allowCloudPaths": false,
            "apiKey": key,
        }))
    };

    let first = save(
        "openai",
        "https://api.example.test/v1",
        Some("sk-old-secret"),
    )
    .expect("first save must succeed");
    assert_eq!(first["has_api_key"], json!(true), "the key was stored");

    // Same provider, same normalized origin, key omitted: the stored key stays.
    let kept = save("openai", "https://api.example.test/v1/", None).expect("same-source save");
    assert_eq!(
        kept["has_api_key"],
        json!(true),
        "same source keeps the key"
    );

    // Different origin, key omitted: the old key must not follow.
    let moved = save("openai", "https://other.example.test/v1", None).expect("new-origin save");
    assert_eq!(
        moved["has_api_key"],
        json!(false),
        "changing the origin must clear the old key"
    );

    // Plaintext must never reach the config file.
    let raw = std::fs::read_to_string(dir.path().join("ai-config.json")).expect("config file");
    assert!(
        !raw.contains("sk-old-secret"),
        "the stored key must be DPAPI-encrypted, not plaintext"
    );
}

/// A wire payload with a valid app identity but a flipped safety flag is
/// rejected by the server-side snapshot comparison, not by trusting the client.
#[test]
fn cleanup_wire_payload_with_a_flipped_safety_flag_is_rejected() {
    use crate::policy::CleanupSource;
    use crate::scanner::CleanupItem;

    let _scan_lock = crate::scan_allow::test_lock();
    let fixture = crate::test_support::TempDir::new("wire_cleanup");
    let dir = fixture.path().to_string_lossy().into_owned();

    let app = crate::apps::InstalledApp {
        name: "WireTrustedDemo".into(),
        version: "1".into(),
        publisher: "Vendor".into(),
        install_location: dir.clone(),
        uninstall_string: r"C:\Program Files\Demo\unins000.exe".into(),
        quiet_uninstall_string: String::new(),
        source: "HKLM64".into(),
        registry_key: r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\WireTrustedDemo"
            .into(),
        estimated_size_kb: 0,
        install_date: String::new(),
        display_icon: String::new(),
    };

    // The client sends JSON, exactly as the frontend builds it.
    let benign: CleanupItem = serde_json::from_value(json!({
        "path": dir,
        "kind": "dir",
        "score": 90,
        "confidence": "confirmed",
        "risk": "low",
        "reason": "fixture",
        "evidence": [],
        "service_bound": false,
    }))
    .expect("wire item parses");

    let key = crate::apps::app_key(&app);
    crate::scan_allow::remember_uninstall_items(&key, std::slice::from_ref(&benign));
    assert!(
        crate::authorize_cleanup_items(
            &app,
            std::slice::from_ref(&benign),
            CleanupSource::Uninstall
        )
        .is_ok(),
        "the recorded item itself is accepted"
    );

    let mut forged_json = json!({
        "path": benign.path,
        "kind": "dir",
        "score": 90,
        "confidence": "confirmed",
        "risk": "low",
        "reason": "fixture",
        "evidence": [],
        "service_bound": false,
    });
    forged_json["service_bound"] = json!(true);
    let forged: CleanupItem = serde_json::from_value(forged_json).expect("forged wire item parses");
    let error = crate::authorize_cleanup_items(&app, &[forged], CleanupSource::Uninstall)
        .expect_err("a flipped safety flag must be refused");
    assert!(
        error.contains("not from the latest analysis"),
        "refusal must come from the snapshot comparison: {error}"
    );
}
