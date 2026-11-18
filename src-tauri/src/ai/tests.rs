use super::*;

#[test]
fn credential_binding_changes_when_provider_or_origin_changes() {
    assert_eq!(
        credential_source("openai", "https://api.example.test/v1").as_deref(),
        Some("openai|https://api.example.test")
    );
    assert_ne!(
        credential_source("openai", "https://api.example.test/v1"),
        credential_source("anthropic", "https://api.example.test/v1")
    );
    assert_ne!(
        credential_source("openai", "https://api.example.test/v1"),
        credential_source("openai", "https://other.example.test/v1")
    );
    assert_eq!(
        credential_source("openai", "https://api.example.test/v1"),
        credential_source("openai", "https://api.example.test:443/other/path")
    );
    assert!(credential_source("openai", "http://remote.example.test/v1").is_none());
}

#[test]
fn changing_ai_credential_source_clears_key_but_same_origin_retains_it() {
    let mut cfg = AiConfig::default();
    cfg.api_key = "fixture-secret".into();
    replace_credential(&mut cfg, "openai", "https://api.openai.com/v1/", None);
    assert_eq!(cfg.api_key, "fixture-secret");

    replace_credential(&mut cfg, "anthropic", "https://api.anthropic.com/v1", None);
    assert!(cfg.api_key.is_empty());
    cfg.api_key = "fixture-secret".into();
    replace_credential(&mut cfg, "ollama", "http://127.0.0.1:11434/v1", None);
    assert!(cfg.api_key.is_empty());

    cfg.api_key = "fixture-secret".into();
    replace_credential(&mut cfg, "openai", "https://different.example/v1", None);
    assert!(cfg.api_key.is_empty());

    cfg.api_key = "fixture-secret".into();
    replace_credential(
        &mut cfg,
        "openai",
        "https://api.openai.com/v1",
        Some(zeroize::Zeroizing::new(String::new())),
    );
    assert!(
        cfg.api_key.is_empty(),
        "explicit empty input clears the key"
    );
}

#[test]
fn cleartext_remote_endpoints_are_refused_before_any_request() {
    for bad in [
        "http://api.example.com/v1",
        "http://192.168.1.10:11434/v1",
        "https://user:pass@api.example.com/v1",
        "http://user@example.com/v1",
        "ftp://api.example.com/v1",
        "api.example.com/v1",
        "http://localhost.evil.com/v1",
        "http://127.0.0.1.evil.com/v1",
    ] {
        assert!(
            secure_base_url(bad, "openai").is_err(),
            "{bad} must be refused"
        );
    }
    for ok in [
        "https://api.example.com/v1",
        "http://127.0.0.1:11434/v1",
        "http://localhost:11434/v1",
        "http://[::1]:11434/v1",
    ] {
        assert!(
            secure_base_url(ok, "openai").is_ok(),
            "{ok} must be allowed"
        );
    }
    // An empty base falls back to the provider preset, which is HTTPS.
    assert_eq!(
        secure_base_url("", "openai").unwrap(),
        provider_preset("openai").0
    );
}

