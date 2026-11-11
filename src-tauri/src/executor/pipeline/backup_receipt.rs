use crate::scanner::CleanupItem;
use std::io;
use std::path::Path;

#[cfg(windows)]
use crate::scanner::ItemKind;
#[cfg(windows)]
use std::collections::BTreeMap;

#[cfg(windows)]
const MAX_PATH_MAP_BYTES: u64 = 64 * 1024 * 1024;

pub(super) enum FilesystemDeleteError {
    Unverified(String),
    Io(io::Error),
}

#[derive(Default)]
pub(super) struct BackupReceipt {
    #[cfg(windows)]
    identities: BTreeMap<String, Result<crate::fsutil::CleanupTreeIdentity, String>>,
    #[cfg(windows)]
    keys: BTreeMap<String, String>,
    #[cfg(windows)]
    manifest: crate::path_seal::FileManifest,
}

impl BackupReceipt {
    pub(super) fn capture(items: &[CleanupItem]) -> Self {
        #[cfg(windows)]
        {
            let identities = items
                .iter()
                .filter(|item| matches!(item.kind, ItemKind::File | ItemKind::Dir))
                .map(|item| {
                    let identity = crate::fsutil::CleanupTreeIdentity::capture(
                        Path::new(&item.path),
                        crate::path_seal::MAX_PAYLOAD_NODES,
                    )
                    .map_err(|error| error.to_string());
                    (item.path.clone(), identity)
                })
                .collect();
            Self {
                identities,
                ..Self::default()
            }
        }
        #[cfg(not(windows))]
        {
            let _ = items;
            Self::default()
        }
    }

    #[cfg(windows)]
    pub(super) fn authenticate(mut self, session: &Path) -> Result<Self, String> {
        use std::io::Read;

        if self.identities.is_empty() {
            return Ok(self);
        }
        let seal = crate::path_seal::verified_seal(session)?;
        let expected = seal
            .file_manifest_digest
            .as_deref()
            .ok_or("seal:missing_file_manifest")?;
        let file = crate::fsutil::open_local_regular_file(&session.join("files/path_map.json"))
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take(MAX_PATH_MAP_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_PATH_MAP_BYTES {
            return Err("seal:path_map_too_large".into());
        }
        let map: BTreeMap<String, String> =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if !crate::path_seal::map_matches(&seal, &map) {
            return Err("seal:map_mismatch".into());
        }
        self.keys = map
            .into_iter()
            .map(|(key, source)| (source, format!("files/{key}")))
            .collect();
        self.manifest = crate::path_seal::read_file_manifest(session, expected)?;
        Ok(self)
    }

    #[cfg(not(windows))]
    pub(super) fn authenticate(self, _: &Path) -> Result<Self, String> {
        Err("seal:windows_required".into())
    }

    #[cfg(windows)]
    pub(super) fn delete(&self, path: &Path) -> Result<(), FilesystemDeleteError> {
        let unverified = FilesystemDeleteError::Unverified;
        let source = path
            .to_str()
            .ok_or_else(|| unverified("invalid source path".into()))?;
        let root_key = self
            .keys
            .get(source)
            .ok_or_else(|| unverified("source was not backed up".into()))?;
        let identities = self
            .identities
            .get(source)
            .ok_or_else(|| unverified("source identity was not captured".into()))?
            .as_ref()
            .map_err(|error| unverified(error.clone()))?;
        let tree = crate::fsutil::PinnedLocalDeleteTree::with_entry_limit(
            path,
            crate::path_seal::MAX_PAYLOAD_NODES,
        )
        .map_err(|error| unverified(error.to_string()))?;
        tree.verify_cleanup_backup(identities, &self.manifest, root_key)
            .map_err(|error| unverified(error.to_string()))?;
        tree.remove_backed_up().map_err(FilesystemDeleteError::Io)
    }

    #[cfg(not(windows))]
    pub(super) fn delete(&self, _: &Path) -> Result<(), FilesystemDeleteError> {
        Err(FilesystemDeleteError::Unverified(
            "backup verification requires Windows".into(),
        ))
    }
}
