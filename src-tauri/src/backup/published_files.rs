use crate::fsutil::PinnedLocalDeleteTree;
use crate::path_seal::{self, FileManifest, PathMapSeal};
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// Bind cleanup to the authenticated inventory and keep the on-disk
/// metadata readable for a later restore.
pub(crate) struct PublishedFiles {
    session: PathBuf,
    keys: BTreeMap<String, String>,
    pub(crate) manifest: FileManifest,
    _metadata_pins: [File; 2],
}

impl PublishedFiles {
    pub(crate) fn open(session: &Path, seal: &PathMapSeal) -> Result<Self, String> {
        let expected = seal
            .file_manifest_digest
            .as_deref()
            .ok_or("seal:missing_file_manifest")?;
        let (bytes, map_pin) = super::read_backup_metadata(
            &session.join("files/path_map.json"),
            "seal:path_map_too_large",
            crate::executor::cleanup_cancelled,
        )?;
        let map: BTreeMap<String, String> =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if !path_seal::map_matches(seal, &map) {
            return Err("seal:map_mismatch".into());
        }
        let (manifest, manifest_pin) = path_seal::open_verified_manifest(
            session,
            expected,
            crate::executor::cleanup_cancelled,
        )?;
        Ok(Self {
            session: session.to_owned(),
            keys: map
                .into_iter()
                .map(|(key, source)| (source, format!("files/{key}")))
                .collect(),
            manifest,
            _metadata_pins: [map_pin, manifest_pin],
        })
    }

    pub(crate) fn key_for(&self, source: &Path) -> io::Result<&str> {
        let source = source
            .to_str()
            .ok_or_else(|| io::Error::other("invalid backup source path"))?;
        self.keys
            .get(source)
            .map(String::as_str)
            .ok_or_else(|| io::Error::other("source was not backed up"))
    }

    pub(crate) fn hold(&self, root_key: &str) -> io::Result<PinnedLocalDeleteTree> {
        PinnedLocalDeleteTree::hold_backup_payload(
            &self.session.join(root_key),
            &self.manifest,
            root_key,
        )
    }
}