// Loopback-only fixture: both destinations are owned by the test and all keys
// are synthetic. The server answers a followed redirect too, so a regression
// fails an assertion rather than waiting for the HTTP timeout.
fn capture_ai_request<T>(
    cfg: &mut AiConfig,
    response_status: u16,
    cross_origin: bool,
    model_reply: &str,
    request: impl FnOnce(&AiConfig) -> Result<T, String>,
) -> (Result<T, String>, Vec<(bool, String)>) {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0, "fixture request closed early");
            bytes.extend_from_slice(&buffer[..n]);
            assert!(bytes.len() < 32 * 1024, "fixture request exceeded limit");
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let content_length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + content_length {
                    return String::from_utf8(bytes).unwrap();
                }
            }
        }
    }

    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    source.set_nonblocking(true).unwrap();
    target.set_nonblocking(true).unwrap();
    let source_addr = source.local_addr().unwrap();
    let target_addr = target.local_addr().unwrap();
    cfg.base_url = format!("http://{source_addr}/v1");
    let location = if cross_origin {
        format!("http://{target_addr}/redirected")
    } else {
        format!("http://{source_addr}/redirected")
    };
    let body = serde_json::json!({
        "content": [{"text": model_reply}],
        "choices": [{"message": {"content": model_reply}}],
    })
    .to_string();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        let mut deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            for (is_target, listener) in [(false, &source), (true, &target)] {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Windows inherits the listener's non-blocking flag into
                        // the accepted socket; the request reader needs a
                        // blocking stream or every read fails with WouldBlock.
                        stream.set_nonblocking(false).unwrap();
                        let request = read_request(&mut stream);
                        let first = requests.is_empty();
                        requests.push((is_target, request));
                        let status = if first { response_status } else { 200 };
                        let response = format!(
                            "HTTP/1.1 {status} Fixture\r\nLocation: {location}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        stream.write_all(response.as_bytes()).unwrap();
                        deadline = Instant::now() + Duration::from_millis(150);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        requests
    });
    let result = request(cfg);
    (result, server.join().unwrap())
}

fn fixture_completion(cfg: &AiConfig) -> Result<String, String> {
    chat_completion(cfg, "fixture system", "fixture user")
}

fn privacy_fixture_config() -> AiConfig {
    AiConfig {
        revision: 0,
        enabled: true,
        provider: "ollama".into(),
        base_url: String::new(),
        api_key: String::new(),
        model: "privacy-fixture".into(),
        allow_cloud_paths: false,
    }
}

#[test]
fn production_ai_transport_refuses_all_redirects_without_contacting_target() {
    for provider in ["anthropic", "openai"] {
        for status in [301, 302, 303, 307, 308] {
            for cross_origin in [false, true] {
                let mut cfg = AiConfig::default();
                cfg.enabled = true;
                cfg.provider = provider.into();
                cfg.api_key = "fixture-redirect-secret".into();
                let (result, requests) =
                    capture_ai_request(&mut cfg, status, cross_origin, "ok", fixture_completion);
                assert_eq!(result, Err("ai redirect refused".into()));
                assert_eq!(
                    requests.len(),
                    1,
                    "a redirect must not make another request"
                );
                assert!(!requests[0].0, "only the initial source may be contacted");
                let header = if provider == "anthropic" {
                    "x-api-key:"
                } else {
                    "authorization:"
                };
                assert!(
                    requests[0].1.to_ascii_lowercase().contains(header),
                    "the test must reach an authenticated production request"
                );
            }
        }
    }
}

#[test]
fn switching_to_local_provider_does_not_send_the_old_cloud_key() {
    let mut cfg = AiConfig::default();
    cfg.enabled = true;
    cfg.api_key = "fixture-old-cloud-secret".into();
    replace_credential(&mut cfg, "ollama", "http://127.0.0.1:11434/v1", None);
    cfg.provider = "ollama".into();
    let (result, requests) = capture_ai_request(&mut cfg, 200, false, "ok", fixture_completion);
    assert_eq!(result, Ok("ok".into()));
    assert_eq!(requests.len(), 1);
    assert!(!requests[0]
        .1
        .to_ascii_lowercase()
        .contains("authorization:"));
    assert!(!requests[0].1.contains("fixture-old-cloud-secret"));
}

#[test]
fn credential_rebinding_is_persisted_and_reloads_without_the_old_key() {
    let root = crate::test_support::TempDir::new("ai-source-binding");
    let path = root.join("config.json");
    let mut cfg = AiConfig::default();
    cfg.api_key = "fixture-bound-key".into();
    save_config_file(&path, &cfg).unwrap();
    update_config_file(&path, |stored| {
        replace_credential(stored, "openai", "https://new.example.test/v1", None);
        stored.base_url = "https://new.example.test/v1".into();
    })
    .unwrap();
    assert!(load_config_file(&path).unwrap().api_key.is_empty());
}

