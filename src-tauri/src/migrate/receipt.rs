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

fn receipt_root(_target: &Path) -> io::Result<PathBuf> {
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
    Ok(root)
}

fn receipt_filename(key: &str) -> String {
    format!("{}.json", crate::path_seal::map_digest(key.as_bytes()))
}

fn comparison_key(path: &str) -> String {
    lower(Path::new(path)).replace('/', "\\")
}

impl Receipt {
    fn find(target: &Path, matches: impl Fn(&Self) -> bool) -> Option<Self> {
        let root = receipt_root(target).ok()?;
        let canonical = receipt_filename(&comparison_key(&target.to_string_lossy()));
        let read = |path: &Path| {
            let mut saved: Self =
                serde_json::from_str(&fsutil::read_local_scan_text(path).ok()?).ok()?;
            if path.file_name()? != std::ffi::OsStr::new(&receipt_filename(&saved.target)) {
                return None;
            }
            saved.source = comparison_key(&saved.source);
            saved.target = comparison_key(&saved.target);
            matches(&saved).then_some(saved)
        };
        if let Some(saved) = read(&root.join(&canonical)) {
            return Some(saved);
        }

        // Older versions hashed the spelling supplied by the caller, including
        // any forward slashes. Discover that key without guessing its spelling.
        let _pins = fsutil::pin_local_scan_path(&root).ok()?;
        std::fs::read_dir(root)
            .ok()?
            .take(4096)
            .filter_map(Result::ok)
            .find_map(|entry| {
                let name = entry.file_name();
                let (digest, extension) = name.to_str()?.rsplit_once('.')?;
                if name == std::ffi::OsStr::new(&canonical)
                    || extension != "json"
                    || digest.len() != 64
                    || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return None;
                }
                read(&entry.path())
            })
    }

    pub(super) fn new(source: &Path, target: &Path) -> io::Result<Self> {
        Ok(Self {
            source: comparison_key(&source.to_string_lossy()),
            target: comparison_key(&target.to_string_lossy()),
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
        let mut current = Self::new(source_location, target).ok()?;
        current.source = comparison_key(&source.to_string_lossy());
        Self::find(target, |saved| {
            !saved.cleanup_pending
                && saved.source == current.source
                && saved.target == current.target
                && saved.source_id == current.source_id
                && saved.target_id == current.target_id
        })
    }

    pub(super) fn prepare_cleanup(
        source: &Path,
        target: &Path,
        source_id: (u32, u64),
        target_id: (u32, u64),
    ) -> io::Result<()> {
        Self {
            source: comparison_key(&source.to_string_lossy()),
            target: comparison_key(&target.to_string_lossy()),
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
        let source = comparison_key(&source.to_string_lossy());
        targets.into_iter().any(|target| {
            let target_key = comparison_key(&target.to_string_lossy());
            Self::find(target, |receipt| {
                receipt.cleanup_pending
                    && receipt.complete
                    && receipt.source == source
                    && receipt.target == target_key
                    && receipt.source_id == source_id
                    && receipt.target_id == target_id
            })
            .is_some()
        })
    }

    pub(super) fn finished_with_other_backup(
        source: &Path,
        targets: [&Path; 2],
        target_id: (u32, u64),
        misplaced_id: (u32, u64),
    ) -> bool {
        let source = comparison_key(&source.to_string_lossy());
        targets.into_iter().any(|target| {
            let target_key = comparison_key(&target.to_string_lossy());
            Self::find(target, |receipt| {
                receipt.cleanup_pending
                    && receipt.complete
                    && receipt.source == source
                    && receipt.target == target_key
                    && receipt.target_id == target_id
                    && receipt.source_id != misplaced_id
            })
            .is_some()
        })
    }

    pub(super) fn save(&self, target: &Path) -> io::Result<()> {
        let root = receipt_root(target)?;
        let _pins = fsutil::create_dirs_pinned(&root)?;
        let path = root.join(receipt_filename(&self.target));
        fsutil::write_bytes_atomic(&path, &serde_json::to_vec(self)?)
    }
}
