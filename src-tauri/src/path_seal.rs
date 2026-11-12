//! Path_map seal — out-of-session record of restore write-back targets.
//!
//! Written next to (but outside) the backup session so a tampered `path_map.json`
//! cannot silently widen restore destinations. Library-subpath write-back is only
//! allowed when the seal is present, its map digest matches, and the target is listed.
//!
//! RSEAL2 binds the payload with a privileged installation HMAC key and a
//! current-user DPAPI outer layer. Legacy/missing seals never authorize restore.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Total on-disk bytes, including RSEAL2 magic and DPAPI overhead. This is an
// independent finite budget, not a claim that DPAPI has fixed expansion. The
// canonical plaintext envelope remains capped at 128 KiB in v2.
const MAX_PROTECTED_BLOB: usize = 256 * 1024;

#[cfg(windows)]
mod key;
#[cfg(windows)]
mod manifest;
#[cfg(windows)]
mod v2;

#[cfg(all(test, windows))]
pub(crate) use manifest::read_file_manifest;
#[cfg(windows)]
pub(crate) use manifest::{
    open_verified_manifest, read_file_manifest_with_cancel, write_file_manifest, FileManifest,
    FILE_MANIFEST_NAME,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PathMapSeal {
    /// Session folder name (`{unix}_{safe}`).
    pub session: String,
    /// Digest of the canonical path_map (see [`map_digest_of`]).
    pub map_digest: String,
    /// Sorted unique original restore targets recorded at backup time (normalized).
    pub targets: Vec<String>,
    /// SHA256 of registry exports and path.json, relative to the session.
    /// Empty inventories authorize no registry or PATH snapshots.
    #[serde(default)]
    pub reg_digests: BTreeMap<String, String>,
    /// Digest of every backed-up payload file, keyed `files/<entry>`.
    /// `None` for seals written before this field existed.
    #[serde(default)]
    pub file_digests: Option<BTreeMap<String, String>>,
    /// Authenticated digest of the external file manifest for newly written seals.
    #[serde(default)]
    pub file_manifest_digest: Option<String>,
}

pub(crate) fn normalize_target(s: &str) -> String {
    s.trim().replace('/', "\\").to_lowercase()
}
#[cfg(windows)]
fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB as DATA_BLOB,
        };
        let in_blob = DATA_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as _,
        };
        let mut out_blob = DATA_BLOB::default();
        let ok = CryptProtectData(
            &in_blob,
            windows::core::w!("Remova path_map seal"),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        );
        if ok.is_err() {
            return Err("seal:protect_failed".into());
        }
        let enc = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            out_blob.pbData as *mut core::ffi::c_void,
        ));
        Ok(enc)
    }
}

#[cfg(not(windows))]
fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.to_vec())
}

#[cfg(windows)]
fn unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB as DATA_BLOB,
        };
        let in_blob = DATA_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as _,
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
            return Err("seal:unprotect_failed".into());
        }
        let dec = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            out_blob.pbData as *mut core::ffi::c_void,
        ));
        Ok(dec)
    }
}

#[cfg(not(windows))]
fn unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    // no DPAPI off-Windows — never accept forged plaintext seals.
    let _ = blob;
    Err("seal:unprotect_failed".into())
}

fn seals_root() -> PathBuf {
    // Test override (process-wide, guarded) — env races across parallel tests.
    if let Ok(g) = TEST_ROOT.lock() {
        if let Some(p) = g.as_ref() {
            return p.clone();
        }
    }
    // compile-time test-only (see backup_root).
    #[cfg(test)]
    {
        if let Ok(v) = std::env::var("REMOVA_SEALS_DIR") {
            if !v.trim().is_empty() {
                return PathBuf::from(v);
            }
        }
    }
    program_data().join("Remova").join("seals")
}

pub(crate) fn program_data() -> PathBuf {
    #[cfg(windows)]
    {
        key::program_data().unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(r"C:\ProgramData")
    }
}

/// Serializes tests that install a seals-root override.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
static TEST_ROOT: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

#[cfg(not(test))]
static TEST_ROOT: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Replace the seals root (tests only). Pass `None` to restore the default.
#[cfg(test)]
pub(crate) fn set_seals_root_for_tests(root: Option<PathBuf>) {
    if let Ok(mut g) = TEST_ROOT.lock() {
        *g = root;
    }
}