#[test]
fn checked_config_transactions_preserve_unreadable_or_corrupt_originals() {
    let p = std::env::temp_dir().join(format!(
        "remova-config-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert!(load_config_file(&p).unwrap().api_key.is_empty());
    std::fs::write(&p, b"{ invalid secret config").unwrap();
    assert!(update_config_file(&p, |_| panic!("must not mutate corrupt config")).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), b"{ invalid secret config");
    std::fs::remove_file(&p).unwrap();
    std::fs::create_dir(&p).unwrap();
    assert!(load_config_file(&p).is_err());
    std::fs::remove_dir(&p).unwrap();
}

#[test]
#[cfg(windows)]
fn config_rejects_redirected_paths_and_preserves_legacy_settings() {
    let root = crate::test_support::TempDir::new("ai-config-paths");
    let existing = root.subdir("existing");
    let original = existing.join("ai-config.json");
    let mut legacy = AiConfig::default();
    legacy.api_key = "fixture-legacy-key".into();
    let bytes = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(&original, &bytes).unwrap();

    let junction = root.join("Remova");
    let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&existing)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "could not create the fixture junction"
    );
    let redirected = junction.join("ai-config.json");
    assert!(load_config_file(&redirected).is_err());
    assert!(
        update_config_file(&redirected, |_| panic!("must not load redirected settings")).is_err()
    );
    assert!(save_config_file(&redirected, &AiConfig::default()).is_err());
    assert_eq!(std::fs::read(&original).unwrap(), bytes);
    std::fs::remove_dir(&junction).unwrap();

    let linked_file = root.join("linked-config.json");
    std::fs::hard_link(&original, &linked_file).unwrap();
    assert!(load_config_file(&linked_file).is_err());
    assert!(save_config_file(&linked_file, &AiConfig::default()).is_err());
    assert_eq!(std::fs::read(&linked_file).unwrap(), bytes);
    std::fs::remove_file(&linked_file).unwrap();

    // Normal reads still perform the in-place DPAPI migration after the link is removed.
    assert_eq!(
        load_config_file(&original).unwrap().api_key,
        "fixture-legacy-key"
    );
    let stored: AiConfig = serde_json::from_slice(&std::fs::read(&original).unwrap()).unwrap();
    assert!(stored.api_key.starts_with(KEY_PREFIX));
}

#[test]
fn migration_and_updates_hold_one_transaction_lock() {
    let p = std::env::temp_dir().join(format!(
        "remova-config-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut old = AiConfig::default();
    old.api_key = "fixture-legacy-key".into();
    std::fs::write(&p, serde_json::to_vec(&old).unwrap()).unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let path = &p;
        scope.spawn(move || {
            update_config_file(path, |c| {
                assert!(CONFIG_REVISION.try_lock().is_err());
                ready_tx.send(()).unwrap();
                attempt_rx.recv().unwrap();
                c.model = "older transaction".into();
            })
            .unwrap()
        });
        scope.spawn(move || {
            ready_rx.recv().unwrap();
            attempt_tx.send(()).unwrap();
            update_config_file(path, |c| c.model = "new user settings".into()).unwrap();
        });
    });
    let config = load_config_file(&p).unwrap();
    assert_eq!(config.model, "new user settings");
    assert_eq!(config.api_key, "fixture-legacy-key");
    #[cfg(windows)]
    assert!(!std::fs::read_to_string(&p)
        .unwrap()
        .contains("fixture-legacy-key"));
    std::fs::remove_file(p).unwrap();
}

