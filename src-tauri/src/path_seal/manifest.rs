use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::Path;

pub(crate) const FILE_MANIFEST_NAME: &str = "payload-manifest.json";
const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;

/// Keys are exact paths relative to the session, using forward slashes.
/// Files have a SHA-256 digest. Directory entries record empty directories too.
pub(crate) type FileManifest = BTreeMap<String, Option<String>>;

#[derive(Default)]
struct BoundedManifest(Vec<u8>);

impl Write for BoundedManifest {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_MANIFEST_BYTES - self.0.len() {
            return Err(io::Error::other("seal:file_manifest_too_large"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn write_file_manifest(
    session: &Path,
    entries: &FileManifest,
) -> Result<String, String> {
    let mut manifest = BoundedManifest::default();
    serde_json::to_writer(&mut manifest, entries).map_err(|e| e.to_string())?;
    let bytes = manifest.0;
    let digest = super::v2::digest_payload(bytes.as_slice(), io::sink())?;
    crate::fsutil::write_bytes_atomic(&session.join(FILE_MANIFEST_NAME), &bytes)
        .map_err(|e| e.to_string())?;
    Ok(digest)
}

pub(super) fn open_verified_manifest(
    session: &Path,
    expected: &str,
    cancelled: impl Fn() -> bool,
) -> Result<(FileManifest, std::fs::File), String> {
    let file = crate::fsutil::open_local_regular_file(&session.join(FILE_MANIFEST_NAME))
        .map_err(|e| format!("seal:file_manifest_read: {e}"))?;
    let mut manifest = BoundedManifest::default();
    let actual = super::v2::digest_payload_with_cancel(
        (&file).take(MAX_MANIFEST_BYTES as u64 + 1),
        &mut manifest,
        cancelled,
    )?;
    if actual != expected {
        return Err("seal:file_manifest_mismatch".into());
    }
    let entries =
        serde_json::from_slice(&manifest.0).map_err(|e| format!("seal:file_manifest_json: {e}"))?;
    Ok((entries, file))
}

pub(crate) fn read_file_manifest(session: &Path, expected: &str) -> Result<FileManifest, String> {
    read_file_manifest_with_cancel(session, expected, crate::executor::cleanup_cancelled)
}

pub(crate) fn read_file_manifest_with_cancel(
    session: &Path,
    expected: &str,
    cancelled: impl Fn() -> bool,
) -> Result<FileManifest, String> {
    open_verified_manifest(session, expected, cancelled).map(|(entries, _pin)| entries)
}
