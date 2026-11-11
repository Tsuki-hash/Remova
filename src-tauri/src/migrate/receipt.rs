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
}

fn receipt_path(target: &Path) -> io::Result<PathBuf> {
    let digest = crate::path_seal::map_digest(lower(target).as_bytes());
    if digest.is_empty() {
        return Err(io::Error::other("could not identify the copy target"));
    }
    #[cfg(test)]
    let root = target
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
    pub(super) fn new(source: &Path, target: &Path) -> io::Result<Self> {
        Ok(Self {
            source: lower(source),
            target: lower(target),
            source_id: fsutil::local_directory_id(source)?,
            target_id: fsutil::local_directory_id(target)?,
            complete: false,
        })
    }

    pub(super) fn load(source: &Path, target: &Path) -> Option<Self> {
        let path = receipt_path(target).ok()?;
        let saved: Self = serde_json::from_str(&fsutil::read_local_scan_text(&path).ok()?).ok()?;
        let mut current = Self::new(source, target).ok()?;
        current.complete = saved.complete;
        (saved == current).then_some(saved)
    }

    pub(super) fn save(&self, target: &Path) -> io::Result<()> {
        let path = receipt_path(target)?;
        let _pins = fsutil::create_dirs_pinned(
            path.parent()
                .ok_or_else(|| io::Error::other("no receipt directory"))?,
        )?;
        fsutil::write_bytes_atomic(&path, &serde_json::to_vec(self)?)
    }
}
