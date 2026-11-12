use crate::path_seal::{self, BackupStage, FileManifest, PathMapSeal, RestoreStreams};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 64;

pub(super) enum Inventory {
    Complete(FileManifest),
    LegacyFiles(BTreeMap<String, String>),
}

impl Inventory {
    pub(super) fn read(
        session: &Path,
        seal: &PathMapSeal,
        cancelled: impl Fn() -> bool,
    ) -> Result<Option<Self>, String> {
        if let Some(expected) = &seal.file_manifest_digest {
            return path_seal::read_file_manifest_with_cancel(session, expected, cancelled)
                .map(|m| Some(Self::Complete(m)));
        }
        Ok(seal.file_digests.clone().map(Self::LegacyFiles))
    }

    pub(super) fn stage(
        &self,
        stage: &BackupStage,
        source: &Path,
        rel: &str,
    ) -> Result<StagedPayload, String> {
        let key = format!("files/{rel}");
        let directory = match self {
            Self::Complete(entries) => entries
                .get(&key)
                .ok_or_else(|| format!("skipped backup entry with no recorded digest: {rel}"))?
                .is_none(),
            Self::LegacyFiles(_) => crate::fsutil::scan_path_metadata(source)
                .map_err(|e| e.to_string())?
                .is_dir(),
        };
        let payload = StagedPayload {
            staged: stage.path().join(rel),
            directory,
        };
        match self {
            Self::Complete(entries) => stage_manifest(entries, source, &payload.staged, &key)?,
            Self::LegacyFiles(entries) => {
                let mut left = path_seal::MAX_PAYLOAD_NODES;
                let count = copy_legacy_tree(entries, source, &payload.staged, &key, &mut left, 0)?;
                let prefix = format!("{key}/");
                let expected = if directory {
                    entries
                        .range(prefix.clone()..)
                        .take_while(|(path, _)| path.starts_with(&prefix))
                        .count()
                } else {
                    1
                };
                if count != expected {
                    return Err(format!(
                        "skipped backup directory with missing recorded file: {rel}"
                    ));
                }
            }
        }
        stage.verify_snapshots()?;
        Ok(payload)
    }
}

fn stage_manifest(
    entries: &FileManifest,
    source: &Path,
    staged: &Path,
    root: &str,
) -> Result<(), String> {
    check_walk(root, path_seal::MAX_PAYLOAD_NODES, 0)?;
    let root_entry = entries.get(root).ok_or("seal:file_manifest_missing_root")?;
    copy_manifest_entry(source, staged, root, root_entry.as_deref())?;
    let prefix = format!("{root}/");
    // Lexicographic order places each declared directory before its children.
    // Never enumerate the public payload tree, whose extra names aren't sealed.
    for (index, (key, digest)) in entries
        .range(prefix.clone()..)
        .take_while(|(key, _)| key.starts_with(&prefix))
        .enumerate()
    {
        let tail = &key[prefix.len()..];
        let mut depth = 0;
        for name in tail.split('/') {
            if name.is_empty()
                || name.ends_with(['.', ' '])
                || name.encode_utf16().count() > 255
                || name.contains(['\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
            {
                return Err(format!("seal:file_manifest_invalid_entry: {key}"));
            }
            depth += 1;
        }
        check_walk(
            key,
            path_seal::MAX_PAYLOAD_NODES.saturating_sub(index + 1),
            depth,
        )?;
        if key
            .rsplit_once('/')
            .and_then(|(parent, _)| entries.get(parent))
            != Some(&None)
        {
            return Err(format!("seal:file_manifest_missing_parent: {key}"));
        }
        copy_manifest_entry(
            &source.join(tail),
            &staged.join(tail),
            key,
            digest.as_deref(),
        )?;
    }
    Ok(())
}

fn copy_manifest_entry(
    source: &Path,
    staged: &Path,
    key: &str,
    digest: Option<&str>,
) -> Result<(), String> {
    if let Some(digest) = digest {
        copy_checked_file(
            source,
            staged,
            key,
            digest,
            RestoreStreams::AuthenticatedDefault,
        )
    } else {
        let metadata =
            crate::fsutil::scan_path_metadata(source).map_err(|err| read_error(key, err))?;
        if !metadata.is_dir() {
            return Err(format!(
                "backup entry type differs from the manifest: {key}"
            ));
        }
        fs::create_dir(staged).map_err(|err| err.to_string())
    }
}

fn check_walk(key: &str, left: usize, depth: usize) -> Result<(), String> {
    if crate::executor::cleanup_cancelled() {
        return Err("cleanup cancelled".into());
    }
    if left == 0 || depth > MAX_DEPTH {
        let rel = key.strip_prefix("files/").unwrap_or(key);
        return Err(format!(
            "skipped backup directory too large to verify: {rel}"
        ));
    }
    Ok(())
}

fn read_error(key: &str, err: io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        let rel = key.strip_prefix("files/").unwrap_or(key);
        format!("skipped backup directory with missing recorded file: {rel}")
    } else {
        err.to_string()
    }
}

fn copy_checked_file(
    source: &Path,
    staged: &Path,
    key: &str,
    expected: &str,
    streams: RestoreStreams,
) -> Result<(), String> {
    let source =
        crate::fsutil::open_local_regular_file(source).map_err(|err| read_error(key, err))?;
    let actual = path_seal::stage_payload_file(&source, staged, streams)?;
    if actual != expected {
        let rel = key.strip_prefix("files/").unwrap_or(key);
        return Err(format!(
            "skipped modified backup entry (digest mismatch): {rel}"
        ));
    }
    Ok(())
}

fn copy_legacy_tree(
    entries: &BTreeMap<String, String>,
    source: &Path,
    staged: &Path,
    key: &str,
    left: &mut usize,
    depth: usize,
) -> Result<usize, String> {
    check_walk(key, *left, depth)?;
    *left -= 1;
    let directory = crate::fsutil::scan_path_metadata(source)
        .map_err(|err| err.to_string())?
        .is_dir();
    if directory {
        let source_pin =
            crate::fsutil::pin_target_readonly(source).map_err(|err| err.to_string())?;
        source_pin
            .check_data_streams()
            .map_err(|err| err.to_string())?;
        fs::create_dir(staged).map_err(|err| err.to_string())?;
        let names =
            crate::fsutil::read_local_entry_names(source, *left).map_err(|err| err.to_string())?;
        let mut count = 0;
        for name in names {
            let name_str = name
                .to_str()
                .ok_or("backup file name is not valid Unicode")?;
            count += copy_legacy_tree(
                entries,
                &source.join(&name),
                &staged.join(&name),
                &format!("{key}/{name_str}"),
                left,
                depth + 1,
            )?;
        }
        source_pin
            .check_data_streams()
            .map_err(|err| err.to_string())?;
        return Ok(count);
    }

    let expected = entries.get(key).ok_or_else(|| {
        let kind = if depth == 0 {
            "backup entry with no recorded digest"
        } else {
            "unrecorded backup file"
        };
        let rel = key.strip_prefix("files/").unwrap_or(key);
        format!("skipped {kind}: {rel}")
    })?;
    copy_checked_file(source, staged, key, expected, RestoreStreams::RefuseNamed)?;
    Ok(1)
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
    pub(super) fn path(&self) -> &Path {
        &self.staged
    }
}
