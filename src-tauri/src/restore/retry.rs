//! Private, write-ahead records of the directories created during a restore.

use crate::path_seal::{map_digest, PathMapSeal, RestoreReceiptStore, MAX_PAYLOAD_NODES};
use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::os::windows::io::AsRawHandle;
use std::path::Path;

const KEY_SIZE: usize = 64;
const ID_SIZE: usize = 24;
const RECORD_SIZE: usize = KEY_SIZE + ID_SIZE;
const MAX_BYTES: u64 = (RECORD_SIZE * MAX_PAYLOAD_NODES * 2) as u64;

pub(super) struct Receipts<'a> {
    seal: &'a PathMapSeal,
    store: OnceCell<Result<RestoreReceiptStore, String>>,
}

impl<'a> Receipts<'a> {
    pub(super) fn new(seal: &'a PathMapSeal) -> Self {
        Self {
            seal,
            store: OnceCell::new(),
        }
    }

    pub(super) fn open(&self, entry: &str, writable: bool, create: bool) -> io::Result<File> {
        let store = self
            .store
            .get_or_init(RestoreReceiptStore::open)
            .as_ref()
            .map_err(|error| io::Error::other(error.clone()))?;
        let name = map_digest(
            format!(
                "{}\n{}\n{}\n{entry}",
                self.seal.session,
                self.seal.map_digest,
                self.seal.file_manifest_digest.as_deref().unwrap_or("")
            )
            .as_bytes(),
        );
        store.receipt(&name, writable, create)
    }

    pub(super) fn can_resume(
        &self,
        entry: &str,
        dest: &Path,
        cancelled: impl Fn() -> bool,
    ) -> bool {
        let probe = || -> io::Result<bool> {
            let file = self.open(entry, false, false)?;
            let count = record_count(&file)?;
            let (_parents, root) = crate::fsutil::open_local_scan_leaf(dest, 0x80, 1)?;
            let metadata = crate::fsutil::pin_metadata(&root)?;
            if !metadata.is_dir() || crate::fsutil::is_reparse_metadata(&metadata) {
                return Ok(false);
            }
            let expected = record(&directory_key(Path::new(""))?, &root)?;
            let mut reader = BufReader::new(file);
            let mut actual = [0; RECORD_SIZE];
            for _ in 0..count {
                if cancelled() {
                    break;
                }
                reader.read_exact(&mut actual)?;
                if actual == expected {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        probe().unwrap_or(false)
    }
}

pub(super) fn directory_key(relative: &Path) -> io::Result<[u8; KEY_SIZE]> {
    let name = relative
        .to_str()
        .ok_or_else(|| io::Error::other("invalid restore directory name"))?;
    map_digest(name.as_bytes())
        .into_bytes()
        .try_into()
        .map_err(|_| io::Error::other("could not identify restore directory name"))
}

fn record(key: &[u8; KEY_SIZE], handle: &impl AsRawHandle) -> io::Result<[u8; RECORD_SIZE]> {
    let (volume, object) = crate::fsutil::object_identity(handle)?;
    let mut record = [0; RECORD_SIZE];
    record[..KEY_SIZE].copy_from_slice(key);
    record[KEY_SIZE..KEY_SIZE + 8].copy_from_slice(&volume.to_le_bytes());
    record[KEY_SIZE + 8..].copy_from_slice(&object);
    Ok(record)
}

fn record_count(file: &File) -> io::Result<u64> {
    let length = file.metadata()?.len();
    if length > MAX_BYTES {
        return Err(io::Error::other("restore receipt exceeds its size limit"));
    }
    Ok(length / RECORD_SIZE as u64)
}

pub(super) struct Journal {
    file: File,
    directories: BTreeSet<[u8; RECORD_SIZE]>,
    bytes: u64,
}

impl Journal {
    pub(super) fn open(mut file: File) -> io::Result<Self> {
        let count = record_count(&file)?;
        let mut reader = BufReader::new(&mut file);
        let mut directories = BTreeSet::new();
        for _ in 0..count {
            let mut record = [0; RECORD_SIZE];
            reader.read_exact(&mut record)?;
            directories.insert(record);
        }
        // A process can stop during an append. Only complete records can
        // authorize a retry, and the next write removes a torn suffix.
        let bytes = count * RECORD_SIZE as u64;
        if bytes != file.metadata()?.len() {
            file.set_len(bytes)?;
        }
        file.seek(SeekFrom::Start(bytes))?;
        Ok(Self {
            file,
            directories,
            bytes,
        })
    }

    pub(super) fn owns(&self, key: &[u8; KEY_SIZE], directory: &File) -> io::Result<bool> {
        let metadata = directory.metadata()?;
        Ok(metadata.is_dir()
            && !crate::fsutil::is_reparse_metadata(&metadata)
            && self.directories.contains(&record(key, directory)?))
    }

    pub(super) fn remember(&mut self, key: [u8; KEY_SIZE], directory: &File) -> io::Result<()> {
        if self.bytes + RECORD_SIZE as u64 > MAX_BYTES {
            return Err(io::Error::other("restore receipt exceeds its size limit"));
        }
        let record = record(&key, directory)?;
        // The final name is claimed only after its exact object's record is durable.
        self.file.write_all(&record)?;
        self.file.sync_all()?;
        self.bytes += RECORD_SIZE as u64;
        self.directories.insert(record);
        Ok(())
    }

    pub(super) fn finish(self) {
        super::publish::discard_temporary_file(&self.file);
    }
}
