//! Per-user restart reminders. Only live uninstall receipts can arm a cleanup.

use super::reboot::{self, GateStatus};
use super::uninstall::{check_registration, RegistrationState};
use super::uninstall_review::protects_installation;
use crate::apps::{scan_install_owners, InstalledApp, ScannedApp};
use crate::fsutil;
use serde::{Deserialize, Serialize};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_RECORDS: usize = 32;
const MAX_BYTES: usize = 512 * 1024;
const UNAVAILABLE: &str = "pending_review:unavailable";
static FILE: Mutex<()> = Mutex::new(());

#[derive(Deserialize, Serialize)]
struct Reminder {
    id: u32,
    boot: Option<String>,
    name: String,
    version: String,
    publisher: String,
    source: String,
    registry_key: String,
    install_location: String,
    aliases: Vec<String>,
}

impl Reminder {
    fn scanned(&self) -> ScannedApp {
        ScannedApp {
            app: InstalledApp {
                name: self.name.clone(),
                version: self.version.clone(),
                publisher: self.publisher.clone(),
                source: self.source.clone(),
                registry_key: self.registry_key.clone(),
                install_location: self.install_location.clone(),
                uninstall_string: String::new(),
                quiet_uninstall_string: String::new(),
                estimated_size_kb: 0,
                install_date: String::new(),
                display_icon: String::new(),
            },
            registration_aliases: self.aliases.clone(),
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
struct Reminders {
    sequence: u32,
    rows: Vec<Reminder>,
}

#[derive(Serialize)]
pub(crate) struct PendingReview {
    id: u32,
    name: String,
    version: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewStatus {
    AwaitingRestart,
    Installed,
    Removed,
    Unverified,
}

fn profile_file() -> Result<PathBuf, String> {
    #[cfg(windows)]
    if let Some(root) =
        crate::sysops::lookup_known_folder(&windows::Win32::UI::Shell::FOLDERID_LocalAppData)
    {
        return Ok(root
            .join("Remova")
            .join("pending-uninstall-reviews.v1.json"));
    }
    Err(UNAVAILABLE.into())
}

fn load(path: &Path) -> Result<Reminders, String> {
    #[cfg(windows)]
    let opened = fsutil::open_local_regular_file(path);
    #[cfg(not(windows))]
    let opened = std::fs::File::open(path);
    let file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Reminders::default()),
        Err(_) => return Err(UNAVAILABLE.into()),
    };
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| UNAVAILABLE)?;
    if bytes.len() > MAX_BYTES {
        return Err(UNAVAILABLE.into());
    }
    let saved: Reminders = serde_json::from_slice(&bytes).map_err(|_| UNAVAILABLE)?;
    if saved.rows.len() > MAX_RECORDS {
        return Err(UNAVAILABLE.into());
    }
    Ok(saved)
}

fn transact(
    path: &Path,
    change: impl FnOnce(&mut Reminders) -> Result<(), String>,
) -> Result<(), String> {
    let _lock = FILE.lock().map_err(|_| UNAVAILABLE)?;
    let parent = path.parent().ok_or(UNAVAILABLE)?;
    let _pins = fsutil::create_dirs_pinned(parent).map_err(|_| UNAVAILABLE)?;
    let mut state = load(path)?;
    change(&mut state)?;
    let json = serde_json::to_vec(&state).map_err(|_| UNAVAILABLE)?;
    if json.len() > MAX_BYTES {
        return Err(UNAVAILABLE.into());
    }
    fsutil::write_bytes_atomic(path, &json).map_err(|_| UNAVAILABLE.into())
}

fn save(path: &Path, scanned: &ScannedApp, boot: Option<String>) -> Result<(), String> {
    let app = &scanned.app;
    transact(path, |state| {
        state.sequence = state.sequence.checked_add(1).ok_or(UNAVAILABLE)?;
        state.rows.retain(|row| {
            (
                row.source.as_str(),
                row.registry_key.as_str(),
                row.name.as_str(),
            ) != (
                app.source.as_str(),
                app.registry_key.as_str(),
                app.name.as_str(),
            )
        });
        state.rows.insert(
            0,
            Reminder {
                id: state.sequence,
                boot,
                name: app.name.clone(),
                version: app.version.clone(),
                publisher: app.publisher.clone(),
                source: app.source.clone(),
                registry_key: app.registry_key.clone(),
                install_location: app.install_location.clone(),
                aliases: scanned.registration_aliases.clone(),
            },
        );
        state.rows.truncate(MAX_RECORDS);
        Ok(())
    })
}

#[cfg(not(test))]
pub(super) fn remember(scanned: &ScannedApp) -> Result<(), String> {
    save(&profile_file()?, scanned, reboot::system_boot_id())
}

pub(crate) fn list() -> Result<Vec<PendingReview>, String> {
    let _lock = FILE.lock().map_err(|_| UNAVAILABLE)?;
    Ok(load(&profile_file()?)?
        .rows
        .into_iter()
        .map(|row| PendingReview {
            id: row.id,
            name: row.name,
            version: row.version,
        })
        .collect())
}

pub(crate) fn dismiss(id: u32) -> Result<(), String> {
    transact(&profile_file()?, |state| {
        state.rows.retain(|row| row.id != id);
        Ok(())
    })
}

fn inspect(
    saved_boot: Option<&str>,
    current_boot: Option<&str>,
    gate: GateStatus,
    registration: impl FnOnce() -> RegistrationState,
) -> ReviewStatus {
    match (saved_boot, current_boot, gate) {
        (_, _, GateStatus::Corrupt) | (None, _, _) | (_, None, _) => ReviewStatus::Unverified,
        (saved, current, gate) if saved == current || gate == GateStatus::Pending => {
            ReviewStatus::AwaitingRestart
        }
        _ => match registration() {
            RegistrationState::Present => ReviewStatus::Installed,
            RegistrationState::Removed => ReviewStatus::Removed,
            RegistrationState::Unverified => ReviewStatus::Unverified,
        },
    }
}

fn review_registration(scanned: &ScannedApp) -> RegistrationState {
    // The review store grants no authority. Only read canonical uninstall
    // records or package families, even if a local reminder was edited.
    fn allowed_key(key: &str) -> bool {
        let Some((view, subkey)) = key.split_once('\\') else {
            return false;
        };
        match view {
            "Store" => !subkey.is_empty() && !subkey.contains('\\'),
            "HKCU" | "HKLM64" | "HKLM32" => subkey
                .strip_prefix("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\")
                .is_some_and(|leaf| !leaf.is_empty() && !leaf.contains('\\')),
            _ => false,
        }
    }
    let app = &scanned.app;
    if !std::iter::once(&app.registry_key)
        .chain(&scanned.registration_aliases)
        .all(|key| allowed_key(key))
    {
        return RegistrationState::Unverified;
    }
    let result = check_registration(scanned, &[]);
    if result != RegistrationState::Removed {
        return result;
    }
    let current = scan_install_owners();
    if current
        .apps
        .iter()
        .any(|other| crate::apps::same_product(app, other))
        || current
            .install_roots
            .iter()
            .any(|root| protects_installation(app, root))
    {
        return RegistrationState::Present;
    }
    if current.completeness.status != crate::scan_contract::ScanStatus::Complete {
        return RegistrationState::Unverified;
    }
    RegistrationState::Removed
}

pub(crate) fn check(id: u32) -> Result<ReviewStatus, String> {
    let saved = {
        let _lock = FILE.lock().map_err(|_| UNAVAILABLE)?;
        load(&profile_file()?)?
            .rows
            .into_iter()
            .find(|row| row.id == id)
            .ok_or(UNAVAILABLE)?
    };
    Ok(inspect(
        saved.boot.as_deref(),
        reboot::system_boot_id().as_deref(),
        reboot::gate_status(),
        || review_registration(&saved.scanned()),
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn after_restart_reviews_live_registration_and_never_treats_unknown_as_absent() {
        assert_eq!(
            inspect(Some("old"), Some("old"), GateStatus::Clear, || panic!(
                "before reboot"
            )),
            ReviewStatus::AwaitingRestart
        );
        assert_eq!(
            inspect(Some("old"), Some("new"), GateStatus::Pending, || panic!(
                "gate pending"
            )),
            ReviewStatus::AwaitingRestart
        );
        assert_eq!(
            inspect(Some("old"), None, GateStatus::Clear, || panic!(
                "unknown boot"
            )),
            ReviewStatus::Unverified
        );
        assert_eq!(
            inspect(Some("old"), Some("new"), GateStatus::Corrupt, || panic!(
                "corrupt gate"
            )),
            ReviewStatus::Unverified
        );
        for (registration, expected) in [
            (RegistrationState::Present, ReviewStatus::Installed),
            (RegistrationState::Removed, ReviewStatus::Removed),
            (RegistrationState::Unverified, ReviewStatus::Unverified),
        ] {
            assert_eq!(
                inspect(Some("old"), Some("new"), GateStatus::Clear, || registration),
                expected
            );
        }
    }

    #[test]
    fn local_reminder_survives_reads_without_carrying_uninstall_commands() {
        let lab = crate::test_support::TempDir::new("pending_review");
        let path = lab.join("review.json");
        assert!(load(&path).unwrap().rows.is_empty());
        let scanned = Reminder {
            id: 0,
            boot: None,
            name: "Demo".into(),
            version: "2".into(),
            publisher: "Vendor".into(),
            source: "HKCU".into(),
            registry_key: r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Demo".into(),
            install_location: r"C:\Apps\Demo".into(),
            aliases: vec![],
        }
        .scanned();
        save(&path, &scanned, Some("old".into())).unwrap();
        assert_eq!(load(&path).unwrap().rows[0].boot.as_deref(), Some("old"));
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("uninstall_string"));
        save(&path, &scanned, Some("new".into())).unwrap();
        assert_eq!(load(&path).unwrap().rows[0].id, 2);
        assert_eq!(load(&path).unwrap().rows.len(), 1);
    }
}