#[cfg(windows)]
#[test]
fn undecryptable_keys_abort_updates_and_preserve_ciphertext() {
    let p = std::env::temp_dir().join(format!(
        "remova-bad-key-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for key in ["dpapi:zz", "dpapi:00", "dpapi:"] {
        let mut config = AiConfig::default();
        config.api_key = key.into();
        let original = serde_json::to_vec(&config).unwrap();
        std::fs::write(&p, &original).unwrap();
        assert!(load_config_file(&p).is_err());
        let error =
            update_config_file(&p, |_| panic!("must not overwrite an unreadable key")).unwrap_err();
        assert!(error.starts_with("ai:config_read_failed"));
        assert_eq!(std::fs::read(&p).unwrap(), original);
    }
    std::fs::remove_file(p).unwrap();
}

fn captured_user_message(requests: &[(bool, String)]) -> String {
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].0);
    let (_, body) = requests[0].1.split_once("\r\n\r\n").unwrap();
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    json["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "user")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string()
}

fn candidates_in(prompt: &str) -> Vec<serde_json::Value> {
    let (_, payload) = prompt.split_once("\nCandidates:\n").unwrap();
    let (json, _) = payload.rsplit_once("\nReturn ").unwrap();
    serde_json::from_str(json).unwrap()
}

#[test]
fn real_explain_requests_omit_paths_by_default_and_gate_opt_in_paths() {
    let paths = [
        r"D:\CustomProfiles\ElsewherePrivate\LocalCache",
        r"\\PrivateServer\PrivateShare\NetworkCache",
        r"E:/Users/ProfilePrivate/AppData/Local/AllowedProduct",
    ];
    let inputs: Vec<_> = paths
        .iter()
        .map(|path| ExplainInput {
            path: path.to_string(),
            kind: "dir".into(),
            confidence: "confirmed".into(),
            risk: "medium".into(),
            reason: r#"Path found at "D:\Company Space, Punctuation Tail""#.into(),
            evidence_labels: vec![
                r"Evidence \\EvidenceServer\EvidenceShare\Sample".into(),
                r"%LOCALAPPDATA%\AliasSecret".into(),
                r"Folder\RelativeSecret".into(),
                "Matches the installed publisher".into(),
            ],
        })
        .collect();
    let mut cfg = privacy_fixture_config();
    let reply = r#"[{"id":3,"summary":"third","suggest_check":true},{"id":1,"summary":"first","suggest_check":false},{"id":2,"summary":"second","suggest_check":false}]"#;
    for share in [false, true] {
        cfg.allow_cloud_paths = share;
        let app = "Path privacy fixture";
        let (out, requests) = capture_ai_request(&mut cfg, 200, false, reply, |cfg| {
            explain_items(cfg, app, "Vendor", &inputs, "en")
        });
        let result = out.unwrap();
        assert_eq!(
            result
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            paths
        );
        assert_eq!(result[2].summary, "third");
        let prompt = captured_user_message(&requests);
        let payload = candidates_in(&prompt);
        for secret in [
            "ElsewherePrivate",
            "PrivateServer",
            "PrivateShare",
            "Company Space",
            "Punctuation Tail",
            "EvidenceServer",
            "EvidenceShare",
            "AliasSecret",
            "RelativeSecret",
            "ProfilePrivate",
        ] {
            assert!(!prompt.contains(secret), "request leaked {secret}");
        }
        assert!(prompt.contains("Matches the installed publisher"));
        assert!(payload[0].get("path").is_none());
        assert!(payload[1].get("path").is_none());
        if share {
            assert_eq!(
                payload[2]["path"],
                r"E:\Users\*\AppData\Local\AllowedProduct"
            );
        } else {
            assert!(!prompt.contains("AllowedProduct"));
            assert!(payload[2].get("path").is_none());
        }
    }
}

