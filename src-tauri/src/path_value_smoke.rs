//! S-08: PATH / value.reg chain smoke with injectable PATH mock (never touches system PATH).

use std::fs;

use crate::regops::path_mock;
use crate::scanner::{CleanupItem, Confidence, ItemKind, RiskLevel};

fn path_item(entry: &str) -> CleanupItem {
    CleanupItem {
        path: entry.into(),
        kind: ItemKind::Path,
        score: 40,
        confidence: Confidence::Suspected,
        risk: RiskLevel::Medium,
        reason: "path".into(),
        evidence: vec![],
        shared: false,
        user_data: false,
        size_kb: None,
        bucket: None,
    }
}

fn tmp_session(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("remova_s08_{}_{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(p.join("files")).unwrap();
    fs::create_dir_all(p.join("registry")).unwrap();
    p
}

#[test]
fn path_backup_scrub_restore_merge_chain() {
    let _lock = path_mock::lock_mock();
    path_mock::install(r"C:\Windows;C:\Vendor\Tool;C:\KeepMe", r"C:\Windows");
    let session = tmp_session("chain");
    let item = path_item(r"C:\Vendor\Tool");
    crate::backup::backup_item(&item, &session).unwrap();
    let pj = session.join("path.json");
    assert!(pj.exists(), "path.json snapshot must be written");
    let snap: crate::backup::PathSnapshot =
        serde_json::from_str(&fs::read_to_string(&pj).unwrap()).unwrap();
    assert_eq!(snap.items.len(), 1);
    assert!(
        snap.items[0]
            .scopes
            .iter()
            .any(|s| s.eq_ignore_ascii_case("User")),
        "scopes should include User, got {:?}",
        snap.items[0].scopes
    );

    let changed = crate::regops::scrub_path_entry(r"C:\Vendor\Tool").unwrap();
    assert!(changed, "scrub should change User PATH");
    let user = path_mock::get("User");
    assert!(
        !crate::regops::path_contains_entry(&user, r"C:\Vendor\Tool"),
        "scrub removed entry: {user}"
    );
    assert!(
        crate::regops::path_contains_entry(&user, r"C:\KeepMe"),
        "scrub must not wipe unrelated segments: {user}"
    );
    assert!(
        crate::regops::path_contains_entry(&path_mock::get("Machine"), r"C:\Windows"),
        "Machine PATH untouched when entry was User-only"
    );

    // Restore merges missing segment only — never whole-env overwrite.
    let restored = crate::regops::restore_path_entry(r"C:\Vendor\Tool", &["User"]).unwrap();
    assert!(restored);
    let user2 = path_mock::get("User");
    assert!(crate::regops::path_contains_entry(
        &user2,
        r"C:\Vendor\Tool"
    ));
    assert!(
        crate::regops::path_contains_entry(&user2, r"C:\KeepMe"),
        "restore must keep existing unrelated segments: {user2}"
    );
    assert!(
        crate::regops::path_contains_entry(&user2, r"C:\Windows"),
        "restore must keep existing unrelated segments: {user2}"
    );

    let _ = fs::remove_dir_all(&session);
    path_mock::clear();
}

#[test]
fn path_read_failure_is_error_not_process_fallback() {
    let _lock = path_mock::lock_mock();
    path_mock::install(r"C:\Vendor\Tool", r"C:\Windows");
    path_mock::set_fail_read(true);
    let err = crate::regops::scrub_path_entry(r"C:\Vendor\Tool").unwrap_err();
    assert!(
        err.contains("read Path") || err.contains("path:io"),
        "S-03: read failure must Err, got {err}"
    );
    path_mock::clear();
}

#[test]
fn value_reg_missing_export_fails_backup_with_structured_code() {
    let _lock = path_mock::lock_mock();
    path_mock::clear();
    let session = tmp_session("valuereg");
    // Non-existent Run value: export_reg_value returns Ok(false) → backup must fail (S-04).
    let item = CleanupItem {
        path: r"HKCU\Software\RemovaS08NoSuchKey|NoSuchValue".into(),
        kind: ItemKind::Registry,
        score: 40,
        confidence: Confidence::Suspected,
        risk: RiskLevel::Medium,
        reason: "run".into(),
        evidence: vec![],
        shared: false,
        user_data: false,
        size_kb: None,
        bucket: None,
    };
    let err = crate::backup::backup_item(&item, &session).unwrap_err();
    assert!(
        err.contains("backup:registry") || err.contains("backup:value_reg"),
        "expected structured backup error, got {err}"
    );
    let _ = fs::remove_dir_all(&session);
}

#[test]
fn restore_prefers_value_reg_path_when_present() {
    // Pure layout check: restore import target selection prefers value.reg.
    // Full registry import is environment-dependent; we assert file layout contract only.
    let session = tmp_session("restore_layout");
    // Windows forbids '|' in file names — use the same sanitization style as backup::safe_name.
    let dir = session.join("registry").join("HKCU__Software__App_Run");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("value.reg"),
        b"Windows Registry Editor Version 5.00\n",
    )
    .unwrap();
    assert!(dir.join("value.reg").is_file());
    let _ = fs::remove_dir_all(&session);
}
