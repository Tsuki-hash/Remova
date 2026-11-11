//! Publication fault injection. All writes/deletes stay in owned temporary trees.
use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fault {
    FirstEntry,
    MiddleEntry,
    Copy,
    OmitAll,
    CorruptCopy,
    Fallback,
}
thread_local! {
    static FAULT: Cell<Option<Fault>> = const { Cell::new(None) };
    static REACHED: Cell<u32> = const { Cell::new(0) };
}
struct FaultGuard;
impl FaultGuard {
    fn new(fault: Fault) -> Self {
        FAULT.with(|v| assert!(v.replace(Some(fault)).is_none()));
        REACHED.with(|v| v.set(0));
        Self
    }
}
impl Drop for FaultGuard {
    fn drop(&mut self) {
        FAULT.with(|v| v.set(None));
    }
}
fn injected() -> std::io::Error {
    std::io::Error::other("fixture publication I/O")
}
struct FixtureIo(Fault);
impl PublishIo for FixtureIo {
    fn entries(
        &mut self,
        private: &Path,
    ) -> std::io::Result<Box<dyn Iterator<Item = std::io::Result<PathBuf>>>> {
        REACHED.with(|v| v.set(v.get() + 1));
        let mut entries = fs::read_dir(private)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort();
        let mut results: Vec<_> = entries.into_iter().map(Ok).collect();
        match self.0 {
            Fault::FirstEntry => results.insert(0, Err(injected())),
            Fault::MiddleEntry => results.insert(1, Err(injected())),
            Fault::OmitAll => results.clear(),
            _ => {}
        }
        Ok(Box::new(results.into_iter()))
    }
    fn rename(&mut self, src: &Path, dest: &Path) -> std::io::Result<()> {
        match self.0 {
            Fault::Copy | Fault::CorruptCopy | Fault::Fallback => Err(injected()),
            _ => NativePublishIo.rename(src, dest),
        }
    }
    fn copy(&mut self, src: &Path, dest: &Path) -> std::io::Result<()> {
        if self.0 == Fault::Copy {
            return Err(injected());
        }
        NativePublishIo.copy(src, dest)?;
        if self.0 == Fault::CorruptCopy && dest.file_name().is_some_and(|s| s == "files") {
            // Return success after truncating an actual copied nested payload.
            let map: std::collections::BTreeMap<String, String> =
                serde_json::from_slice(&fs::read(dest.join("path_map.json"))?)?;
            let rel = map.keys().next().unwrap();
            fs::write(dest.join(rel).join("nested/payload.bin"), b"truncated")?;
        }
        Ok(())
    }
}
pub(super) fn with_io<T>(f: impl FnOnce(&mut dyn PublishIo) -> T) -> T {
    match FAULT.with(Cell::get) {
        Some(fault) => f(&mut FixtureIo(fault)),
        None => f(&mut NativePublishIo),
    }
}

#[cfg(windows)]
fn item(source: &Path) -> CleanupItem {
    use crate::scanner::{Confidence, Evidence, RiskLevel};
    CleanupItem {
        path: source.to_string_lossy().into_owned(),
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
        shared: false,
        service_bound: false,
        user_data: false,
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: None,
    }
}
#[cfg(windows)]
fn source(root: &Path) -> PathBuf {
    let source = root.join("FixtureApp");
    fs::create_dir_all(source.join("nested/empty")).unwrap();
    fs::write(source.join("nested/payload.bin"), b"complete backup bytes").unwrap();
    source
}
#[cfg(windows)]
fn app(source: &Path) -> crate::apps::InstalledApp {
    crate::apps::InstalledApp {
        name: "PublicationFixtureApp".into(),
        version: "1".into(),
        publisher: "Fixture".into(),
        install_location: source.to_string_lossy().into_owned(),
        uninstall_string: String::new(),
        quiet_uninstall_string: String::new(),
        source: "HKLM64".into(),
        registry_key: String::new(),
        estimated_size_kb: 0,
        install_date: String::new(),
        display_icon: String::new(),
    }
}

#[test]
#[cfg(windows)]
fn cleanup_with_backup_preserves_files_and_directories_with_named_streams() {
    let _env = lock_backup_env();
    let root = crate::test_support::TempDir::new("named-backup-streams");
    let _store = crate::path_seal::TestStore::new(root.join("seals"));
    let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", root.join("Backup"));
    let source = source(&root);
    let payload = source.join("nested/payload.bin");
    for (owner, selected, suffix) in [
        (&payload, &payload, ":FixtureMetadata"),
        (&payload, &source, ":Zone.Identifier"),
        (&source, &source, ":FixtureMetadata"),
    ] {
        let mut stream = owner.as_os_str().to_os_string();
        stream.push(suffix);
        let stream = PathBuf::from(stream);
        fs::write(&stream, b"preserve this stream").unwrap();
        let mut selected_item = item(selected);
        if selected == &payload {
            selected_item.kind = ItemKind::File;
        }
        let report = crate::executor::run_full_cleanup(
            &app(&source),
            &[selected_item],
            &crate::executor::FullCleanupOptions {
                dry_run: false,
                skip_official_uninstall: true,
                backup_enabled: true,
                restore_point: false,
                cleanup_source: None,
            },
        );
        assert!(report.aborted && report.deleted == 0, "{report:?}");
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("named data streams")),
            "{:?}",
            report.errors
        );
        assert!(crate::path_seal::verified_seal(Path::new(&report.backup_dir)).is_err());
        assert_eq!(fs::read(&payload).unwrap(), b"complete backup bytes");
        assert_eq!(fs::read(&stream).unwrap(), b"preserve this stream");
        fs::remove_file(&stream).unwrap();
    }
}

