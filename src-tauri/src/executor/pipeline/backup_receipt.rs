use crate::scanner::CleanupItem;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

#[cfg(windows)]
use crate::scanner::ItemKind;

pub(super) enum FilesystemDeleteError {
    Unverified(String),
    BackupUnavailable(io::Error),
    Io(io::Error),
}

#[derive(Default)]
pub(super) struct BackupReceipt {
    #[cfg(windows)]
    identities: BTreeMap<String, Result<crate::fsutil::CleanupTreeIdentity, String>>,
    #[cfg(windows)]
    files: Option<crate::backup::PublishedFiles>,
    #[cfg(windows)]
    metadata_pins: Vec<std::fs::File>,
    #[cfg(windows)]
    paths_selected: bool,
    #[cfg(all(windows, not(test)))]
    registry: BTreeMap<String, Result<crate::regops::RegistrySource, String>>,
    path_plan: Option<crate::regops::BackedPathPlan>,
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
                paths_selected: items.iter().any(|item| matches!(item.kind, ItemKind::Path)),
                #[cfg(not(test))]
                registry: items
                    .iter()
                    .filter(|item| matches!(item.kind, ItemKind::Registry))
                    .map(|item| {
                        let source =
                            crate::regops::RegistrySource::capture(&item.path).and_then(|source| {
                                source.ok_or("registry source was missing before backup".into())
                            });
                        (item.path.clone(), source)
                    })
                    .collect(),
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

        let (seal, seal_pin) = crate::path_seal::hold_verified_seal(session)?;
        self.metadata_pins.push(seal_pin);
        if self.paths_selected {
            let (bytes, file) = crate::backup::read_backup_metadata(
                &session.join("path.json"),
                "seal:path_snapshot_too_large",
                crate::executor::cleanup_cancelled,
            )?;
            if !crate::path_seal::snapshot_matches(&seal, "path.json", &bytes) {
                return Err("seal:path_snapshot_mismatch".into());
            }
            let snapshot: crate::backup::PathSnapshot =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            self.path_plan = Some(
                snapshot
                    .cleanup_plan
                    .ok_or("seal:missing_path_cleanup_plan")?,
            );
            self.metadata_pins.push(file);
        }
        // Restore preflight also needs the registry snapshots in a mixed
        // session, even when the item being removed is a filesystem object.
        let mut budget = crate::restore::RegistryRestoreBudget::default();
        for (relative, digest) in seal
            .reg_digests
            .iter()
            .filter(|(key, _)| key.starts_with("registry/"))
        {
            if relative.contains(['\\', ':'])
                || relative
                    .split('/')
                    .any(|part| matches!(part, "" | "." | ".."))
            {
                return Err("seal:bad_export_name".into());
            }
            let file = crate::fsutil::open_local_regular_file(&session.join(relative))
                .map_err(|error| error.to_string())?;
            let size = file.metadata().map_err(|error| error.to_string())?.len();
            budget.add(relative, size)?;
            if crate::path_seal::publication_reader_digest((&file).take(size + 1))? != *digest {
                return Err(format!("seal:registry_snapshot_mismatch: {relative}"));
            }
            self.metadata_pins.push(file);
        }
        #[cfg(not(test))]
        for (target, receipt) in &mut self.registry {
            if let Ok(source) = receipt {
                let relative =
                    crate::backup::registry_restore_snapshot(target, target.contains('|'));
                if !seal
                    .reg_digests
                    .get(&relative)
                    .is_some_and(|digest| source.matches_backup(digest))
                {
                    *receipt = Err("registry source could not be bound to its backup".into());
                }
            }
        }
        self.files = Some(crate::backup::PublishedFiles::open(session, &seal)?);
        Ok(self)
    }

    #[cfg(not(windows))]
    pub(super) fn authenticate(self, _: &Path) -> Result<Self, String> {
        Err("seal:windows_required".into())
    }

    pub(super) fn scrub_paths(&self) -> BTreeMap<String, crate::regops::BackedPathResult> {
        self.path_plan
            .as_ref()
            .map(crate::regops::BackedPathPlan::scrub)
            .unwrap_or_default()
    }

    #[cfg(all(windows, not(test)))]
    pub(super) fn verify_registry(&self, path: &str) -> Result<bool, String> {
        self.registry
            .get(path)
            .ok_or("no registry backup receipt")?
            .as_ref()
            .map_err(Clone::clone)?
            .verify()
    }

    #[cfg(any(not(windows), test))]
    pub(super) fn verify_registry(&self, _: &str) -> Result<bool, String> {
        Err("no registry backup receipt".into())
    }

    pub(super) fn delete_registry_value(&self, path: &str) -> Option<Result<bool, String>> {
        #[cfg(all(windows, not(test)))]
        if let Some(Ok(source)) = self.registry.get(path) {
            return source.delete_value(path);
        }
        let _ = path;
        None
    }

    #[cfg(windows)]
    pub(super) fn delete(&self, path: &Path) -> Result<(), FilesystemDeleteError> {
        let unverified = FilesystemDeleteError::Unverified;
        let source = path
            .to_str()
            .ok_or_else(|| unverified("invalid source path".into()))?;
        let files = self
            .files
            .as_ref()
            .ok_or_else(|| unverified("no authenticated file manifest".into()))?;
        let root_key = files
            .key_for(path)
            .map_err(|error| unverified(error.to_string()))?;
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
        tree.verify_cleanup_backup(identities, &files.manifest, root_key)
            .map_err(|error| unverified(error.to_string()))?;
        let _backup = files
            .hold(root_key)
            .map_err(FilesystemDeleteError::BackupUnavailable)?;
        tree.remove_backed_up().map_err(FilesystemDeleteError::Io)
    }

    #[cfg(not(windows))]
    pub(super) fn delete(&self, _: &Path) -> Result<(), FilesystemDeleteError> {
        Err(FilesystemDeleteError::Unverified(
            "backup verification requires Windows".into(),
        ))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn cleanup_receipt_keeps_the_registry_exports_needed_for_restore() {
        let root = crate::test_support::TempDir::new("backup-registry-receipt");
        let _store = crate::path_seal::TestStore::new(root.join("seals"));
        let stage = root.subdir("stage");
        std::fs::create_dir(stage.join("files")).unwrap();
        std::fs::create_dir_all(stage.join("registry/App")).unwrap();
        let relative = "registry/App/export.reg";
        std::fs::write(
            stage.join(relative),
            crate::restore::encode_reg_text(
                "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\ReceiptFixture]\r\n\"Example\"=\"fixture\"\r\n",
            ),
        )
        .unwrap();
        let digests = crate::path_seal::collect_reg_digests(&stage).unwrap();
        let session = root.subdir("1700000000_receipt");
        crate::backup::publish_verified_stage(&stage, &session, &BTreeMap::new(), &digests, |_| {
            Ok(())
        })
        .unwrap();
        let receipt = BackupReceipt::default().authenticate(&session).unwrap();
        let exported = session.join(relative);
        assert!(std::fs::remove_file(&exported).is_err());
        assert!(std::fs::write(&exported, b"replacement").is_err());
        drop(receipt);
        std::fs::remove_file(exported).unwrap();
        assert!(BackupReceipt::default().authenticate(&session).is_err());
    }
}
