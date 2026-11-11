//! Strict RSEAL2 protocol. Signing keys remain in the privileged native store.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use windows::Win32::Security::Cryptography::*;

const MAGIC: &[u8] = b"RSEAL2";
// UTF-8 bytes of the canonical JSON envelope, before DPAPI protection.
const MAX_PLAINTEXT_ENVELOPE: usize = 128 * 1024;

struct Provider(BCRYPT_ALG_HANDLE);
impl Drop for Provider {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptCloseAlgorithmProvider(self.0, 0);
        }
    }
}
struct Hash(BCRYPT_HASH_HANDLE);
impl Drop for Hash {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptDestroyHash(self.0);
        }
    }
}

fn digest(data: &[u8], key: Option<&[u8; 32]>) -> Result<[u8; 32], String> {
    digest_reader(&mut std::io::Cursor::new(data), key)
}

fn digest_reader(
    reader: &mut dyn std::io::Read,
    key: Option<&[u8; 32]>,
) -> Result<[u8; 32], String> {
    unsafe {
        let flags = if key.is_some() {
            BCRYPT_ALG_HANDLE_HMAC_FLAG
        } else {
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0)
        };
        let mut alg = BCRYPT_ALG_HANDLE::default();
        BCryptOpenAlgorithmProvider(
            &mut alg,
            BCRYPT_SHA256_ALGORITHM,
            windows::core::PCWSTR::null(),
            flags,
        )
        .ok()
        .map_err(|_| "seal:cng_provider")?;
        let provider = Provider(alg);
        let mut raw_hash = BCRYPT_HASH_HANDLE::default();
        // CNG allocates its own hash object when the object buffer is None.
        BCryptCreateHash(
            provider.0,
            &mut raw_hash,
            None,
            key.map(|k| k.as_slice()),
            0,
        )
        .ok()
        .map_err(|_| "seal:cng_create_hash")?;
        let hash = Hash(raw_hash);
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let n = reader
                .read(&mut chunk)
                .map_err(|e| format!("seal:hash_read: {e}"))?;
            if n == 0 {
                break;
            }
            BCryptHashData(hash.0, &chunk[..n], 0)
                .ok()
                .map_err(|_| "seal:cng_hash_data")?;
        }
        let mut out = [0; 32];
        BCryptFinishHash(hash.0, &mut out, 0)
            .ok()
            .map_err(|_| "seal:cng_finish")?;
        Ok(out)
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("seal:v2_bad_hex".into());
    }
    let mut bytes = [0; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| "seal:v2_bad_hex")?;
    }
    Ok(bytes)
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Payload {
    pub(super) session: String,
    pub(super) map_digest: String,
    pub(super) targets: Vec<String>,
    pub(super) reg_digests: BTreeMap<String, String>,
    /// SHA256 of every published backup payload, keyed by its relative path
    /// (`files/<entry>`). `None` on seals written before this field existed;
    /// restore treats `None` as "content not authenticated" rather than
    /// pretending the bytes were verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) file_digests: Option<BTreeMap<String, String>>,
    /// Digest of the independently bounded file and directory manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) file_manifest_digest: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    key_id: String,
    payload: String,
    mac: String,
}

/// Convenience wrapper for tests that carry no payload digests. Production
/// backup uses `payload_with_files`.
#[cfg(test)]
pub(super) fn payload(
    session: &str,
    map: &BTreeMap<String, String>,
    reg_digests: BTreeMap<String, String>,
) -> Result<Payload, String> {
    payload_with_files(session, map, reg_digests, None)
}

pub(super) fn payload_with_files(
    session: &str,
    map: &BTreeMap<String, String>,
    reg_digests: BTreeMap<String, String>,
    file_digests: Option<BTreeMap<String, String>>,
) -> Result<Payload, String> {
    let targets: BTreeSet<_> = map.values().map(|s| super::normalize_target(s)).collect();
    let map_bytes = serde_json::to_vec(map).map_err(|_| "seal:v2_map_json")?;
    Ok(Payload {
        session: session.into(),
        map_digest: hex(&digest(&map_bytes, None)?),
        targets: targets.into_iter().collect(),
        reg_digests,
        file_digests,
        file_manifest_digest: None,
    })
}