#[test]
fn real_report_and_intent_requests_keep_default_paths_local() {
    let input = ReportBriefInput {
        app_name: "Report privacy fixture".into(),
        deleted: 0,
        failed: 1,
        skipped: 0,
        aborted: false,
        restore_point_ok: false,
        backup_dir: r"C:\Users\ReportPrivate\secret\backup".into(),
        top_failed: vec![r"C:\Users\ReportPrivate\secret\failed.bin".into()],
    };
    let mut cfg = privacy_fixture_config();
    let (report, requests) = capture_ai_request(&mut cfg, 200, false, "ok", |cfg| {
        summarize_report(cfg, &input, "en")
    });
    assert_eq!(report.unwrap(), "ok");
    let prompt = captured_user_message(&requests);
    for private in [
        "ReportPrivate",
        "secret",
        "failed.bin",
        "Backup dir",
        "Failed samples",
    ] {
        assert!(!prompt.contains(private));
    }
    assert!(prompt.contains("Backup recorded: true"));

    cfg.allow_cloud_paths = true;
    let (report, requests) = capture_ai_request(&mut cfg, 200, false, "ok", |cfg| {
        summarize_report(cfg, &input, "en")
    });
    assert!(report.is_ok());
    let prompt = captured_user_message(&requests);
    assert!(prompt.contains(r"C:\Users\*\secret\failed.bin"));
    assert!(!prompt.contains("ReportPrivate"));

    let names = vec![
        r"\\IntentPrivate\Share\Product".into(),
        "Vendor Product".into(),
    ];
    let (intent, requests) =
        capture_ai_request(&mut cfg, 200, false, r#"{"action":"list"}"#, |cfg| {
            parse_nl_intent(cfg, r"Find D:IntentRelativeSecret", &names, "en")
        });
    assert_eq!(intent.unwrap().action, "list");
    let prompt = captured_user_message(&requests);
    assert!(!prompt.contains("IntentPrivate"));
    assert!(!prompt.contains("IntentRelativeSecret"));
    assert!(prompt.contains("Vendor Product"));
}

#[test]
fn opt_in_paths_are_limited_to_recognized_local_folders() {
    assert_eq!(
        cloud_path(r"E:/Users/İ/AppData/Local/厂商", true).unwrap(),
        r"E:\Users\*\AppData\Local\厂商"
    );
    assert_eq!(
        cloud_path(r"D:\Program Files\Vendor", true).unwrap(),
        r"D:\Program Files\Vendor"
    );
    assert!(cloud_path(r"D:\Program Files\Vendor", false).is_none());
    for path in [
        r"D:\ProfileRoot\Someone\Cache",
        r"\\server\share\Someone\Cache",
        r"\\?\D:\Users\Someone\Cache",
        r"D:\Users\..\Someone\Cache",
        r".\Someone\Cache",
        r"D:\ProgramData\file:stream",
    ] {
        assert!(cloud_path(path, true).is_none(), "{path}");
    }
    assert_eq!(scrub_cloud_text("Matches Acme"), "Matches Acme");
    assert_eq!(scrub_cloud_text("Look at C:relative"), "[path]");
    assert_eq!(scrub_cloud_text(r"See another\relative"), "[path]");
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
            assert_eq!(decrypt_stored_key(&wrapped).unwrap(), "sk-abc");
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
#[cfg(windows)]
#[test]
fn config_write_preserves_preexisting_hard_link() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = std::env::temp_dir().join(format!("remova-r23-ai-write-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ai.json");
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp_name);
    let sentinel = dir.join("sentinel");
    std::fs::write(&sentinel, "old partial").unwrap();
    std::fs::hard_link(&sentinel, &tmp).unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x4)
        .open(&tmp)
        .unwrap();
    write_config_file(&path, "new data").unwrap();
    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "old partial");
    assert!(tmp.exists());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new data");
    drop(held);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Rename-target failure (dest is a directory) must also clean the tmp —
