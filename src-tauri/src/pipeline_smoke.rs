//! Full-pipeline smoke test: dry-run → backup → delete → restore.
//! Uses a temp directory only; never touches real uninstall keys or ProgramData backup root
//! except through explicit session paths we create under temp.

use std::fs;
use std::path::PathBuf;

use crate::apps::InstalledApp;
use crate::executor::{run_cleanup_dry, run_full_cleanup, FullCleanupOptions};
use crate::scanner::{CleanupItem, Confidence, Evidence, ItemKind, RiskLevel};

fn tmp_root() -> PathBuf {
    let p = std::env::temp_dir().join(format!("remova_pipeline_{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn fake_app(name: &str, loc: &str) -> InstalledApp {
    InstalledApp {
        name: name.into(),
        version: "1.0.0".into(),
        publisher: "Test Vendor".into(),
        install_location: loc.into(),
        uninstall_string: String::new(),
        quiet_uninstall_string: String::new(),
        source: "HKLM64".into(),
        registry_key: format!(
            r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{{{name}}}"
        ),
        estimated_size_kb: 0,
        install_date: String::new(),
        display_icon: String::new(),
    }
}

fn file_item(path: &str) -> CleanupItem {
    CleanupItem {
        path: path.into(),
        kind: ItemKind::Dir,
        score: 90,
        confidence: Confidence::Confirmed,
        risk: RiskLevel::Low,
        reason: "install location".into(),
        evidence: vec![Evidence {
            code: "install_loc".into(),
            label: "Install location".into(),
            weight: 90,
            detail: String::new(),
        }],
    }
}

#[test]
fn pipeline_dry_run_backup_delete_restore() {
    let root = tmp_root();
    let app_dir = root.join("FakeApp");
    fs::create_dir_all(app_dir.join("bin")).unwrap();
    fs::write(app_dir.join("bin").join("app.exe"), b"fake-binary").unwrap();
    fs::write(app_dir.join("readme.txt"), b"hello remova").unwrap();
    let loc = app_dir.to_string_lossy().to_string();

    let app = fake_app("RemovaPipelineSmoke", &loc);
    let items = vec![file_item(&loc)];

    // 1) Dry-run: planned, nothing deleted
    let dry = run_cleanup_dry(&app.name, &items);
    assert!(dry.dry_run);
    assert_eq!(dry.deleted_planned, 1);
    assert_eq!(dry.skipped, 0);
    assert!(app_dir.exists(), "dry-run must not delete");

    // 2) Full cleanup with backup (skip official uninstaller — no uninstall string)
    let report = run_full_cleanup(
        &app,
        &items,
        &FullCleanupOptions {
            dry_run: false,
            skip_official_uninstall: true,
            backup_enabled: true,
        },
    );
    assert!(!report.aborted, "cleanup aborted: {:?}", report.errors);
    assert_eq!(report.deleted, 1);
    assert_eq!(report.failed, 0);
    assert!(!app_dir.exists(), "dir should be deleted after cleanup");
    assert!(
        !report.backup_dir.is_empty(),
        "backup_dir should be recorded"
    );

    // 3) Restore from the recorded session
    let session = PathBuf::from(&report.backup_dir);
    assert!(
        session.is_dir(),
        "backup session missing: {}",
        report.backup_dir
    );
    let msgs = crate::restore::restore_session(&session).expect("restore failed");
    assert!(msgs.iter().any(|m| m.contains("restored")));
    assert!(app_dir.join("bin").join("app.exe").exists(), "exe restored");
    assert_eq!(
        fs::read_to_string(app_dir.join("readme.txt")).unwrap(),
        "hello remova"
    );

    // cleanup temp (and the session we created under ProgramData for this smoke run)
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&session);
}

#[test]
fn pipeline_skips_protected_and_shallow_paths() {
    let items = vec![
        CleanupItem {
            path: r"C:\".into(),
            kind: ItemKind::Dir,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
        },
        CleanupItem {
            path: r"C:\Windows".into(),
            kind: ItemKind::Dir,
            score: 90,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "t".into(),
            evidence: vec![],
        },
    ];
    let dry = run_cleanup_dry("Protected", &items);
    assert_eq!(dry.deleted_planned, 0);
    assert_eq!(dry.skipped, 2);
}

#[test]
fn pipeline_empty_items_aborts() {
    let app = fake_app("Empty", r"C:\Program Files\NoSuchApp");
    let report = run_full_cleanup(
        &app,
        &[],
        &FullCleanupOptions {
            dry_run: false,
            skip_official_uninstall: true,
            backup_enabled: false,
        },
    );
    assert!(report.aborted);
    assert_eq!(report.deleted, 0);
}