fn frame(key_id: &[u8; 32], payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"Remova:path-seal:v2\0".to_vec();
    bytes.push(2);
    bytes.extend_from_slice(key_id);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn envelope(key: &[u8; 32], payload: &Payload) -> Result<Vec<u8>, String> {
    let key_id = digest(key, None)?;
    let payload = serde_json::to_string(payload).map_err(|_| "seal:v2_payload_json")?;
    let mac = digest(&frame(&key_id, payload.as_bytes()), Some(key))?;
    let bytes = serde_json::to_vec(&Envelope {
        version: 2,
        key_id: hex(&key_id),
        payload,
        mac: hex(&mac),
    })
    .map_err(|_| "seal:v2_envelope_json")?;
    if bytes.len() > MAX_PLAINTEXT_ENVELOPE {
        return Err("seal:v2_input_too_large".into());
    }
    Ok(bytes)
}

fn verify_envelope(
    key: &[u8; 32],
    expected_session: &str,
    bytes: &[u8],
) -> Result<Payload, String> {
    if bytes.len() > MAX_PLAINTEXT_ENVELOPE {
        return Err("seal:v2_input_too_large".into());
    }
    let env: Envelope = serde_json::from_slice(bytes).map_err(|_| "seal:v2_envelope_json")?;
    if env.version != 2 || serde_json::to_vec(&env).map_err(|_| "seal:v2_envelope_json")? != bytes {
        return Err("seal:v2_noncanonical_envelope".into());
    }
    let key_id = digest(key, None)?;
    if unhex(&env.key_id)? != key_id {
        return Err("seal:v2_key_mismatch".into());
    }
    let supplied_mac = unhex(&env.mac)?;
    let computed_mac = digest(&frame(&key_id, env.payload.as_bytes()), Some(key))?;
    // Full-length MAC comparison; no data-dependent early byte exit.
    let mismatch = supplied_mac
        .iter()
        .zip(computed_mac)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b));
    if mismatch != 0 {
        return Err("seal:v2_bad_mac".into());
    }
    let parsed: Payload = serde_json::from_str(&env.payload).map_err(|_| "seal:v2_payload_json")?;
    if serde_json::to_string(&parsed).map_err(|_| "seal:v2_payload_json")? != env.payload {
        return Err("seal:v2_noncanonical_payload".into());
    }
    if parsed.session != expected_session {
        return Err("seal:v2_session_mismatch".into());
    }
    unhex(&parsed.map_digest)?;
    for value in parsed.reg_digests.values() {
        unhex(value)?;
    }
    if let Some(value) = &parsed.file_manifest_digest {
        unhex(value)?;
        if parsed.file_digests.is_some() {
            return Err("seal:v2_conflicting_file_manifests".into());
        }
    }
    let targets: BTreeSet<_> = parsed
        .targets
        .iter()
        .map(|s| super::normalize_target(s))
        .collect();
    if targets.into_iter().collect::<Vec<_>>() != parsed.targets {
        return Err("seal:v2_noncanonical_targets".into());
    }
    Ok(parsed)
}

pub(super) fn encode_blob(key: &[u8; 32], payload: &Payload) -> Result<Vec<u8>, String> {
    let plain = envelope(key, payload)?;
    // Encoding must not report success for a payload rejected by our strict
    // reader (including malformed digests or noncanonical target inventories).
    verify_envelope(key, &payload.session, &plain)?;
    let mut bytes = MAGIC.to_vec();
    bytes.extend(super::protect(&plain)?);
    if bytes.len() > super::MAX_PROTECTED_BLOB {
        return Err("seal:v2_protected_blob_too_large".into());
    }
    verify_blob(key, &payload.session, &bytes)?;
    Ok(bytes)
}
pub(super) fn verify_blob(
    key: &[u8; 32],
    expected_session: &str,
    bytes: &[u8],
) -> Result<Payload, String> {
    if bytes.len() > super::MAX_PROTECTED_BLOB || !bytes.starts_with(MAGIC) {
        return Err("seal:v2_bad_magic_or_length".into());
    }
    verify_envelope(
        key,
        expected_session,
        &super::unprotect(&bytes[MAGIC.len()..])?,
    )
}

