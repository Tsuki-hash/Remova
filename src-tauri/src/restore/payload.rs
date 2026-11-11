use crate::path_seal::{self, BackupStage, FileManifest, PathMapSeal};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const DIR_VERIFY_BUDGET: usize = 200_000;

pub(super) enum Inventory<'a> {
    Complete(FileManifest),
    LegacyFiles(&'a BTreeMap<String, String>),
}

impl<'a> Inventory<'a> {
    pub(super) fn read(session: &Path, seal: &'a PathMapSeal) -> Result<Option<Self>, String> {
        if let Some(expected) = &seal.file_manifest_digest {
            return path_seal::read_file_manifest(session, expected)
                .map(|m| Some(Self::Complete(m)));
        }
        Ok(seal.file_digests.as_ref().map(Self::LegacyFiles))
    }

    fn expected_count(&self, key: &str, directory: bool) -> usize {
        if !directory {
            return 1;
        }
        let prefix = format!("{key}/");
        match self {
            Self::Complete(entries) => {
                1 + entries
                    .range(prefix.clone()..)
                    .take_while(|(path, _)| path.starts_with(&prefix))
                    .count()
            }
            Self::LegacyFiles(entries) => entries
                .range(prefix.clone()..)
                .take_while(|(path, _)| path.starts_with(&prefix))
                .count(),
        }
    }

    pub(super) fn stage(
        &self,
        stage: &BackupStage,
        source: &Path,
        rel: &str,
    ) -> Result<StagedPayload, String> {
        let key = format!("files/{rel}");
        let directory = crate::fsutil::scan_path_metadata(source)
            .map_err(|e| e.to_string())?
            .is_dir();
        let payload = StagedPayload {
            staged: stage.path().join(rel),
            directory,
        };
        let mut left = DIR_VERIFY_BUDGET;
        let count = self.copy_tree(source, &payload.staged, &key, directory, &mut left, 0)?;
        if count != self.expected_count(&key, directory) {
            return Err(format!(
                "skipped backup directory with missing recorded file: {rel}"
            ));
        }
        stage.verify_snapshots()?;
        Ok(payload)
    }

    fn copy_tree(
        &self,
        source: &Path,
        staged: &Path,
        key: &str,
        directory: bool,
        left: &mut usize,
        depth: usize,
    ) -> Result<usize, String> {
        let rel = key.strip_prefix("files/").unwrap_or(key);
        if crate::executor::cleanup_cancelled() {
            return Err("cleanup cancelled".into());
        }
        if *left == 0 || depth > 64 {
            return Err(format!(
                "skipped backup directory too large to verify: {rel}"
            ));
        }
        *left -= 1;
        if directory {
            let count_root = match self {
                Self::Complete(entries) if entries.get(key) != Some(&None) => {
                    return Err(format!("skipped unrecorded backup directory: {rel}"));
                }
                Self::Complete(_) => 1,
                Self::LegacyFiles(_) => 0,
            };
            fs::create_dir(staged).map_err(|e| e.to_string())?;
            let names =
                crate::fsutil::read_local_entry_names(source, *left).map_err(|e| e.to_string())?;
            let mut count = count_root;
            for name in names {
                let path = source.join(&name);
                let child_dir = crate::fsutil::scan_path_metadata(&path)
                    .map_err(|e| e.to_string())?
                    .is_dir();
                let name_str = name
                    .to_str()
                    .ok_or("backup file name is not valid Unicode")?;
                let child_key = format!("{key}/{name_str}");
                count += self.copy_tree(
                    &path,
                    &staged.join(name),
                    &child_key,
                    child_dir,
                    left,
                    depth + 1,
                )?;
            }
            return Ok(count);
        }

        let expected = match self {
            Self::Complete(entries) => entries.get(key).and_then(Option::as_ref),
            Self::LegacyFiles(entries) => entries.get(key),
        };
        let Some(expected) = expected else {
            let kind = if depth == 0 {
                "backup entry with no recorded digest"
            } else {
                "unrecorded backup file"
            };
            return Err(format!("skipped {kind}: {rel}"));
        };
        let actual = path_seal::stage_payload_file(source, staged)?;
        if &actual != expected {
            return Err(format!(
                "skipped modified backup entry (digest mismatch): {rel}"
            ));
        }
        Ok(1)
    }
}

pub(super) struct StagedPayload {
    staged: PathBuf,
    directory: bool,
}

impl Drop for StagedPayload {
    fn drop(&mut self) {
        if self.directory {
            let _ = fs::remove_dir_all(&self.staged);
        } else {
            let _ = fs::remove_file(&self.staged);
        }
    }
}

impl StagedPayload {
    pub(super) fn publish(&self, destination: &Path) -> std::io::Result<()> {
        // Only files in the privileged stage are copied. The original backup
        // path can change after staging without changing the restored bytes.
        if self.directory {
            crate::fsutil::copy_dir(&self.staged, destination)
        } else {
            crate::fsutil::copy_file_no_reparse_pinned(&self.staged, destination).map(|_| ())
        }
    }
}
