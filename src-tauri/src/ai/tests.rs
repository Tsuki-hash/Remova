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
fn capture_ai_request(
    cfg: &mut AiConfig,
    response_status: u16,
    cross_origin: bool,
) -> (Result<String, String>, Vec<(bool, String)>) {
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
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        let mut deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            for (is_target, listener) in [(false, &source), (true, &target)] {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = read_request(&mut stream);
                        let first = requests.is_empty();
                        requests.push((is_target, request));
                        let body = r#"{"content":[{"text":"ok"}],"choices":[{"message":{"content":"ok"}}]}"#;
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
    let result = chat_completion(cfg, "fixture system", "fixture user");
    (result, server.join().unwrap())
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
                let (result, requests) = capture_ai_request(&mut cfg, status, cross_origin);
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
    let (result, requests) = capture_ai_request(&mut cfg, 200, false);
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
                assert!(CONFIG_LOCK.try_lock().is_err());
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

#[test]
fn report_prompt_respects_path_privacy_without_network_requests() {
    let input = ReportBriefInput {
        app_name: "Vendor".into(),
        deleted: 0,
        failed: 1,
        skipped: 0,
        aborted: false,
        restore_point_ok: false,
        backup_dir: r"C:\Users\PrivacyUser\private\backup\session".into(),
        top_failed: vec![r"C:\Users\PrivacyUser\private\product\failed.bin".into()],
    };
    let mut cfg = AiConfig::default();
    let scrubbed = report_user_prompt(&cfg, &input, "zh");
    assert!(!scrubbed.contains("PrivacyUser"));
    assert!(!scrubbed.contains("private"));
    assert!(!scrubbed.contains(r"C:\Users"));
    assert!(scrubbed.contains(r"product\failed.bin"));
    cfg.allow_cloud_paths = true;
    let allowed = report_user_prompt(&cfg, &input, "zh");
    assert!(allowed.contains(r"C:\Users\*\private\product\failed.bin"));
    assert!(!allowed.contains("PrivacyUser"));
    assert_ne!(allowed, scrubbed);
}

#[test]
fn sanitize_strips_username() {
    assert_eq!(
        sanitize_path(r"C:\Users\İ\AppData\Local\厂商", false),
        r"AppData\厂商"
    );
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
    assert!(s.contains("[path]"), "{s}");
    // Non-path free text is preserved.
    assert_eq!(
        scrub_cloud_text("Shell/Classes leftover: Foo"),
        "Shell/Classes leftover: Foo"
    );
    // The same helper is applied to `app_name` and `publisher` before a
    // cloud request; both can carry a path from a hostile registry value.
    let app = scrub_cloud_text(r"C:\Users\alice\AppData\Local\Acme\acme.exe");
    assert!(!app.contains("alice"), "{app}");
    let publisher = scrub_cloud_text(r"\\fileserver\share\Publisher");
    assert!(!publisher.contains("fileserver"), "{publisher}");
}

#[test]
fn explain_payload_redacts_spaced_absolute_paths_without_network() {
    let cfg = AiConfig::default();
    let secrets = [
        r"D:\Company Files\Sensitive\Private\LocalCache",
        r"\\Company Server\Private Share\Sensitive\LocalCache",
        r"D:/Company Files/Sensitive/Private/LocalCache",
    ];
    let inputs: Vec<ExplainInput> = secrets
        .iter()
        .enumerate()
        .map(|(i, secret)| ExplainInput {
            path: format!(r"D:\Vendor\PrivacyFixture{i}"),
            kind: "dir".into(),
            confidence: "suspected".into(),
            risk: "medium".into(),
            reason: format!("Product dir under {secret}; check ownership"),
            evidence_labels: vec![format!("Found under \"{secret}\", candidate")],
        })
        .collect();
    let mut calls = 0;
    explain_items_with_completion(
        &cfg,
        "privacy-spaced-paths-fixture",
        "Vendor",
        &inputs,
        "zh",
        |user| {
            calls += 1;
            for secret in secrets {
                assert!(!user.contains(secret));
            }
            for private in ["Company", "Sensitive", "Private", "LocalCache"] {
                assert!(!user.contains(private), "request leaked {private}");
            }
            assert!(user.contains("[path]"));
            assert!(user.contains("check ownership"));
            assert!(user.contains("candidate"));
            Ok("[]".into())
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
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
/// `sanitize_path` is deliberately NOT idempotent for the
/// common AppData shape — the explain rematch must therefore consult the
/// strings actually sent to the model instead of re-sanitizing the echo.
/// the path gate only applies to tokens that look like paths; plain
/// product names must keep going through `scrub_cloud_text` untouched.
#[test]
fn path_token_detection_covers_drive_letter_and_unc_shapes() {
    for s in [
        r"C:\Program Files\App",
        "C:/Tools/app",
        r"\\server\share\app",
        "//server/share/app",
        " D:\\x ",
    ] {
        assert!(looks_like_path_token(s), "{s:?} must look like a path");
    }
    for s in [
        "Notepad++",
        "微信输入法",
        "",
        "C:",
        "C:relative",
        "Microsoft .NET",
        r".\relative",
    ] {
        assert!(!looks_like_path_token(s), "{s:?} must not look like a path");
    }
}

#[test]
fn sanitize_path_is_not_idempotent_for_appdata_shapes() {
    let once = sanitize_path(r"C:\Users\a\AppData\Local\Vendor\Product", false);
    assert_eq!(once, r"AppData\Vendor\Product");
    assert_ne!(sanitize_path(&once, false), once);
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
fn explain_batches_rematch_appdata_echoes_and_cache_every_result() {
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
        "zh",
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
    // the caller's order is the contract, even though the model
    // echoes each batch reversed.
    assert_eq!(
        out.iter().map(|o| o.path.as_str()).collect::<Vec<_>>(),
        inputs.iter().map(|i| i.path.as_str()).collect::<Vec<_>>()
    );
    for input in &inputs {
        let result = out.iter().find(|item| item.path == input.path).unwrap();
        assert_eq!(result.summary, sanitize_path(&input.path, false));
        assert_eq!(result.suggest_check, input.risk != "high");
    }
    let cached =
        explain_items_with_completion(&cfg, "batching-cache-test", "Vendor", &inputs, "zh", |_| {
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
fn explain_duplicate_sanitized_echoes_consume_originals_once() {
    let inputs: Vec<ExplainInput> = ["a", "b"]
        .iter()
        .map(|name| ExplainInput {
            path: format!(r"C:\Users\{name}\AppData\Local\Vendor\StableCollision"),
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
        "collision-cache-test",
        "Vendor",
        &inputs,
        "zh",
        |_| Ok(serde_json::to_string(&outputs).unwrap()),
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].path, inputs[0].path);
    assert_eq!(result[1].path, inputs[1].path);
    assert_eq!(result[1].summary, "explanation-1");
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