#[cfg(test)]
mod boundary_tests;

#[test]
fn cng_reproduces_independent_sha256_hmac_vector() {
    let key = zeroize::Zeroizing::new(std::array::from_fn(|i| i as u8));
    let data = payload("1700000000_demo", &BTreeMap::new(), BTreeMap::new()).unwrap();
    assert_eq!(
        data.map_digest,
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
    let env: Envelope = serde_json::from_slice(&envelope(&key, &data).unwrap()).unwrap();
    assert_eq!(
        env.key_id,
        "630dcd2966c4336691125448bbb25b4ff412a49c732db2c8abc1b8581bd710dd"
    );
    assert_eq!(env.payload.len(), 139);
    assert_eq!(
        env.mac,
        "30e49f383765b74c9cde78d2511d45e6911143e5368b316083e1f59290c9bd29"
    );
}

#[test]
fn v2_dpapi_roundtrip_rejects_legacy_wrong_session_and_key() {
    let key = [7; 32];
    let map = BTreeMap::from([("file".into(), r"C:\Vendor\产品".into())]);
    let data = payload(
        "1700000000_demo",
        &map,
        BTreeMap::from([(
            "registry/App/export.reg".into(),
            hex(&digest(b"reg bytes", None).unwrap()),
        )]),
    )
    .unwrap();
    let blob = encode_blob(&key, &data).unwrap();
    assert_eq!(verify_blob(&key, "1700000000_demo", &blob).unwrap(), data);
    assert!(verify_blob(&[8; 32], "1700000000_demo", &blob).is_err());
    assert!(verify_blob(&key, "1700000000_other", &blob).is_err());
    let forged_v1 = super::PathMapSeal {
        session: "1700000000_demo".into(),
        map_digest: super::map_digest_of(&map),
        targets: vec![super::normalize_target(r"C:\Vendor\产品")],
        reg_digests: BTreeMap::new(),
        file_digests: None,
        file_manifest_digest: None,
    };
    let mut legacy = b"RSEAL1".to_vec();
    legacy.extend(super::protect(&serde_json::to_vec(&forged_v1).unwrap()).unwrap());
    assert!(verify_blob(&key, "1700000000_demo", &legacy).is_err());
    assert!(verify_blob(&key, "1700000000_demo", b"").is_err());
}

#[test]
fn v2_strict_fields_lengths_and_mac_tampering() {
    let key = [9; 32];
    let session = "1700000000_strict";
    let data = payload(session, &BTreeMap::new(), BTreeMap::new()).unwrap();
    let good = envelope(&key, &data).unwrap();
    for field in ["version", "key_id", "payload", "mac"] {
        let mut changed: serde_json::Value = serde_json::from_slice(&good).unwrap();
        if field == "version" {
            changed[field] = 3.into();
        } else {
            changed[field] = "bad".into();
        }
        assert!(verify_envelope(&key, session, &serde_json::to_vec(&changed).unwrap()).is_err());
    }
    let mut env: Envelope = serde_json::from_slice(&good).unwrap();
    let old = env.mac.as_bytes()[0];
    env.mac
        .replace_range(0..1, if old == b'0' { "1" } else { "0" });
    assert!(verify_envelope(&key, session, &serde_json::to_vec(&env).unwrap()).is_err());
    assert!(verify_envelope(&key, session, &[b'x'; MAX_PLAINTEXT_ENVELOPE + 1]).is_err());
    let unknown = String::from_utf8(good.clone())
        .unwrap()
        .replacen('{', "{\"extra\":true,", 1);
    assert!(verify_envelope(&key, session, unknown.as_bytes()).is_err());
    let duplicate = String::from_utf8(good)
        .unwrap()
        .replacen('{', "{\"version\":2,", 1);
    assert!(verify_envelope(&key, session, duplicate.as_bytes()).is_err());
}

#[test]
fn v2_canonical_payload_rejects_duplicate_nested_keys() {
    let key = [11; 32];
    let session = "1700000000_canonical";
    let key_id = digest(&key, None).unwrap();
    let text = format!("{{\"session\":\"{session}\",\"map_digest\":\"{}\",\"targets\":[],\"reg_digests\":{{\"registry/a/export.reg\":\"{}\",\"registry/a/export.reg\":\"{}\"}}}}", hex(&[0; 32]), hex(&[1; 32]), hex(&[2; 32]));
    let mac = hex(&digest(&frame(&key_id, text.as_bytes()), Some(&key)).unwrap());
    let bytes = serde_json::to_vec(&Envelope {
        version: 2,
        key_id: hex(&key_id),
        payload: text,
        mac,
    })
    .unwrap();
    assert!(verify_envelope(&key, session, &bytes).is_err());
    let one = BTreeMap::from([
        ("b".into(), r"C:/Vendor/产品".into()),
        ("a".into(), r"c:\vendor\产品".into()),
    ]);
    let reversed = BTreeMap::from([
        ("a".into(), r"c:\vendor\产品".into()),
        ("b".into(), r"C:/Vendor/产品".into()),
    ]);
    let p1 = payload(session, &one, BTreeMap::new()).unwrap();
    let p2 = payload(session, &reversed, BTreeMap::new()).unwrap();
    assert_eq!(p1.targets, [r"c:\vendor\产品"]);
    assert_eq!(envelope(&key, &p1).unwrap(), envelope(&key, &p2).unwrap());
}

pub(super) fn sha256(bytes: &[u8]) -> Result<String, String> {
    Ok(hex(&digest(bytes, None)?))
}

pub(super) fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    let file = crate::fsutil::open_local_regular_file(path).map_err(|e| e.to_string())?;
    digest_payload(file, std::io::sink())
}

