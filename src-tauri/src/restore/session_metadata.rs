use super::{is_safe_map_rel, RegistryRestoreBudget};
use crate::path_seal::PathMapSeal;
use std::fs;
use std::io;
use std::path::Path;

pub(super) fn io_status(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "seal:read_requires_admin",
        _ => "seal:metadata_unreadable",
    }
}

pub(super) fn is_empty_unsealed_session(session: &Path) -> io::Result<bool> {
    let root = fs::symlink_metadata(session)?;
    if !root.is_dir() || crate::fsutil::is_reparse_metadata(&root) {
        return Ok(false);
    }
    for entry in fs::read_dir(session)? {
        let entry = entry?;
        if entry.file_name() != "files" && entry.file_name() != "registry" {
            return Ok(false);
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir()
            || crate::fsutil::is_reparse_metadata(&metadata)
            || fs::read_dir(entry.path())?.next().transpose()?.is_some()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn snapshot_size(path: &Path, missing: &str) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            missing
        } else {
            io_status(&error)
        }
    })?;
    if !metadata.is_file() || crate::fsutil::is_reparse_metadata(&metadata) {
        return Err("seal:invalid_metadata_object".into());
    }
    Ok(metadata.len())
}

/// Check the critical metadata named by the seal without parsing the manifest,
/// opening registry contents, or enumerating any backed-up directory tree.
pub(super) fn check_session_metadata(
    session: &Path,
    seal: &PathMapSeal,
    cancelled: impl Fn() -> bool,
) -> Result<(), String> {
    if !crate::path_seal::map_matches(seal, &Default::default()) {
        snapshot_size(&session.join("files/path_map.json"), "seal:map_missing")?;
    }
    #[cfg(windows)]
    if seal.file_manifest_digest.is_some() {
        snapshot_size(
            &session.join(crate::path_seal::FILE_MANIFEST_NAME),
            "seal:file_manifest_missing",
        )?;
    }
    if seal.reg_digests.contains_key("path.json") {
        snapshot_size(&session.join("path.json"), "seal:path_snapshot_missing")?;
    }
    let mut budget = RegistryRestoreBudget::default();
    for rel in seal
        .reg_digests
        .keys()
        .filter(|key| key.starts_with("registry/"))
    {
        if cancelled() {
            return Err("restore cancelled before any changes".into());
        }
        let (entry, file) = rel["registry/".len()..]
            .split_once('/')
            .ok_or("seal:bad_export_name")?;
        if !is_safe_map_rel(entry) || !matches!(file, "value.reg" | "export.reg") {
            return Err("seal:bad_export_name".into());
        }
        let size = snapshot_size(&session.join(rel), "seal:registry_snapshot_missing")?;
        budget.add(rel, size)?;
    }
    Ok(())
}
