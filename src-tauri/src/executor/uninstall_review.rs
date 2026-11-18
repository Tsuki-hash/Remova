//! A successful vendor uninstall may authorize a later residual scan in this process.

use super::uninstall::{check_registration, RegistrationState};
use crate::apps::{app_key, InstalledApp, ScannedApp};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
struct Review {
    id: u64,
    scanned: ScannedApp,
    argv: Vec<String>,
}

#[derive(Default)]
struct Reviews {
    next_id: u64,
    by_app: HashMap<String, Review>,
}

fn reviews() -> &'static Mutex<Reviews> {
    static REVIEWS: OnceLock<Mutex<Reviews>> = OnceLock::new();
    REVIEWS.get_or_init(Mutex::default)
}

#[cfg(test)]
thread_local! {
    static FAIL_NEXT_VERIFY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(super) fn fail_next_verification() {
    FAIL_NEXT_VERIFY.set(true);
}

pub(super) fn retire(app: &InstalledApp) {
    if let Ok(mut reviews) = reviews().lock() {
        reviews.by_app.remove(&app_key(app));
    }
}

pub(super) fn issue(scanned: ScannedApp, argv: Vec<String>) -> Option<u64> {
    let mut reviews = reviews().lock().ok()?;
    let id = reviews.next_id.checked_add(1)?;
    reviews.next_id = id;
    reviews
        .by_app
        .insert(app_key(&scanned.app), Review { id, scanned, argv });
    if reviews.by_app.len() > 64 {
        let oldest = reviews
            .by_app
            .iter()
            .min_by_key(|(_, review)| review.id)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            reviews.by_app.remove(&oldest);
        }
    }
    Some(id)
}

pub(crate) fn analysis_key(app: &InstalledApp, receipt: Option<u64>) -> String {
    match receipt {
        Some(id) => format!("review:{id}"),
        None => app_key(app),
    }
}

pub(super) fn protects_installation(app: &InstalledApp, location: &str) -> bool {
    let Some(old) = crate::fsutil::path_key(Path::new(&app.install_location)) else {
        return false;
    };
    let Some(live) = crate::fsutil::path_key(Path::new(location)) else {
        return false;
    };
    old == live
        || live
            .strip_prefix(&old)
            .is_some_and(|tail| tail.starts_with('\\'))
        || old
            .strip_prefix(&live)
            .is_some_and(|tail| tail.starts_with('\\'))
}

fn find(app: &InstalledApp, id: u64) -> Result<Review, String> {
    let guard = reviews().lock().map_err(|_| "uninstall review expired")?;
    guard
        .by_app
        .get(&app_key(app))
        .filter(|review| {
            review.id == id
                && review.scanned.app.version == app.version
                && review.scanned.app.install_location == app.install_location
        })
        .cloned()
        .ok_or_else(|| "uninstall review expired".into())
}

pub(crate) fn resolve(app: &InstalledApp, id: u64) -> Result<InstalledApp, String> {
    Ok(find(app, id)?.scanned.app)
}

fn found_installed(app: &InstalledApp, id: u64) -> String {
    if let Ok(mut guard) = reviews().lock() {
        let key = app_key(app);
        if guard.by_app.get(&key).is_some_and(|review| review.id == id) {
            guard.by_app.remove(&key);
        }
    }
    "uninstall review found an installed application".into()
}

pub(crate) fn verify(app: &InstalledApp, id: u64) -> Result<InstalledApp, String> {
    let review = find(app, id)?;
    #[cfg(test)]
    if FAIL_NEXT_VERIFY.replace(false) {
        return Err("uninstall review could not verify installation".into());
    }
    match check_registration(&review.scanned, &review.argv) {
        RegistrationState::Present => return Err(found_installed(app, id)),
        RegistrationState::Unverified => {
            return Err("uninstall review could not verify installation".into())
        }
        RegistrationState::Removed => {}
    }
    let original = review.scanned.app;
    if original.source != "Store" {
        // A reinstall can register a new key or product code at the same root.
        let inventory = crate::apps::scan_install_owners();
        if inventory
            .install_roots
            .iter()
            .any(|root| protects_installation(&original, root))
            || inventory
                .apps
                .iter()
                .any(|other| crate::apps::same_product(&original, other))
        {
            return Err(found_installed(app, id));
        }
        if inventory
            .completeness
            .failed_roots
            .iter()
            .any(|source| source != crate::apps::STORE_INVENTORY_ROOT)
            || inventory.completeness.status == crate::scan_contract::ScanStatus::Cancelled
        {
            return Err("uninstall review could not verify installation".into());
        }
    }
    Ok(original)
}
