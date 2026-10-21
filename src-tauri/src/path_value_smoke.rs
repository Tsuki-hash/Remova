//! PATH / value.reg chain smoke with injectable PATH mock (never touches system PATH).

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
        service_bound: false,
        user_data: false,
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: None,
    }
}

fn tmp_session(tag: &str) -> crate::test_support::TempDir {
    let dir = crate::test_support::TempDir::new(&format!("s08_{tag}"));
    dir.subdir("files");
    dir.subdir("registry");
    dir
}

#[test]
fn path_backup_scrub_restore_merge_chain() {
    let session = tmp_session("chain");
    let _seal_store = crate::path_seal::TestStore::new(session.join("seals"));
    let _lock = path_mock::lock_mock();
    path_mock::install(r"C:\Windows;C:\Vendor\Tool;C:\KeepMe", r"C:\Windows");
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

    // Restore merges missing segment only —never whole-env overwrite.
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
    let session = tmp_session("valuereg");
    let _seal_store = crate::path_seal::TestStore::new(session.join("seals"));
    let _lock = path_mock::lock_mock();
    path_mock::clear();
    // Non-existent Run value: export_reg_value returns Ok(false) →backup must fail.
    let item = CleanupItem {
        path: r"HKCU\Software\RemovaS08NoSuchKey|NoSuchValue".into(),
        kind: ItemKind::Registry,
        score: 40,
        confidence: Confidence::Suspected,
        risk: RiskLevel::Medium,
        reason: "run".into(),
        evidence: vec![],
        shared: false,
        service_bound: false,
        user_data: false,
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: None,
    };
    let err = crate::backup::backup_item(&item, &session).unwrap_err();
    assert!(
        err.contains("backup:registry") || err.contains("backup:value_reg"),
        "expected structured backup error, got {err}"
    );
}

#[test]
fn restore_prefers_value_reg_path_when_present() {
    // drive the real import-target selection (`pick_import_target`) over
    // the four backup-entry layouts instead of asserting a file we just wrote.
    let session = tmp_session("restore_layout");
    // Windows forbids '|' in file names —use the same sanitization style as backup::safe_name.
    let dir = session.join("registry").join("HKCU__Software__App_Run");
    fs::create_dir_all(&dir).unwrap();
    // Neither file → no import target.
    assert!(crate::restore::pick_import_target(&dir).is_none());
    // export.reg only → whole-key export is the target.
    fs::write(
        dir.join("export.reg"),
        b"Windows Registry Editor Version 5.00\n",
    )
    .unwrap();
    assert_eq!(
        crate::restore::pick_import_target(&dir),
        Some(dir.join("export.reg"))
    );
    // Both present → the single-value export wins (Run values restore exactly).
    fs::write(
        dir.join("value.reg"),
        b"Windows Registry Editor Version 5.00\n",
    )
    .unwrap();
    assert_eq!(
        crate::restore::pick_import_target(&dir),
        Some(dir.join("value.reg"))
    );
}
