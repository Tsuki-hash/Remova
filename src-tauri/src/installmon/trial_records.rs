//! Completed named recordings, separate from the monitor deletion scope.

use super::MonitorDiff;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Trial {
    pub id: String,
    name: String,
    started_at: u64,
    file_roots: Vec<String>,
    registry_roots: Vec<String>,
}

impl Trial {
    pub(super) fn new(label: Option<String>) -> Result<Option<Self>, String> {
        let Some(name) = label
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            return Ok(None);
        };
        if name.chars().count() > 120 || name.chars().any(char::is_control) {
            return Err("trial:invalid_name".into());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "trial:unavailable")?;
        Ok(Some(Self {
            id: format!("{}-{}", std::process::id(), now.as_nanos()),
            name: name.to_owned(),
            started_at: now.as_secs(),
            file_roots: super::roots()
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            registry_roots: super::registry_roots(),
        }))
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct TrialRecord {
    trial: Trial,
    finished_at: u64,
    diff: MonitorDiff,
}

#[cfg(windows)]
mod store {
    use super::*;
    use std::io::{ErrorKind, Read};
    use std::path::PathBuf;
    use std::sync::Mutex;

    const MAX_RECORDS: usize = 12;
    const MAX_BYTES: usize = 5 * 1024 * 1024;
    const ERR: &str = "trial:unavailable";
    static LOCK: Mutex<()> = Mutex::new(());

    struct Store {
        path: PathBuf,
        context: String,
    }

    impl Store {
        fn current() -> Result<Self, String> {
            use windows::Win32::UI::Shell::FOLDERID_LocalAppData;
            let root = crate::sysops::lookup_known_folder(&FOLDERID_LocalAppData).ok_or(ERR)?;
            let sid = crate::sysops::current_user_sid().ok_or(ERR)?;
            let elevated = crate::sysops::is_process_elevated().map_err(|_| ERR)?;
            let name = if elevated {
                "install-trials-admin.v1"
            } else {
                "install-trials.v1"
            };
            Ok(Self {
                path: root.join("Remova").join(name),
                context: format!("Remova install trials v1:{sid}:{elevated}"),
            })
        }

        fn read(&self) -> Result<Vec<TrialRecord>, String> {
            let file = match crate::fsutil::open_local_regular_file(&self.path) {
                Ok(file) => file,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
                Err(_) => return Err(ERR.into()),
            };
            let mut sealed = Vec::new();
            file.take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut sealed)
                .map_err(|_| ERR)?;
            if sealed.len() > MAX_BYTES {
                return Err(ERR.into());
            }
            let plain = super::super::checkpoint::protect(&sealed, &self.context, false)
                .map_err(|_| ERR)?;
            let rows: Vec<TrialRecord> = serde_json::from_slice(&plain).map_err(|_| ERR)?;
            if rows.len() > MAX_RECORDS {
                return Err(ERR.into());
            }
            Ok(rows)
        }

        fn update(&self, change: impl FnOnce(&mut Vec<TrialRecord>)) -> Result<(), String> {
            let _locked = LOCK.lock().map_err(|_| ERR)?;
            let parent = self.path.parent().ok_or(ERR)?;
            let _pins = crate::fsutil::create_dirs_pinned(parent).map_err(|_| ERR)?;
            let mut rows = self.read()?;
            change(&mut rows);
            let plain = serde_json::to_vec(&rows).map_err(|_| ERR)?;
            let sealed =
                super::super::checkpoint::protect(&plain, &self.context, true).map_err(|_| ERR)?;
            if sealed.len() > MAX_BYTES {
                return Err(ERR.into());
            }
            crate::fsutil::write_bytes_atomic(&self.path, &sealed).map_err(|_| ERR.into())
        }
    }

    fn bounded_entries(entries: &mut Vec<String>, limit: usize, omitted: &mut usize) {
        let initial = entries.len();
        entries.retain(|line| line.len() <= 2048);
        entries.truncate(limit);
        *omitted += initial - entries.len();
    }

    pub(crate) fn list() -> Result<Vec<TrialRecord>, String> {
        let _locked = LOCK.lock().map_err(|_| ERR)?;
        Store::current()?.read()
    }

    fn save_record(store: &Store, trial: &Trial, source_diff: &MonitorDiff) -> Result<(), String> {
        let mut diff = source_diff.clone();
        bounded_entries(&mut diff.added_files, 64, &mut diff.files_truncated);
        bounded_entries(&mut diff.added_reg_values, 32, &mut diff.reg_truncated);
        let finished_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ERR)?
            .as_secs();
        store.update(|rows| {
            if let Some(row) = rows.iter_mut().find(|row| row.trial.id == trial.id) {
                row.diff = diff;
                row.finished_at = finished_at;
            } else {
                rows.insert(
                    0,
                    TrialRecord {
                        trial: trial.clone(),
                        finished_at,
                        diff,
                    },
                );
                rows.truncate(MAX_RECORDS);
            }
        })
    }

    pub(crate) fn save(trial: &Trial, source_diff: &MonitorDiff) -> Result<(), String> {
        save_record(&Store::current()?, trial, source_diff)
    }

    #[cfg(test)]
    #[test]
    fn completed_history_contains_the_read_only_diff_and_survives_reopen() {
        let lab = crate::test_support::TempDir::new("trial_history");
        let store = Store {
            path: lab.path().join("record.v1"),
            context: "trial history fixture".into(),
        };
        assert!(store.read().unwrap().is_empty());
        let trial = Trial::new(Some("Editor test".into())).unwrap().unwrap();
        let initial = MonitorDiff {
            added_files: vec![r"D:\Editor\cache".into()],
            recovered_read_only: true,
            trial_read_only: true,
            ..Default::default()
        };
        save_record(&store, &trial, &initial).unwrap();
        let mut diff = initial;
        diff.added_files
            .extend((0..70).map(|n| format!(r"D:\Editor\cache\{n}")));
        save_record(&store, &trial, &diff).unwrap();
        let rows = store.read().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trial.name, "Editor test");
        assert!(rows[0].diff.recovered_read_only);
        assert!(rows[0].diff.trial_read_only);
        assert_eq!(rows[0].diff.added_files.len(), 64);
        assert_eq!(rows[0].diff.files_truncated, 7);
        assert!(!std::fs::read(&store.path)
            .unwrap()
            .windows(11)
            .any(|part| part == b"Editor test"));
    }
}

#[cfg(windows)]
pub(crate) use store::{list, save};

#[cfg(not(windows))]
pub(crate) fn list() -> Result<Vec<TrialRecord>, String> {
    Ok(Vec::new())
}
