//! Readback failure injection confined to a TestStore's temporary seal path.
use super::*;
use std::cell::Cell;
thread_local! {
    static CORRUPT_WRITE: Cell<bool> = const { Cell::new(false) };
    static REACHED: Cell<bool> = const { Cell::new(false) };
}
pub(crate) struct CorruptWrite;
impl CorruptWrite {
    pub(crate) fn new() -> Self {
        assert!(TEST_ROOT.lock().unwrap().is_some(), "TestStore required");
        CORRUPT_WRITE.with(|v| assert!(!v.replace(true)));
        REACHED.with(|v| v.set(false));
        Self
    }
}
impl Drop for CorruptWrite {
    fn drop(&mut self) {
        CORRUPT_WRITE.with(|v| v.set(false));
    }
}
pub(crate) fn readback_reached() -> bool {
    REACHED.with(Cell::get)
}
pub(super) fn after_write(path: &Path) {
    if CORRUPT_WRITE.with(Cell::get) {
        let root = TEST_ROOT
            .lock()
            .unwrap()
            .clone()
            .expect("TestStore required");
        assert!(path.starts_with(root));
        std::fs::write(path, b"RSEAL2broken-fixture").unwrap();
        REACHED.with(|v| v.set(true));
    }
}

#[test]
fn seal_boundary_bounded_production_reader_and_failed_write_leave_no_valid_seal() {
    let root = crate::test_support::TempDir::new("seal-read-budget");
    let _store = TestStore::new(root.join("seals"));
    let session = root.subdir("1700000000_reader");
    let target = seal_path("1700000000_reader");
    for size in [
        MAX_PROTECTED_BLOB - 1,
        MAX_PROTECTED_BLOB,
        MAX_PROTECTED_BLOB + 1,
    ] {
        let mut bytes = vec![0; size];
        bytes[..6].copy_from_slice(b"RSEAL2");
        std::fs::write(&target, bytes).unwrap();
        let error = verified_seal(&session).unwrap_err();
        if size > MAX_PROTECTED_BLOB {
            assert_eq!(error, "seal:v2_input_too_large");
        } else {
            assert_eq!(
                error, "seal:unprotect_failed",
                "within-budget bytes must reach DPAPI"
            );
        }
    }
    std::fs::remove_file(&target).unwrap();
    let _corrupt = CorruptWrite::new();
    assert!(write_seal(&session, "", &BTreeMap::new(), &BTreeMap::new()).is_err());
    assert!(readback_reached());
    assert!(
        !target.exists(),
        "failed readback must remove the published seal"
    );
    assert!(verified_seal(&session).is_err());
    assert!(
        std::fs::read_dir(root.join("seals"))
            .unwrap()
            .next()
            .is_none(),
        "no seal or temp file remains"
    );
}
