use super::lower;
use crate::fsutil;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// The receipt lets an interrupted copy resume after restart. It is never
/// permission to overwrite a file; every existing target entry is checked.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    source: String,
    target: String,
    source_id: (u32, u64),
    target_id: (u32, u64),
    pub(super) complete: bool,
    #[serde(default)]
    cleanup_pending: bool,
}

fn receipt_path(_target: &Path, key: &str) -> io::Result<PathBuf> {
    let digest = crate::path_seal::map_digest(key.as_bytes());
    if digest.is_empty() {
        return Err(io::Error::other("could not identify the copy target"));
    }
    #[cfg(test)]
    let root = _target
        .parent()
        .ok_or_else(|| io::Error::other("no fixture parent"))?
        .join("receipts");
    #[cfg(all(windows, not(test)))]
    let root = crate::sysops::known_folder(&windows::Win32::UI::Shell::FOLDERID_LocalAppData)
        .ok_or_else(|| io::Error::other("LocalAppData is unavailable"))?
        .join("Remova")
        .join("migration-copies");
    #[cfg(all(not(windows), not(test)))]
    let root = return Err(io::Error::other("directory migration requires Windows"));
    Ok(root.join(format!("{digest}.json")))
}

impl Receipt {
    fn read(target: &Path) -> Option<Self> {
        let key = lower(target);
        let legacy = target.to_string_lossy().to_lowercase();
        let mut keys = vec![
            key.clone(),
            legacy.trim_end_matches('\\').to_string(),
            format!("{key}/"),
        ];
        keys.dedup();
        keys.into_iter().find_map(|key| {
            let path = receipt_path(target, &key).ok()?;
            let mut saved: Self =
                serde_json::from_str(&fsutil::read_local_scan_text(&path).ok()?).ok()?;
            saved.source = lower(Path::new(&saved.source));
            saved.target = lower(Path::new(&saved.target));
            Some(saved)
        })
    }

    pub(super) fn new(source: &Path, target: &Path) -> io::Result<Self> {
        Ok(Self {
            source: lower(source),
            target: lower(target),
            source_id: fsutil::local_directory_id(source)?,
            target_id: fsutil::local_directory_id(target)?,
            complete: false,
            cleanup_pending: false,
        })
    }

    pub(super) fn load(source: &Path, target: &Path) -> Option<Self> {
        Self::load_with_source_at(source, source, target)
    }

    pub(super) fn load_with_source_at(
        source: &Path,
        source_location: &Path,
        target: &Path,
    ) -> Option<Self> {
        let saved = Self::read(target)?;
        let mut current = Self::new(source_location, target).ok()?;
        current.source = lower(source);
        current.complete = saved.complete;
        (saved == current).then_some(saved)
    }

    pub(super) fn prepare_cleanup(
        source: &Path,
        target: &Path,
        source_id: (u32, u64),
        target_id: (u32, u64),
    ) -> io::Result<()> {
        Self {
            source: lower(source),
            target: lower(target),
            source_id,
            target_id,
            complete: true,
            cleanup_pending: true,
        }
        .save(target)
    }

    pub(super) fn matches_cleanup(
        source: &Path,
        targets: [&Path; 2],
        source_id: (u32, u64),
        target_id: (u32, u64),
    ) -> bool {
        // The current junction path is stable across equivalent requests.
        // Older receipts may have used the spelling supplied by the caller.
        let source = lower(source);
        targets.into_iter().any(|target| {
            Self::read(target).is_some_and(|receipt| {
                receipt.cleanup_pending
                    && receipt.complete
                    && receipt.source == source
                    && receipt.target == lower(target)
                    && receipt.source_id == source_id
                    && receipt.target_id == target_id
            })
        })
    }

    pub(super) fn finished_with_other_backup(
        source: &Path,
        targets: [&Path; 2],
        target_id: (u32, u64),
        misplaced_id: (u32, u64),
    ) -> bool {
        let source = lower(source);
        targets.into_iter().any(|target| {
            Self::read(target).is_some_and(|receipt| {
                receipt.cleanup_pending
                    && receipt.complete
                    && receipt.source == source
                    && receipt.target == lower(target)
                    && receipt.target_id == target_id
                    && receipt.source_id != misplaced_id
            })
        })
    }

    pub(super) fn save(&self, target: &Path) -> io::Result<()> {
        let path = receipt_path(target, &lower(target))?;
        let _pins = fsutil::create_dirs_pinned(
            path.parent()
                .ok_or_else(|| io::Error::other("no receipt directory"))?,
        )?;
        fsutil::write_bytes_atomic(&path, &serde_json::to_vec(self)?)
    }
}