#[cfg(test)]
pub(crate) struct TestStore(std::sync::MutexGuard<'static, ()>);
#[cfg(test)]
impl TestStore {
    pub fn new(root: PathBuf) -> Self {
        let guard = test_lock();
        std::fs::create_dir_all(&root).unwrap();
        set_seals_root_for_tests(Some(root));
        Self(guard)
    }
}
#[cfg(test)]
impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = &self.0;
        set_seals_root_for_tests(None);
    }
}

fn seal_path(session_name: &str) -> PathBuf {
    // Session names are `{digits}_{safe}` — reject anything that could traverse.
    if session_name.is_empty()
        || session_name.len() > 120
        || session_name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
        || session_name.contains("..")
    {
        return seals_root().join("_invalid");
    }
    seals_root().join(format!("{session_name}.seal.json"))
}

pub(crate) fn map_digest(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        v2::sha256(bytes).unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        String::new()
    }
}

fn session_name_of(session: &Path) -> Option<String> {
    session
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

fn map_digest_of(path_map: &BTreeMap<String, String>) -> String {
    // Canonical compact JSON (BTreeMap = sorted keys) — independent of pretty-print.
    let canonical = serde_json::to_string(path_map).unwrap_or_default();
    map_digest(canonical.as_bytes())
}

/// Persist a DPAPI-protected seal for `path_map` and registry export digests of `session`.
#[cfg(test)]
pub fn write_seal(
    session: &Path,
    _map_json: &str,
    path_map: &BTreeMap<String, String>,
    reg_digests: &BTreeMap<String, String>,
) -> Result<(), String> {
    write_seal_with_files(session, _map_json, path_map, reg_digests, None)
}

/// Seal including the digest of every published payload file. Restore uses this
/// to prove the bytes it writes back are the ones that were backed up.
#[cfg(test)]
pub fn write_seal_with_files(
    session: &Path,
    _map_json: &str,
    path_map: &BTreeMap<String, String>,
    reg_digests: &BTreeMap<String, String>,
    file_digests: Option<BTreeMap<String, String>>,
) -> Result<(), String> {
    write_seal_inner(session, path_map, reg_digests, file_digests, None)
}

#[cfg(windows)]
pub(crate) fn write_seal_with_manifest(
    session: &Path,
    path_map: &BTreeMap<String, String>,
    reg_digests: &BTreeMap<String, String>,
    manifest_digest: String,
) -> Result<(), String> {
    // Refuse to publish a seal for a missing, changed or malformed manifest.
    // The verified handle denies writes and renames until the seal is published.
    let (_, _manifest_pin) = manifest::open_verified_manifest(
        session,
        &manifest_digest,
        crate::executor::cleanup_cancelled,
    )?;
    write_seal_inner(session, path_map, reg_digests, None, Some(manifest_digest))
}

#[cfg(windows)]
pub(crate) fn check_manifest_seal_capacity(
    session: &Path,
    path_map: &BTreeMap<String, String>,
    reg_digests: &BTreeMap<String, String>,
    manifest_digest: &str,
) -> Result<(), String> {
    let name = session_name_of(session).ok_or("seal: bad session name")?;
    let mut payload = v2::payload_with_files(&name, path_map, reg_digests.clone(), None)?;
    payload.file_manifest_digest = Some(manifest_digest.to_owned());
    // The key ID and MAC are fixed-width hex, so every key yields the same
    // serialized size. The preflight writes nothing and needs no real key.
    v2::envelope(&[0; 32], &payload).map(|_| ())
}

fn write_seal_inner(
    session: &Path,
    path_map: &BTreeMap<String, String>,
    reg_digests: &BTreeMap<String, String>,
    file_digests: Option<BTreeMap<String, String>>,
    file_manifest_digest: Option<String>,
) -> Result<(), String> {
    let name = session_name_of(session).ok_or("seal: bad session name")?;
    #[cfg(windows)]
    let blob = {
        let store = key::load(true)?;
        let mut payload =
            v2::payload_with_files(&name, path_map, reg_digests.clone(), file_digests.clone())?;
        payload
            .file_manifest_digest
            .clone_from(&file_manifest_digest);
        v2::encode_blob(&store.key, &payload)?
    };
    #[cfg(not(windows))]
    let blob: Vec<u8> = return Err("seal:windows_required".into());
    let dir = seals_root();
    let _pins = crate::fsutil::create_dirs_pinned(&dir).map_err(|e| e.to_string())?;
    // tmp + rename — a torn seal fails closed and is unusable.
    let target = seal_path(&name);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let tmp = dir.join(format!("seal.{}.{}.tmp", std::process::id(), nonce));
    let mut published = false;
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(&blob)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        published = true;
        #[cfg(all(test, windows))]
        boundary_tests::after_write(&target);
        // Exercise exactly the bounded production reader before any caller may
        // return Ready. A durable but unreadable seal is not a successful backup.
        let verified = verified_seal(session)?;
        let mut targets: Vec<_> = path_map.values().map(|s| normalize_target(s)).collect();
        targets.sort();
        targets.dedup();
        if verified.session != name
            || !map_matches(&verified, path_map)
            || verified.targets != targets
            || &verified.reg_digests != reg_digests
            || verified.file_digests != file_digests
            || verified.file_manifest_digest != file_manifest_digest
        {
            return Err("seal:write_readback_mismatch".into());
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    if result.is_err() && published {
        let _ = std::fs::remove_file(&target);
    }
    result
}

/// Load and DPAPI-verify the seal for `session`. `None` on missing/tampered/foreign-user.
#[cfg(test)]
pub fn load_seal(session: &Path) -> Option<PathMapSeal> {
    verified_seal(session).ok()
}

pub(crate) fn verified_seal(session: &Path) -> Result<PathMapSeal, String> {
    let name = session_name_of(session).ok_or("seal:bad_session")?;
    let file = std::fs::File::open(seal_path(&name)).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => "seal:missing",
        std::io::ErrorKind::PermissionDenied => "seal:read_requires_admin",
        _ => "seal:read",
    })?;
    verified_seal_from_file(&name, &file)
}