/// mirrors the icon-cache failure test for the same write discipline.
#[cfg(windows)]
#[test]
fn config_write_rename_failure_removes_tmp() {
    let dir = std::env::temp_dir().join(format!("remova-r27-ai-write-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The publish target is a DIRECTORY: write succeeds, rename fails.
    let path = dir.join("ai.json");
    std::fs::create_dir_all(&path).unwrap();
    assert!(write_config_file(&path, "new data").is_err());
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "no tmp leftovers: {leftovers:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn explain_batches_rematch_candidate_ids_and_cache_every_result() {
    let cfg = AiConfig::default();
    let inputs: Vec<ExplainInput> = (0..25)
        .map(|i| ExplainInput {
            path: format!(r"C:\Users\a\AppData\Local\Vendor\StableProduct{i}"),
            kind: "dir".into(),
            confidence: "confirmed".into(),
            risk: if i == 0 { "high" } else { "medium" }.into(),
            reason: "cache regression".into(),
            evidence_labels: vec![],
        })
        .collect();
    let mut batches = vec![];
    let out = explain_items_with_completion(
        &cfg,
        "batching-cache-test",
        "Vendor",
        &inputs,
        "en",
        |user| {
            let payload = candidates_in(user);
            batches.push(payload.len());
            let echoed: Vec<_> = payload
                .iter()
                .rev()
                .map(|item| {
                    serde_json::json!({
                        "id": item["id"],
                        "summary": item["id"].to_string(),
                        "suggest_check": true,
                    })
                })
                .collect();
            Ok(serde_json::to_string(&echoed).unwrap())
        },
    )
    .unwrap();
    assert_eq!(batches, [12, 12, 1]);
    assert_eq!(out.len(), inputs.len());
    // the caller's order is the contract, even though the model
    // echoes each batch reversed.
    assert_eq!(
        out.iter().map(|o| o.path.as_str()).collect::<Vec<_>>(),
        inputs.iter().map(|i| i.path.as_str()).collect::<Vec<_>>()
    );
    for (index, input) in inputs.iter().enumerate() {
        let result = &out[index];
        assert_eq!(result.summary, (index % 12 + 1).to_string());
        assert_eq!(result.suggest_check, input.risk != "high");
    }
    let cached =
        explain_items_with_completion(&cfg, "batching-cache-test", "Vendor", &inputs, "en", |_| {
            panic!("all 25 explanations must hit cache, including the last batch")
        })
        .unwrap();
    assert_eq!(cached.len(), inputs.len());
    assert_eq!(
        cached.iter().map(|o| o.path.as_str()).collect::<Vec<_>>(),
        inputs.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(),
        "fully cached results must keep the input order"
    );
    assert!(cached
        .iter()
        .all(|item| out.iter().any(|v| v.path == item.path
            && v.summary == item.summary
            && v.suggest_check == item.suggest_check)));
}

#[test]
fn explain_ignores_repeated_and_unknown_ids() {
    let inputs: Vec<_> = ["a", "b"]
        .iter()
        .map(|name| ExplainInput {
            path: format!(r"C:\Users\{name}\AppData\Local\Vendor\StableCollision"),
            kind: "dir".into(),
            confidence: "suspected".into(),
            risk: "medium".into(),
            reason: "duplicate fixture".into(),
            evidence_labels: vec![],
        })
        .collect();
    let result = explain_items_with_completion(
        &AiConfig::default(),
        "duplicate-id-fixture",
        "Vendor",
        &inputs,
        "zh",
        |_| {
            Ok(r#"[
            {"id":2,"summary":"second","suggest_check":true},
            {"id":0,"summary":"invalid","suggest_check":false},
            {"id":2,"summary":"duplicate","suggest_check":false},
            {"id":4,"summary":"outside batch","suggest_check":false},
            {"id":1,"summary":"first","suggest_check":false}
        ]"#
            .into())
        },
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].path, inputs[0].path);
    assert_eq!(result[1].path, inputs[1].path);
    assert_eq!(result[1].summary, "second");
    assert!(!result[1].suggest_check);
}