#[test]
#[cfg(windows)]
fn backup_keeps_a_file_when_its_contents_cannot_be_opened() {
    use std::os::windows::fs::OpenOptionsExt;

    let _env = lock_backup_env();
    let root = crate::test_support::TempDir::new("backup-locked-source");
    let _store = crate::path_seal::TestStore::new(root.join("seals"));
    let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", root.join("Backup"));
    let source = source(&root);
    let file = source.join("nested/payload.bin");
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    let session = create_session("locked-source").unwrap();
    let (ok, fail, _) = backup_items(&[item(&source)], &session);
    assert_eq!((ok, fail), (0, 1));
    assert!(crate::path_seal::verified_seal(&session).is_err());
    drop(lock);
    assert_eq!(fs::read(file).unwrap(), b"complete backup bytes");
}

#[test]
#[cfg(windows)]
fn backup_and_restore_use_the_same_limit_for_each_payload_tree() {
    let limit = crate::path_seal::MAX_PAYLOAD_NODES;
    let first = std::iter::repeat_n("files/first/entry", limit);
    let two_roots = first.clone().chain(["files/second"]);
    validate_payload_capacity(two_roots).unwrap();
    assert!(validate_payload_capacity(first.chain(["files/first/last"])).is_err());
}

#[test]
#[cfg(windows)]
fn oversized_seal_is_rejected_before_backup_publication() {
    let root = crate::test_support::TempDir::new("seal-capacity");
    let stage = root.subdir("stage");
    let session = root.subdir("1700000000_capacity");
    fs::create_dir_all(stage.join("files")).unwrap();
    fs::create_dir_all(stage.join("registry")).unwrap();
    let targets = (0..1000)
        .map(|i| {
            (
                format!("target-{i}"),
                format!(r"C:\Fixture\{i}\{}", "x".repeat(180)),
            )
        })
        .collect();
    let error = publish_verified_stage(
        &stage,
        &session,
        &targets,
        &std::collections::BTreeMap::new(),
        |_| panic!("an oversized seal must be rejected before publication"),
    )
    .unwrap_err();
    assert_eq!(error, "seal:v2_input_too_large");
    assert_eq!(fs::read_dir(&session).unwrap().count(), 0);
}

#[test]
#[cfg(windows)]
fn publication_faults_fail_backup_and_abort_real_pipeline_without_deleting_sources() {
    let _env = lock_backup_env();
    for fault in [
        Fault::FirstEntry,
        Fault::MiddleEntry,
        Fault::Copy,
        Fault::OmitAll,
        Fault::CorruptCopy,
    ] {
        let root = crate::test_support::TempDir::new("publication-fault");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", root.join("Backup"));
        let source = source(&root);
        let item = item(&source);
        let _fault = FaultGuard::new(fault);
        crate::executor::begin_cleanup_control();
        let session = create_session("direct").unwrap();
        let (ok, fail, errors) = backup_items(std::slice::from_ref(&item), &session);
        assert_eq!(ok, 0, "{fault:?}: {errors:?}");
        assert!(fail > 0, "{fault:?}");
        let expected_error = match fault {
            Fault::FirstEntry | Fault::MiddleEntry => {
                "backup publish enumeration: fixture publication I/O"
            }
            Fault::Copy => "backup publish copy failed: fixture publication I/O",
            Fault::OmitAll | Fault::CorruptCopy => "backup publication inventory mismatch",
            Fault::Fallback => unreachable!(),
        };
        assert!(
            errors.iter().any(|e| e.contains(expected_error)),
            "{fault:?}: {errors:?}"
        );
        assert!(crate::path_seal::verified_seal(&session).is_err());
        assert!(!root
            .join("seals")
            .join(format!(
                "{}.seal.json",
                session.file_name().unwrap().to_str().unwrap()
            ))
            .exists());
        assert_eq!(REACHED.with(Cell::get), 1, "real publish must be reached");
        // Production pipeline; no restore point, registry/PATH rows or vendor command.
        // Its ignore-list load is read-only; no host configuration is changed.
        let report = crate::executor::run_full_cleanup(
            &app(&source),
            &[item],
            &crate::executor::FullCleanupOptions {
                dry_run: false,
                skip_official_uninstall: true,
                backup_enabled: true,
                restore_point: false,
                cleanup_source: None,
            },
        );
        assert_eq!(
            REACHED.with(Cell::get),
            2,
            "{fault:?}: pipeline did not reach publication: {:?}",
            report.errors
        );
        assert!(report.aborted, "{fault:?}: {report:?}");
        assert_eq!(report.deleted, 0);
        assert!(
            report.errors.iter().any(|e| e.contains(expected_error)),
            "{:?}",
            report.errors
        );
        assert!(crate::path_seal::verified_seal(Path::new(&report.backup_dir)).is_err());
        assert_eq!(
            fs::read(source.join("nested/payload.bin")).unwrap(),
            b"complete backup bytes"
        );
    }
}