#[cfg(windows)]
pub(crate) fn hold_verified_seal(session: &Path) -> Result<(PathMapSeal, std::fs::File), String> {
    let name = session_name_of(session).ok_or("seal:bad_session")?;
    let file = crate::fsutil::open_local_regular_file(&seal_path(&name))
        .map_err(|error| format!("seal:read: {error}"))?;
    let seal = verified_seal_from_file(&name, &file)?;
    Ok((seal, file))
}

fn verified_seal_from_file(name: &str, file: &std::fs::File) -> Result<PathMapSeal, String> {
    use std::io::Read;
    let mut raw = vec![];
    file.take(MAX_PROTECTED_BLOB as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "seal:read")?;
    if raw.len() > MAX_PROTECTED_BLOB {
        return Err("seal:v2_input_too_large".into());
    }
    if raw.starts_with(b"RSEAL1") {
        return Err("seal:legacy_manual_restore_only".into());
    }
    #[cfg(windows)]
    {
        let store = key::load(false)?;
        let seal = v2::verify_blob(&store.key, name, &raw)?;
        Ok(PathMapSeal {
            session: seal.session,
            map_digest: seal.map_digest,
            targets: seal.targets,
            reg_digests: seal.reg_digests,
            file_digests: seal.file_digests,
            file_manifest_digest: seal.file_manifest_digest,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Err("seal:windows_required".into())
    }
}

pub(crate) const MAX_PAYLOAD_NODES: usize = 200_000;

/// Streaming receipt for the private backup tree and published contents.
pub(crate) fn publication_file_digest(path: &Path) -> Result<String, String> {
    #[cfg(windows)]
    {
        let file = crate::fsutil::open_local_regular_file(path).map_err(|e| e.to_string())?;
        crate::fsutil::reject_named_data_streams(&file).map_err(|e| e.to_string())?;
        let digest = v2::digest_payload(&file, std::io::sink())?;
        crate::fsutil::reject_named_data_streams(&file).map_err(|e| e.to_string())?;
        Ok(digest)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("seal:windows_required".into())
    }
}

/// Hash the default stream through an already held file handle.
#[cfg(windows)]
pub(crate) fn publication_reader_digest(reader: impl std::io::Read) -> Result<String, String> {
    v2::digest_payload(reader, std::io::sink())
}

#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoreStreams {
    AuthenticatedDefault,
    RefuseNamed,
}

/// Stage only the bytes read from the pinned unnamed stream and return their digest.
#[cfg(windows)]
pub(crate) fn stage_payload_file(
    source: &std::fs::File,
    target: &Path,
    streams: RestoreStreams,
) -> Result<String, String> {
    let check_streams = || {
        if streams == RestoreStreams::RefuseNamed {
            crate::fsutil::reject_named_data_streams(source).map_err(|e| e.to_string())?;
        }
        Ok::<_, String>(())
    };
    check_streams()?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| e.to_string())?;
    let digest = v2::digest_payload(source, &mut output)?;
    check_streams()?;
    if crate::executor::cleanup_cancelled() {
        return Err("cleanup cancelled".into());
    }
    output.sync_all().map_err(|e| e.to_string())?;
    Ok(digest)
}