#[test]
fn user_prompts_follow_the_ui_locale() {
    let input = ReportBriefInput {
        app_name: "Vendor".into(),
        deleted: 1,
        failed: 0,
        skipped: 0,
        aborted: false,
        restore_point_ok: true,
        backup_dir: r"C:\Backups\s1".into(),
        top_failed: vec![],
    };
    let cfg = AiConfig::default();
    let zh = report_user_prompt(&cfg, &input, "zh-CN");
    let en = report_user_prompt(&cfg, &input, "en-US");
    assert!(zh.starts_with("软件:"));
    assert!(en.starts_with("App:"));
    assert!(!en.contains('软'));

    let items = vec![ExplainInput {
        path: r"C:\Users\x\AppData\Local\Vendor\LocaleFixture".into(),
        kind: "dir".into(),
        confidence: "confirmed".into(),
        risk: "low".into(),
        reason: "locale fixture".into(),
        evidence_labels: vec![],
    }];
    let mut seen = String::new();
    let _ =
        explain_items_with_completion(&cfg, "locale-prompt-test", "Vendor", &items, "en", |user| {
            seen = user.to_string();
            Ok("[]".into())
        })
        .unwrap();
    assert!(seen.starts_with("App:"));
    assert!(seen.contains("Candidates"));
    assert!(!seen.contains("候选"));

    let risk = RiskBriefInput {
        app_name: "Vendor".into(),
        publisher: "Acme".into(),
        action: "cleanup".into(),
        item_count: 3,
        kind_counts: HashMap::from([("dir".to_string(), 3usize)]),
        has_service: false,
        has_run_key: false,
        has_shared_hint: false,
    };
    assert!(risk_user_prompt(&risk, "en").starts_with("App:"));
    assert!(risk_user_prompt(&risk, "zh").starts_with("软件:"));
    assert!(!risk_user_prompt(&risk, "en").contains("软件"));
    assert!(!risk_user_prompt(&risk, "zh").contains("App:"));
    let intent_en = intent_user_prompt("uninstall Foo", "Foo | Bar", "en");
    assert!(intent_en.starts_with("User instruction:"));
    assert!(!intent_en.contains("用户指令"));
    let intent_zh = intent_user_prompt("卸载 Foo", "Foo", "zh");
    assert!(intent_zh.starts_with("用户指令："));
    assert!(!intent_zh.contains("User instruction"));
}

#[test]
fn nl_intent_size_filter_uses_the_serde_field_name() {
    assert!(INTENT_SYSTEM_EN.contains("size_gt_kb"));
    assert!(!INTENT_SYSTEM_EN.contains("size_gt_KB"));
    let lower: NlIntent =
        serde_json::from_str(r#"{"action":"list","filter":{"size_gt_kb":500}}"#).unwrap();
    assert_eq!(lower.filter.size_gt_kb, Some(500));
    // Older prompts / models may still emit the upper-case spelling; the
    // alias keeps those filters instead of silently dropping them.
    let alias: NlIntent =
        serde_json::from_str(r#"{"action":"list","filter":{"size_gt_KB":500}}"#).unwrap();
    assert_eq!(alias.filter.size_gt_kb, Some(500));
}

#[test]
fn saved_configuration_separates_in_flight_cache_entries() {
    let root = crate::test_support::TempDir::new("remova-ai-cache");
    let path = root.join("ai-config.json");
    update_config_file(&path, |config| {
        config.enabled = true;
        config.model = "fixture-a".into();
    })
    .unwrap();
    let old = load_config_snapshot(&path).unwrap();

    update_config_file(&path, |config| {
        config.model = "fixture-b".into();
        config.base_url = "http://localhost:12345/v1".into();
    })
    .unwrap();
    let current = load_config_snapshot(&path).unwrap();
    assert_eq!(current.model, "fixture-b");
    for request in ["explain", "risk", "report"] {
        let hash = fnv1a64(request);
        let earlier = scoped_cache_key(&old, hash);
        let later = scoped_cache_key(&current, hash);
        assert_ne!(earlier, later);
        cache_put(later, "current reply".into());
        cache_put(earlier, "late old reply".into());
        assert_eq!(cache_get(later).as_deref(), Some("current reply"));
    }
    assert!(!std::fs::read_to_string(&path).unwrap().contains("revision"));
}