#[test]
#[cfg(windows)]
fn publication_native_rename_and_fallback_copy_restore_complete_tree() {
    let _env = lock_backup_env();
    for fallback in [false, true] {
        let root = crate::test_support::TempDir::new("publication-restore");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", root.join("Backup"));
        let source = source(&root);
        let _fault = fallback.then(|| FaultGuard::new(Fault::Fallback));
        crate::executor::begin_cleanup_control();
        let session = create_session("complete").unwrap();
        let (ok, fail, errors) = backup_items(&[item(&source)], &session);
        assert_eq!((ok, fail), (1, 0), "fallback={fallback}: {errors:?}");
        crate::path_seal::verified_seal(&session).unwrap();
        fs::remove_dir_all(&source).unwrap();
        crate::restore::restore_session(&session).unwrap();
        assert_eq!(
            fs::read(source.join("nested/payload.bin")).unwrap(),
            b"complete backup bytes"
        );
        assert!(source.join("nested/empty").is_dir());
    }
}

#[test]
#[cfg(windows)]
fn publication_expected_map_and_snapshot_inventory_cannot_be_replaced_by_empty_tree() {
    let root = crate::test_support::TempDir::new("publication-inventory");
    let stage = root.subdir("stage");
    fs::create_dir_all(stage.join("files/payload/empty")).unwrap();
    fs::create_dir_all(stage.join("registry/App")).unwrap();
    fs::write(stage.join("files/payload/file.bin"), b"content").unwrap();
    fs::write(
        stage.join("registry/App/export.reg"),
        b"fixture bytes only, never imported",
    )
    .unwrap();
    fs::write(stage.join("path.json"), b"fixture snapshot, never applied").unwrap();
    let map = std::collections::BTreeMap::from([(
        "payload".into(),
        root.join("original").to_string_lossy().into_owned(),
    )]);
    fs::write(
        stage.join("files/path_map.json"),
        serde_json::to_vec(&map).unwrap(),
    )
    .unwrap();
    let digests = crate::path_seal::collect_reg_digests(&stage).unwrap();
    let expected = publication_inventory(&stage).unwrap();
    for missing in [
        "files/payload/file.bin",
        "files/payload/empty",
        "registry/App/export.reg",
        "path.json",
        "files/path_map.json",
    ] {
        let dest = root.join("published");
        crate::fsutil::copy_dir(&stage, &dest).unwrap();
        let p = dest.join(missing);
        if p.is_dir() {
            fs::remove_dir(&p).unwrap();
        } else {
            fs::remove_file(&p).unwrap();
        }
        assert!(
            verify_publication(&dest, &expected, &map, &digests).is_err(),
            "{missing}"
        );
        fs::remove_dir_all(dest).unwrap();
    }
    fs::remove_dir_all(&stage).unwrap();
    assert!(
        publication_inventory(&stage).is_err(),
        "lost stage is never an empty receipt"
    );
}

#[test]
#[cfg(windows)]
fn publication_readback_failure_aborts_real_pipeline_before_delete() {
    let _env = lock_backup_env();
    let root = crate::test_support::TempDir::new("publication-readback");
    let _store = crate::path_seal::TestStore::new(root.join("seals"));
    let _backup = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", root.join("Backup"));
    let source = source(&root);
    let _readback = crate::path_seal::boundary_tests::CorruptWrite::new();
    let report = crate::executor::run_full_cleanup(
        &app(&source),
        &[item(&source)],
        &crate::executor::FullCleanupOptions {
            dry_run: false,
            skip_official_uninstall: true,
            backup_enabled: true,
            restore_point: false,
            cleanup_source: None,
        },
    );
    assert!(crate::path_seal::boundary_tests::readback_reached());
    assert!(report.aborted);
    assert_eq!(report.deleted, 0);
    assert!(
        report.errors.iter().any(|e| e.contains("seal:")),
        "{:?}",
        report.errors
    );
    assert!(crate::path_seal::verified_seal(Path::new(&report.backup_dir)).is_err());
    assert_eq!(
        fs::read(source.join("nested/payload.bin")).unwrap(),
        b"complete backup bytes"
    );
}
