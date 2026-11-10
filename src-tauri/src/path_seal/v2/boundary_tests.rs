//! Native Windows DPAPI boundary tests; only temporary TestStore files are written.
use super::*;

fn plain_len(key: &[u8; 32], data: &Payload) -> usize {
    // Same canonical wire fields, without the encoder's deliberate size guard.
    let key_id = digest(key, None).unwrap();
    let payload = serde_json::to_string(data).unwrap();
    let mac = digest(&frame(&key_id, payload.as_bytes()), Some(key)).unwrap();
    serde_json::to_vec(&Envelope {
        version: 2,
        key_id: hex(&key_id),
        payload,
        mac: hex(&mac),
    })
    .unwrap()
    .len()
}
fn near_limit_map(
    session: &std::path::Path,
    key: &[u8; 32],
    size: usize,
) -> (BTreeMap<String, String>, BTreeMap<String, String>, Payload) {
    let name = session.file_name().unwrap().to_str().unwrap();
    let mut map: BTreeMap<_, _> = (0..180)
        .map(|i| {
            let target = session
                .join("originals")
                .join(format!("{i:04}_{}", "产品🦀".repeat(50)))
                .join("payload.bin");
            (format!("file{i:04}"), target.to_string_lossy().into_owned())
        })
        .collect();
    let inventory: BTreeMap<_, _> = (0..50)
        .map(|i| {
            (
                format!("registry/{i:04}_{}/export.reg", "目录".repeat(14)),
                hex(&digest(format!("fixture-{i}").as_bytes(), None).unwrap()),
            )
        })
        .collect();
    let data = payload(name, &map, inventory.clone()).unwrap();
    let base_len = plain_len(key, &data);
    assert!(base_len < size, "fixture base is too large: {base_len}");
    // Distribute exact ASCII padding over long logical target paths. This tests
    // UTF-8 bytes, not character counts; no such paths are opened or restored.
    let mut remaining = size - base_len;
    for target in map.values_mut() {
        let count = remaining.min(240);
        target.push_str(&"a".repeat(count));
        remaining -= count;
    }
    assert_eq!(remaining, 0);
    let data = payload(name, &map, inventory.clone()).unwrap();
    assert_eq!(plain_len(key, &data), size);
    (map, inventory, data)
}

#[test]
fn seal_boundary_native_dpapi_unicode_long_targets_at_plaintext_limit_minus_exact_plus_one() {
    let root = crate::test_support::TempDir::new("seal-native-boundary");
    let _store = super::super::TestStore::new(root.join("seals"));
    let key = super::super::key::load(true).unwrap().key;
    for size in [
        MAX_PLAINTEXT_ENVELOPE - 1,
        MAX_PLAINTEXT_ENVELOPE,
        MAX_PLAINTEXT_ENVELOPE + 1,
    ] {
        let session = root.subdir(&format!("1700000000_limit{size}"));
        let name = session.file_name().unwrap().to_str().unwrap();
        let (map, inventory, data) = near_limit_map(&session, &key, size);
        if size > MAX_PLAINTEXT_ENVELOPE {
            assert_eq!(
                encode_blob(&key, &data).unwrap_err(),
                "seal:v2_input_too_large"
            );
            assert_eq!(
                super::super::write_seal(&session, "", &map, &inventory).unwrap_err(),
                "seal:v2_input_too_large"
            );
            assert!(super::super::verified_seal(&session).is_err());
            continue;
        }
        let plain = envelope(&key, &data).unwrap();
        assert_eq!(plain.len(), size);
        let blob = encode_blob(&key, &data).unwrap();
        assert!(
            blob.len() > MAX_PLAINTEXT_ENVELOPE,
            "native DPAPI expansion must reproduce the former read-budget failure"
        );
        assert!(blob.len() <= super::super::MAX_PROTECTED_BLOB);
        assert_eq!(verify_blob(&key, name, &blob).unwrap(), data);
        super::super::write_seal(&session, "", &map, &inventory).unwrap();
        let seal = super::super::verified_seal(&session).unwrap();
        assert!(super::super::map_matches(&seal, &map));
        assert_eq!(seal.reg_digests, inventory);
        assert_eq!(seal.targets, data.targets);
        println!(
            "plaintext={size} protected={} targets={} registry_digests={}",
            blob.len(),
            map.len(),
            seal.reg_digests.len()
        );
    }
}

#[test]
fn seal_boundary_native_blob_budget_and_strict_plaintext_gate_are_independent() {
    let root = crate::test_support::TempDir::new("seal-small-compatibility");
    let _store = super::super::TestStore::new(root.join("seals"));
    let key = super::super::key::load(true).unwrap().key;
    let data = payload(
        "1700000000_strict_budget",
        &BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap();
    for size in [
        super::super::MAX_PROTECTED_BLOB - 1,
        super::super::MAX_PROTECTED_BLOB,
        super::super::MAX_PROTECTED_BLOB + 1,
    ] {
        let mut bytes = vec![0; size];
        bytes[..MAGIC.len()].copy_from_slice(MAGIC);
        let error = verify_blob(&key, &data.session, &bytes).unwrap_err();
        if size > super::super::MAX_PROTECTED_BLOB {
            assert_eq!(error, "seal:v2_bad_magic_or_length");
        } else {
            assert_eq!(error, "seal:unprotect_failed");
        }
    }
    // Valid DPAPI within the protected budget cannot bypass the plaintext cap.
    let mut oversized = MAGIC.to_vec();
    oversized.extend(super::super::protect(&vec![b' '; MAX_PLAINTEXT_ENVELOPE + 1]).unwrap());
    assert!(oversized.len() < super::super::MAX_PROTECTED_BLOB);
    assert_eq!(
        verify_blob(&key, &data.session, &oversized).unwrap_err(),
        "seal:v2_input_too_large"
    );
    // Old small RSEAL2 construction is unchanged, including HMAC frame and DPAPI.
    let mut old_small = MAGIC.to_vec();
    old_small.extend(super::super::protect(&envelope(&key, &data).unwrap()).unwrap());
    assert!(old_small.len() < 128 * 1024);
    assert_eq!(verify_blob(&key, &data.session, &old_small).unwrap(), data);
    let session = root.subdir(&data.session);
    std::fs::write(super::super::seal_path(&data.session), &old_small).unwrap();
    assert_eq!(
        super::super::verified_seal(&session).unwrap().map_digest,
        data.map_digest
    );
    let mut noncanonical = data;
    noncanonical.targets = vec!["C:/NOT-NORMALIZED".into()];
    assert_eq!(
        encode_blob(&key, &noncanonical).unwrap_err(),
        "seal:v2_noncanonical_targets"
    );
}