pub(crate) fn map_matches(seal: &PathMapSeal, map: &BTreeMap<String, String>) -> bool {
    !seal.map_digest.is_empty() && seal.map_digest == map_digest_of(map)
}
pub(crate) fn snapshot_matches(seal: &PathMapSeal, rel: &str, bytes: &[u8]) -> bool {
    seal.reg_digests
        .get(rel)
        .is_some_and(|expected| !expected.is_empty() && expected == &map_digest(bytes))
}
#[cfg(windows)]
pub(crate) struct BackupStage(key::Stage);
#[cfg(windows)]
impl BackupStage {
    pub fn path(&self) -> &Path {
        &self.0.path
    }
    pub fn verify_snapshots(&self) -> Result<(), String> {
        self.0.verify_snapshots()
    }
}
#[cfg(windows)]
pub(crate) fn backup_stage() -> Result<BackupStage, String> {
    Ok(BackupStage(key::load(true)?.stage()?))
}

#[cfg(windows)]
pub(crate) fn restore_stage() -> Result<BackupStage, String> {
    Ok(BackupStage(key::load(false)?.stage()?))
}

#[cfg(windows)]
pub(crate) struct RestoreReceiptStore(key::Store);

#[cfg(windows)]
impl RestoreReceiptStore {
    pub fn open() -> Result<Self, String> {
        key::load(false).map(Self)
    }

    pub fn receipt(
        &self,
        name: &str,
        writable: bool,
        create: bool,
    ) -> std::io::Result<std::fs::File> {
        self.0.open_restore_receipt(name, writable, create)
    }

    pub fn remove_receipts(&self, names: &std::collections::HashSet<String>) {
        self.0.remove_restore_receipts(names);
    }

    pub fn lock_receipt(&self, name: &str) -> std::io::Result<std::fs::File> {
        self.0.lock_restore_receipt(name)
    }

    pub fn replace_receipt(
        &self,
        name: &str,
        write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
    ) -> std::io::Result<std::fs::File> {
        let stage = restore_stage().map_err(std::io::Error::other)?;
        self.0.replace_restore_receipt(name, stage.path(), write)
    }
}

#[cfg(windows)]
pub(crate) fn cleanup_abandoned_stages() {
    if let Ok(store) = key::load(false) {
        store.cleanup_abandoned();
    }
}

/// True when this original path is a sealed restore target **and** the decoded
/// `path_map` still matches the digest recorded at backup time.
#[cfg(test)]
pub fn target_sealed(session: &Path, path_map: &BTreeMap<String, String>, original: &str) -> bool {
    let Some(seal) = load_seal(session) else {
        return false;
    };
    if seal.map_digest != map_digest_of(path_map) {
        return false;
    }
    let t = normalize_target(original);
    !t.is_empty() && seal.targets.iter().any(|x| normalize_target(x) == t)
}

/// Digests of registry export files under `session/registry/<folder>/`.
/// Keys are `registry/<folder>/<file>` so restore can look them up by path.
pub fn collect_reg_digests(session: &Path) -> Result<BTreeMap<String, String>, String> {
    #[cfg(not(windows))]
    {
        let _ = session;
        Err("seal:windows_required".into())
    }
    #[cfg(windows)]
    {
        let mut out = BTreeMap::new();
        if session.join("path.json").exists() {
            out.insert(
                "path.json".into(),
                v2::sha256_file(&session.join("path.json"))?,
            );
        }
        let root = session.join("registry");
        if !root.exists() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let folder = entry.file_name();
            // Single-value backups authorize value.reg only. Deleting it must
            // not fall back to importing the broader parent-key export.
            if let Some(file) = crate::restore::pick_import_target(&entry.path()) {
                let name = file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or("seal:bad_export_name")?;
                out.insert(
                    format!("registry/{}/{name}", folder.to_string_lossy()),
                    v2::sha256_file(&file)?,
                );
            }
        }
        Ok(out)
    }
}