/// Hash exactly the bytes handed to the output and poll cancellation before every read.
pub(super) fn digest_payload(
    source: impl std::io::Read,
    output: impl std::io::Write,
) -> Result<String, String> {
    struct PayloadReader<R, W>(R, W);
    impl<R: std::io::Read, W: std::io::Write> std::io::Read for PayloadReader<R, W> {
        fn read(&mut self, chunk: &mut [u8]) -> std::io::Result<usize> {
            if crate::executor::cleanup_cancelled() {
                return Err(std::io::Error::other("cleanup cancelled"));
            }
            let n = self.0.read(chunk)?;
            self.1.write_all(&chunk[..n])?;
            Ok(n)
        }
    }
    Ok(hex(&digest_reader(
        &mut PayloadReader(source, output),
        None,
    )?))
}

#[test]
fn sha256_streams_registry_exports_larger_than_envelope_limit() {
    let bytes = vec![b'a'; 1024 * 1024 + 1];
    assert_eq!(
        sha256(&bytes).unwrap(),
        "4a3f0c0c213adea174f9a3d4c13177315b588bdb2e9c1012d3d0bf0453ca0f6a"
    );
}

#[test]
fn payload_hash_and_copy_stop_after_the_current_chunk_when_cancelled() {
    struct CancelAfterChunk;
    impl std::io::Read for CancelAfterChunk {
        fn read(&mut self, chunk: &mut [u8]) -> std::io::Result<usize> {
            chunk.fill(b'a');
            crate::executor::cancel_cleanup_operation();
            Ok(chunk.len())
        }
    }

    crate::executor::begin_cleanup_control();
    let mut copied = Vec::new();
    let result = digest_payload(CancelAfterChunk, &mut copied);
    crate::executor::begin_cleanup_control();
    assert!(result.unwrap_err().contains("cleanup cancelled"));
    assert_eq!(copied.len(), 64 * 1024);
}
