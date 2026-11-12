use std::fs::File;
use std::io::Read;
use std::path::Path;

const MAX_EXPORT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct RegistryRestoreBudget {
    bytes: u64,
}

impl RegistryRestoreBudget {
    pub(crate) fn add(&mut self, relative: &str, size: u64) -> Result<(), String> {
        if size > MAX_EXPORT_BYTES {
            return Err(format!(
                "registry export too large to restore: {relative} ({size} bytes)"
            ));
        }
        let total = self.bytes + size;
        if total > MAX_SESSION_BYTES {
            return Err(format!(
                "registry restore exceeds the total budget at {relative}; refusing the whole session"
            ));
        }
        self.bytes = total;
        Ok(())
    }

    pub(super) fn read(
        &mut self,
        relative: &str,
        path: &Path,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<u8>, String> {
        check_cancelled(&cancelled)?;
        #[cfg(windows)]
        let file = crate::fsutil::open_local_regular_file(path);
        #[cfg(not(windows))]
        let file = File::open(path);
        self.read_file(relative, file.map_err(|err| err.to_string())?, cancelled)
    }

    fn read_file(
        &mut self,
        relative: &str,
        file: File,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<u8>, String> {
        let size = file.metadata().map_err(|err| err.to_string())?.len();
        self.add(relative, size)?;
        let mut bytes = Vec::with_capacity(size as usize);
        let mut reader = file.take(size + 1);
        let mut chunk = [0; 64 * 1024];
        loop {
            check_cancelled(&cancelled)?;
            let count = reader.read(&mut chunk).map_err(|err| err.to_string())?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        if bytes.len() as u64 != size {
            return Err(format!("registry export changed while reading: {relative}"));
        }
        Ok(bytes)
    }
}

fn check_cancelled(cancelled: impl FnOnce() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("restore cancelled before any changes".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_preview_stops_mid_export_without_reading_the_rest() {
        let fixture = crate::test_support::TempDir::new("registry-preview-cancel");
        let file = fixture.path().join("large.reg");
        std::fs::File::create(&file)
            .unwrap()
            .set_len(1024 * 1024)
            .unwrap();
        let polls = std::cell::Cell::new(0);
        let mut budget = RegistryRestoreBudget::default();
        let result = budget.read("large.reg", &file, || {
            polls.set(polls.get() + 1);
            polls.get() >= 4
        });
        assert_eq!(result.unwrap_err(), "restore cancelled before any changes");
        assert_eq!(polls.get(), 4);
    }

    #[test]
    fn registry_budget_counts_only_accepted_exports_at_exact_boundaries() {
        let mut budget = RegistryRestoreBudget::default();
        assert!(budget.add("too large", MAX_EXPORT_BYTES + 1).is_err());
        for _ in 0..(MAX_SESSION_BYTES / MAX_EXPORT_BYTES) {
            budget.add("at the single limit", MAX_EXPORT_BYTES).unwrap();
        }
        assert!(budget.add("one byte over the total", 1).is_err());
    }
}