/// true when this export may proceed to form validation.
/// - Missing/legacy/empty inventory → refuse.
/// - Seal records this file → digest must match.
/// - Seal has `reg_digests` but omits this file → refuse (fail closed).
#[cfg(test)]
pub fn reg_export_allowed(session: &Path, rel: &str, bytes: &[u8]) -> bool {
    load_seal(session).is_some_and(|seal| snapshot_matches(&seal, rel, bytes))
}

#[cfg(all(test, windows))]
pub(crate) mod boundary_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture session directory that removes itself on drop, including on panic.
    fn temp_session(name: &str) -> crate::test_support::TempDir {
        let dir = crate::test_support::TempDir::new(&format!("seal_{name}"));
        dir.subdir("files");
        dir
    }

    fn with_seals_dir<F: FnOnce()>(f: F) {
        let _g = test_lock();
        let dir = std::env::temp_dir().join(format!("remova_seals_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        set_seals_root_for_tests(Some(dir.clone()));
        f();
        set_seals_root_for_tests(None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An empty `map_digest` must never authorize — fail closed.
    #[test]
    fn map_matches_rejects_empty_digest() {
        let map = std::collections::BTreeMap::from([("a.bin".to_string(), r"C:\x".to_string())]);
        let mut seal = PathMapSeal {
            session: "s".into(),
            map_digest: String::new(),
            targets: vec![],
            reg_digests: Default::default(),
            file_digests: None,
            file_manifest_digest: None,
        };
        assert!(!map_matches(&seal, &map));
        seal.map_digest = map_digest_of(&map);
        assert!(map_matches(&seal, &map));
    }

    #[test]
    fn seal_roundtrip_and_tamper_detection() {
        with_seals_dir(|| {
            let sess = temp_session("a");
            let mut map = BTreeMap::new();
            map.insert("x".into(), r"C:\Users\a\Documents\App\f".into());
            write_seal(&sess, "", &map, &BTreeMap::new()).unwrap();

            assert!(target_sealed(&sess, &map, r"C:\Users\a\Documents\App\f"));
            // Target not in seal.
            assert!(!target_sealed(&sess, &map, r"C:\Users\a\Documents\Other\x"));
            // Tampered map → digest mismatch.
            let mut evil = BTreeMap::new();
            evil.insert("x".into(), r"C:\evil".into());
            assert!(!target_sealed(&sess, &evil, r"C:\Users\a\Documents\App\f"));
            let _ = std::fs::remove_dir_all(&sess);
        });
    }

    #[test]
    fn missing_seal_fails_closed() {
        with_seals_dir(|| {
            let sess = temp_session("b");
            let mut map = BTreeMap::new();
            map.insert("x".into(), r"C:\Users\a\Documents\App\f".into());
            assert!(!target_sealed(&sess, &map, r"C:\Users\a\Documents\App\f"));
            let _ = std::fs::remove_dir_all(&sess);
        });
    }

    #[test]
    fn seal_path_rejects_traversal_names() {
        let p = seal_path("..\\evil");
        assert!(p.ends_with("_invalid") || p.to_string_lossy().contains("_invalid"));
    }

    /// Forged plaintext seal without DPAPI magic must not authorize restore.
    #[test]
    fn forged_plaintext_seal_rejected() {
        with_seals_dir(|| {
            let sess = temp_session("forge");
            let mut map = BTreeMap::new();
            map.insert("x".into(), r"C:\Users\a\Documents\App\f".into());
            let name = session_name_of(&sess).unwrap();
            // Attacker drops a plaintext JSON seal (no RSEAL1 / DPAPI).
            let forged = serde_json::to_string(&PathMapSeal {
                session: name.clone(),
                map_digest: map_digest_of(&map),
                targets: vec![normalize_target(r"C:\Users\a\Documents\App\f")],
                reg_digests: BTreeMap::new(),
                file_digests: None,
                file_manifest_digest: None,
            })
            .unwrap();
            std::fs::write(seal_path(&name), forged).unwrap();
            assert!(!target_sealed(&sess, &map, r"C:\Users\a\Documents\App\f"));
            let _ = std::fs::remove_dir_all(&sess);
        });
    }

    /// Flipping a byte in a real seal must fail unprotect (DPAPI integrity).
    #[test]
    fn tampered_seal_blob_rejected() {
        with_seals_dir(|| {
            let sess = temp_session("flip");
            let mut map = BTreeMap::new();
            map.insert("x".into(), r"C:\Users\a\Documents\App\f".into());
            write_seal(&sess, "", &map, &BTreeMap::new()).unwrap();
            assert!(target_sealed(&sess, &map, r"C:\Users\a\Documents\App\f"));
            let name = session_name_of(&sess).unwrap();
            let path = seal_path(&name);
            let mut bytes = std::fs::read(&path).unwrap();
            let n = bytes.len() - 1;
            bytes[n] ^= 0xFF;
            std::fs::write(&path, &bytes).unwrap();
            assert!(!target_sealed(&sess, &map, r"C:\Users\a\Documents\App\f"));
            let _ = std::fs::remove_dir_all(&sess);
        });
    }

    /// registry export digests bind .reg bytes into the seal.
    #[test]
    fn reg_export_digest_roundtrip_and_tamper() {
        with_seals_dir(|| {
            let sess = temp_session("regseal");
            std::fs::create_dir_all(sess.join("registry").join("App")).unwrap();
            let export = sess.join("registry").join("App").join("export.reg");
            std::fs::write(&export, b"Windows Registry Editor Version 5.00").unwrap();
            let digests = collect_reg_digests(&sess).unwrap();
            assert_eq!(digests.len(), 1);
            assert!(digests.contains_key("registry/App/export.reg"));
            write_seal(&sess, "", &BTreeMap::new(), &digests).unwrap();

            let bytes = std::fs::read(&export).unwrap();
            assert!(reg_export_allowed(&sess, "registry/App/export.reg", &bytes));
            // Tampered bytes → refuse.
            assert!(!reg_export_allowed(
                &sess,
                "registry/App/export.reg",
                b"evil"
            ));
            // Unknown export name is not listed → refuse.
            assert!(!reg_export_allowed(&sess, "registry/App/value.reg", &bytes));
            let _ = std::fs::remove_dir_all(&sess);
        });
    }

    /// Empty inventories cannot authorize newly planted exports.
    #[test]
    fn empty_reg_digests_refuses_exports() {
        with_seals_dir(|| {
            let sess = temp_session("oldseal");
            write_seal(&sess, "", &BTreeMap::new(), &BTreeMap::new()).unwrap();
            assert!(!reg_export_allowed(&sess, "registry/App/export.reg", b"x"));
            let _ = std::fs::remove_dir_all(&sess);
        });
    }

    #[test]
    fn restore_v2_authorizes_once_before_file_path_or_registry_writes() {
        with_seals_dir(|| {
            let _path_lock = crate::regops::path_mock::lock_mock();
            crate::regops::path_mock::install(r"C:\Other", r"C:\Windows\System32");
            let sess = temp_session("restore_v2");
            let dest = sess.join("output/file.txt");
            std::fs::write(sess.join("files/file.txt"), b"authenticated destination").unwrap();
            let map = BTreeMap::from([("file.txt".into(), dest.to_string_lossy().into_owned())]);
            std::fs::write(
                sess.join("files/path_map.json"),
                serde_json::to_vec(&map).unwrap(),
            )
            .unwrap();
            let name = session_name_of(&sess).unwrap();
            assert_eq!(
                crate::restore::restore_session(&sess).unwrap_err(),
                "seal:missing"
            );
            // A standard-user-created, DPAPI-valid v1 is still not authorization.
            let old = PathMapSeal {
                session: name.clone(),
                map_digest: map_digest_of(&map),
                targets: map.values().map(|s| normalize_target(s)).collect(),
                reg_digests: BTreeMap::new(),
                file_digests: None,
                file_manifest_digest: None,
            };
            let mut legacy = b"RSEAL1".to_vec();
            legacy.extend(protect(&serde_json::to_vec(&old).unwrap()).unwrap());
            std::fs::write(seal_path(&name), legacy).unwrap();
            assert_eq!(
                crate::restore::restore_session(&sess).unwrap_err(),
                "seal:legacy_manual_restore_only"
            );
            write_seal(&sess, "", &map, &BTreeMap::new()).unwrap();
            let original_blob = std::fs::read(seal_path(&name)).unwrap();
            let mut envelope: serde_json::Value =
                serde_json::from_slice(&unprotect(&original_blob[6..]).unwrap()).unwrap();
            envelope["mac"] = serde_json::Value::String("0".repeat(64));
            let mut bad_mac = b"RSEAL2".to_vec();
            // Value sorts object keys; use the canonical envelope field order.
            let canonical = format!(
                "{{\"version\":2,\"key_id\":{},\"payload\":{},\"mac\":{}}}",
                envelope["key_id"], envelope["payload"], envelope["mac"]
            );
            bad_mac.extend(protect(canonical.as_bytes()).unwrap());
            std::fs::write(seal_path(&name), bad_mac).unwrap();
            assert_eq!(
                crate::restore::restore_session(&sess).unwrap_err(),
                "seal:v2_bad_mac"
            );
            std::fs::write(seal_path(&name), original_blob).unwrap();
            let evil_map = BTreeMap::from([("file.txt", "C:\\evil\\file.txt")]);
            std::fs::write(
                sess.join("files/path_map.json"),
                serde_json::to_vec(&evil_map).unwrap(),
            )
            .unwrap();
            assert_eq!(
                crate::restore::restore_session(&sess).unwrap_err(),
                "seal:map_mismatch"
            );
            std::fs::write(
                sess.join("files/path_map.json"),
                serde_json::to_vec(&map).unwrap(),
            )
            .unwrap();
            let reg = sess.join("registry/App");
            std::fs::create_dir_all(&reg).unwrap();
            std::fs::write(reg.join("export.reg"), b"unlisted export must never import").unwrap();
            assert_eq!(
                crate::restore::restore_session(&sess).unwrap_err(),
                "seal:registry_snapshot_mismatch"
            );
            std::fs::remove_file(reg.join("export.reg")).unwrap();
            let snap = crate::backup::PathSnapshot {
                items: vec![crate::backup::PathSnapshotItem {
                    entry: r"C:\Vendor\Tool".into(),
                    scopes: vec!["User".into()],
                    user_path: r"C:\Vendor\Tool".into(),
                    machine_path: String::new(),
                }],
                ..Default::default()
            };
            let bytes = serde_json::to_vec(&snap).unwrap();
            std::fs::write(sess.join("path.json"), &bytes).unwrap();
            assert_eq!(
                crate::restore::restore_session(&sess).unwrap_err(),
                "seal:path_snapshot_mismatch"
            );
            let inventory = BTreeMap::from([("path.json".into(), map_digest(&bytes))]);
            let files = BTreeMap::from([(
                "files/file.txt".into(),
                publication_file_digest(&sess.join("files/file.txt")).unwrap(),
            )]);
            write_seal_with_files(&sess, "", &map, &inventory, Some(files)).unwrap();
            std::fs::write(sess.join("path.json"), b"tampered").unwrap();
            assert_eq!(
                crate::restore::restore_session(&sess).unwrap_err(),
                "seal:path_snapshot_mismatch"
            );
            assert!(
                !dest.exists(),
                "all failures must precede sibling file writes"
            );
            assert_eq!(crate::regops::path_mock::get("User"), r"C:\Other");
            std::fs::write(sess.join("path.json"), bytes).unwrap();
            crate::restore::restore_session(&sess).unwrap();
            assert_eq!(std::fs::read(&dest).unwrap(), b"authenticated destination");
            assert!(crate::regops::path_mock::get("User").contains(r"C:\Vendor\Tool"));
            assert_eq!(
                crate::regops::path_mock::get("Machine"),
                r"C:\Windows\System32"
            );
            crate::regops::path_mock::clear();
            std::fs::remove_dir_all(sess).unwrap();
        });
    }

    #[test]
    fn single_value_inventory_never_authorizes_parent_key_fallback() {
        with_seals_dir(|| {
            let sess = temp_session("single_value");
            let reg = sess.join("registry/App");
            std::fs::create_dir_all(&reg).unwrap();
            std::fs::write(reg.join("export.reg"), b"whole parent key").unwrap();
            std::fs::write(reg.join("value.reg"), b"single value").unwrap();
            let inventory = collect_reg_digests(&sess).unwrap();
            assert_eq!(inventory.len(), 1);
            assert!(inventory.contains_key("registry/App/value.reg"));
            write_seal(&sess, "", &BTreeMap::new(), &inventory).unwrap();
            std::fs::remove_file(reg.join("value.reg")).unwrap();
            assert!(!reg_export_allowed(
                &sess,
                "registry/App/export.reg",
                b"whole parent key"
            ));
            assert_eq!(
                crate::restore::restore_session(&sess).unwrap_err(),
                "seal:registry_snapshot_missing"
            );
            std::fs::remove_dir_all(sess).unwrap();
        });
    }
}
