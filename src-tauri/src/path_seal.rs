//! Path_map seal — out-of-session record of restore write-back targets (N-risk).
//!
//! Written next to (but outside) the backup session so a tampered `path_map.json`
//! cannot silently widen restore destinations. Library-subpath write-back is only
//! allowed when the seal is present, its map digest matches, and the target is listed.
//!
//! The seal **payload is DPAPI-bound** to the current Windows user+machine, so a
//! forged file dropped into `seals/` cannot authorize an arbitrary write target.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PathMapSeal {
    /// Session folder name (`{unix}_{safe}`).
    pub session: String,
    /// Digest of the canonical path_map (see [`map_digest_of`]).
    pub map_digest: String,
    /// Sorted unique original restore targets recorded at backup time (normalized).
    pub targets: Vec<String>,
    /// R21-SEC-08: FNV digests of registry export bytes, keyed by
    /// `registry/<folder>/<file>` (relative to the session). Absent/empty on
    /// seals written before this field existed (compat: form-whitelist only).
    #[serde(default)]
    pub reg_digests: BTreeMap<String, String>,
}

/// Magic prefix for DPAPI-protected seal blobs.
const SEAL_MAGIC: &[u8] = b"RSEAL1";

fn normalize_target(s: &str) -> String {
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
    // REV-SEC-10: no DPAPI off-Windows — never accept forged plaintext seals.
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
    // R23-SEC-08: compile-time test-only (see backup_root).
    #[cfg(test)]
    {
        if let Ok(v) = std::env::var("REMOVA_SEALS_DIR") {
            if !v.trim().is_empty() {
                return PathBuf::from(v);
            }
        }
    }
    let pd = std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Remova").join("seals")
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

fn map_digest(bytes: &[u8]) -> String {
    // FNV-1a over raw bytes (same primitive as cache keys; enough to detect edits
    // when the digest lives outside the session tree).
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
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
pub fn write_seal(
    session: &Path,
    _map_json: &str,
    path_map: &BTreeMap<String, String>,
    reg_digests: &BTreeMap<String, String>,
) -> Result<(), String> {
    let name = session_name_of(session).ok_or("seal: bad session name")?;
    let mut targets: BTreeSet<String> = BTreeSet::new();
    for orig in path_map.values() {
        let t = orig.trim();
        if !t.is_empty() {
            targets.insert(normalize_target(t));
        }
    }
    let seal = PathMapSeal {
        session: name.clone(),
        map_digest: map_digest_of(path_map),
        targets: targets.into_iter().collect(),
        reg_digests: reg_digests.clone(),
    };
    let json = serde_json::to_string(&seal).map_err(|e| e.to_string())?;
    let mut blob = SEAL_MAGIC.to_vec();
    blob.extend(protect(json.as_bytes())?);
    let dir = seals_root();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // R23-SEC-05: tmp + rename — a torn seal fails closed and is unusable.
    let target = seal_path(&name);
    let tmp = target.with_extension("seal.tmp");
    std::fs::write(&tmp, blob).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Load and DPAPI-verify the seal for `session`. `None` on missing/tampered/foreign-user.
pub fn load_seal(session: &Path) -> Option<PathMapSeal> {
    let name = session_name_of(session)?;
    let raw = std::fs::read(seal_path(&name)).ok()?;
    let payload = raw.strip_prefix(SEAL_MAGIC)?;
    let json = unprotect(payload).ok()?;
    serde_json::from_slice(&json).ok()
}

/// True when this original path is a sealed restore target **and** the decoded
/// `path_map` still matches the digest recorded at backup time.
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
pub fn collect_reg_digests(session: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(session.join("registry")) else {
        return out;
    };
    for e in rd.flatten() {
        let folder = e.file_name();
        let folder = folder.to_string_lossy();
        for name in ["export.reg", "value.reg"] {
            let f = e.path().join(name);
            if let Ok(bytes) = std::fs::read(&f) {
                out.insert(format!("registry/{folder}/{name}"), map_digest(&bytes));
            }
        }
    }
    out
}

/// R21-SEC-08: true when this export may proceed to form validation.
/// - No seal, or a pre-`reg_digests` seal → allow (compat with old sessions).
/// - Seal records this file → digest must match.
/// - Seal has `reg_digests` but omits this file → refuse (fail closed).
pub fn reg_export_allowed(session: &Path, rel: &str, bytes: &[u8]) -> bool {
    let Some(seal) = load_seal(session) else {
        return true;
    };
    if seal.reg_digests.is_empty() {
        return true;
    }
    match seal.reg_digests.get(rel) {
        Some(expected) => expected == &map_digest(bytes),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_session(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("remova_seal_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("files")).unwrap();
        p
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

    /// R21-SEC-08: registry export digests bind .reg bytes into the seal.
    #[test]
    fn reg_export_digest_roundtrip_and_tamper() {
        with_seals_dir(|| {
            let sess = temp_session("regseal");
            std::fs::create_dir_all(sess.join("registry").join("App")).unwrap();
            let export = sess.join("registry").join("App").join("export.reg");
            std::fs::write(&export, b"Windows Registry Editor Version 5.00").unwrap();
            let digests = collect_reg_digests(&sess);
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

    /// Old seals without `reg_digests` keep form-whitelist-only behavior.
    #[test]
    fn empty_reg_digests_allows_compat() {
        with_seals_dir(|| {
            let sess = temp_session("oldseal");
            write_seal(&sess, "", &BTreeMap::new(), &BTreeMap::new()).unwrap();
            assert!(reg_export_allowed(&sess, "registry/App/export.reg", b"x"));
            let _ = std::fs::remove_dir_all(&sess);
        });
    }
}
