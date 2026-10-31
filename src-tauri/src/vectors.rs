//! Installation vectors (M4E.2, read-only) — one display name can exist as a
//! per-machine MSI, a per-user Inno install, an MSIX package, a global CLI
//! package or a standalone service. The inventory keeps every record separate
//! already; this grouping makes the vectors visible so "uninstall X" can be
//! scoped to the vector the user actually means.
//!
//! Grouping is display-only: it never merges authorization identities, and
//! per-user records are never folded into per-machine ones (the existing
//! `dedup_same_products` rule still holds).

use crate::apps::InstalledApp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallationVector {
    /// Stable identity: scan source + registry key / package full name. The UI
    /// echoes it back only for display; deletion still goes through the
    /// per-record commands with their own fresh-scan checks.
    pub vector_id: String,
    /// Scan source: HKLM64 / HKLM32 / HKCU / Store.
    pub source: String,
    pub name: String,
    pub version: String,
    pub install_location: String,
    pub registry_key: String,
    #[serde(default)]
    pub estimated_size_kb: i64,
    /// True when this record installs per-user (HKCU / Store for the current
    /// user): no elevation needed for its own uninstaller.
    pub per_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductFamily {
    /// Lowercased product name — the display grouping key.
    pub family: String,
    pub vectors: Vec<InstallationVector>,
}

pub fn vector_id_of(app: &InstalledApp) -> String {
    format!("{}|{}", app.source, app.registry_key)
}

fn vector_of(app: &InstalledApp) -> InstallationVector {
    InstallationVector {
        vector_id: vector_id_of(app),
        source: app.source.clone(),
        name: app.name.clone(),
        version: app.version.clone(),
        install_location: app.install_location.clone(),
        registry_key: app.registry_key.clone(),
        estimated_size_kb: app.estimated_size_kb,
        per_user: app.source.eq_ignore_ascii_case("HKCU")
            || app.source.eq_ignore_ascii_case("Store"),
    }
}

/// Group the inventory into product families by lowercased name. Pure, so the
/// grouping (and its per-user flag) is testable without a real inventory.
pub fn group_families(apps: &[InstalledApp]) -> Vec<ProductFamily> {
    let mut families: Vec<ProductFamily> = Vec::new();
    for app in apps {
        let family_key = app.name.trim().to_lowercase();
        if family_key.is_empty() {
            continue;
        }
        match families.iter_mut().find(|f| f.family == family_key) {
            Some(family) => family.vectors.push(vector_of(app)),
            None => families.push(ProductFamily {
                family: family_key,
                vectors: vec![vector_of(app)],
            }),
        }
    }
    families.sort_by(|a, b| a.family.cmp(&b.family));
    for family in &mut families {
        family.vectors.sort_by(|a, b| a.vector_id.cmp(&b.vector_id));
    }
    families
}

pub fn scan_install_vectors() -> Vec<ProductFamily> {
    group_families(&crate::apps::scan_installed_inventory())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, source: &str, key: &str, version: &str, loc: &str) -> InstalledApp {
        InstalledApp {
            name: name.into(),
            version: version.into(),
            publisher: String::new(),
            install_location: loc.into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: source.into(),
            registry_key: key.into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        }
    }

    #[test]
    fn same_name_groups_into_one_family_with_distinct_vectors() {
        let families = group_families(&[
            app(
                "Demo",
                "HKLM64",
                r"HKLM64\...\Uninstall\A",
                "1.0",
                r"C:\Program Files\Demo",
            ),
            app(
                "demo",
                "HKCU",
                r"HKCU\...\Uninstall\B",
                "1.3",
                r"C:\Users\u\AppData\Local\Demo",
            ),
            app(
                "Other",
                "Store",
                r"Store\pkg",
                "2.0",
                r"C:\Program Files\WindowsApps\pkg",
            ),
        ]);
        assert_eq!(families.len(), 2);
        assert_eq!(families[0].family, "demo");
        assert_eq!(families[0].vectors.len(), 2);
        // Per-user flag drives the "no elevation needed" display only.
        // The vector sort is by id, so assert membership rather than order.
        assert!(families[0].vectors.iter().any(|v| v.per_user));
        assert!(families[0].vectors.iter().any(|v| !v.per_user));
        // Vector ids never collide across sources even with equal keys.
        assert_ne!(
            families[0].vectors[0].vector_id,
            families[0].vectors[1].vector_id
        );
    }

    #[test]
    fn blank_names_are_skipped_and_families_sort() {
        let families = group_families(&[
            app("Zeta", "Store", r"Store\z", "1", ""),
            app("", "HKLM64", r"HKLM64\...\X", "1", ""),
        ]);
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].family, "zeta");
    }
}
