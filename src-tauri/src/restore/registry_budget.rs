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

    pub(super) fn read(&mut self, relative: &str, path: &Path) -> Result<Vec<u8>, String> {
        #[cfg(windows)]
        let file = crate::fsutil::open_local_regular_file(path);
        #[cfg(not(windows))]
        let file = File::open(path);
        self.read_file(relative, file.map_err(|err| err.to_string())?)
    }

    fn read_file(&mut self, relative: &str, file: File) -> Result<Vec<u8>, String> {
        let size = file.metadata().map_err(|err| err.to_string())?.len();
        self.add(relative, size)?;
        let mut bytes = Vec::new();
        file.take(size + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| err.to_string())?;
        if bytes.len() as u64 != size {
            return Err(format!("registry export changed while reading: {relative}"));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
