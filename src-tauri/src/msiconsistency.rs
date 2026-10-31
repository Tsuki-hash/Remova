//! MSI database cross-check (M4E.3, read-only) — reconcile three views of the
//! same installed products:
//!
//! 1. the `Uninstall` registry records the app list shows;
//! 2. the packed `Classes\Installer\Products` keys Windows Installer owns;
//! 3. the Windows Installer product database (`MsiEnumProducts`), the
//!    supported enumeration API.
//!
//! A record that lost its product key is the classic "orphan uninstall entry";
//! a product key without a record hides a product from Programs & Features.
//! This module only diagnoses — deletion stays in the existing broken-install
//! flow with its fresh scan, exact ProductCode match and Safety Vault backup.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MsiDiagnosis {
    /// Braced, upper-cased ProductCode GUID.
    pub product_code: String,
    pub display_name: String,
    pub install_location: String,
    /// None = the location is empty/UNC/unknown (existence not judgeable).
    pub location_exists: Option<bool>,
    pub has_uninstall_record: bool,
    pub has_installer_key: bool,
    pub in_msi_db: bool,
    /// Stable class the UI localises: `consistent` | `missing_location` |
    /// `record_only` | `key_only` | `db_only`.
    pub class: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MsiConsistencyReport {
    pub products: Vec<MsiDiagnosis>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub completeness: crate::scan_contract::ScanCompleteness,
}

/// Pure reconciliation over the three views (lower-cased GUID keys).
pub fn reconcile(
    uninstall: &[(String, String, String, Option<bool>)],
    installer_keys: &HashSet<String>,
    msi_db: &HashSet<String>,
) -> Vec<MsiDiagnosis> {
    let mut codes: HashSet<String> = HashSet::new();
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut locations: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut existence: std::collections::HashMap<String, Option<bool>> =
        std::collections::HashMap::new();
    for (code, name, location, exists) in uninstall {
        codes.insert(code.to_lowercase());
        names
            .entry(code.to_lowercase())
            .or_insert_with(|| name.clone());
        locations
            .entry(code.to_lowercase())
            .or_insert_with(|| location.clone());
        existence
            .entry(code.to_lowercase())
            .or_insert_with(|| *exists);
    }
    for code in installer_keys.iter().chain(msi_db.iter()) {
        codes.insert(code.to_lowercase());
    }
    let mut out: Vec<MsiDiagnosis> = codes
        .into_iter()
        .map(|code| {
            let name = names.get(&code).cloned().unwrap_or_default();
            let location = locations.get(&code).cloned().unwrap_or_default();
            let has_uninstall_record = uninstall
                .iter()
                .any(|(c, _, _, _)| c.to_lowercase() == code);
            let has_installer_key = installer_keys.contains(&code);
            let in_msi_db = msi_db.contains(&code);
            // Existence evidence comes from the caller (probed with the
            // local-drive guard); reconcile itself never touches the disk, so
            // it stays a pure, machine-independent function.
            let location_exists = existence.get(&code).copied().flatten();
            let class = if has_uninstall_record && !has_installer_key {
                "record_only"
            } else if has_installer_key && !has_uninstall_record {
                "key_only"
            } else if !in_msi_db && has_uninstall_record {
                "db_missing"
            } else if location_exists == Some(false) {
                "missing_location"
            } else {
                "consistent"
            };
            MsiDiagnosis {
                product_code: code.to_uppercase(),
                display_name: name,
                install_location: location,
                location_exists,
                has_uninstall_record,
                has_installer_key,
                in_msi_db,
                class: class.into(),
            }
        })
        .collect();
    out.sort_by(|a, b| a.product_code.cmp(&b.product_code));
    out
}

/// The Windows Installer database as (braced upper ProductCode, name, location).
#[cfg(windows)]
fn msi_db_products() -> Result<Vec<(String, String, String)>, String> {
    use windows::Win32::System::ApplicationInstallationAndServicing::MsiEnumProductsW;
    const ERROR_NO_MORE_ITEMS: u32 = 259;
    let mut out = Vec::new();
    unsafe {
        for index in 0u32..100_000 {
            if crate::scan_task::cancelled() {
                return Err("scan:cancelled".into());
            }
            // The product-code buffer is 39 chars (braced GUID + NUL).
            let mut buf = [0u16; 39];
            let status = MsiEnumProductsW(index, windows::core::PWSTR(buf.as_mut_ptr()));
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != 0 {
                return Err(format!("MsiEnumProducts: {status}"));
            }
            let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            let code_raw = String::from_utf16_lossy(&buf[..end]);
            let code = normalize_product_code(&code_raw);
            let name = msi_property(&code, "ProductName").unwrap_or_default();
            let location = msi_property(&code, "InstallLocation").unwrap_or_default();
            out.push((code, name, location));
        }
    }
    Ok(out)
}

/// Normalize an MSI product-code string: braced, upper-cased, digits only.
fn normalize_product_code(raw: &str) -> String {
    let hex: String = raw
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_uppercase();
    if hex.len() == 32 {
        format!("{{{hex}}}")
    } else {
        raw.trim().to_uppercase()
    }
}

#[cfg(windows)]
fn msi_property(product_code: &str, property: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::ApplicationInstallationAndServicing::MsiGetProductInfoW;
    unsafe {
        let product = to_wide(product_code);
        let prop = to_wide(property);
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let status = MsiGetProductInfoW(
            PCWSTR(product.as_ptr()),
            PCWSTR(prop.as_ptr()),
            windows::core::PWSTR(buf.as_mut_ptr()),
            Some(&mut len),
        );
        if status != 0 || len as usize > buf.len() {
            return None;
        }
        let text = String::from_utf16_lossy(&buf[..len as usize]);
        Some(text.trim().to_string())
    }
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// GUID keys of the packed `Classes\Installer\Products` entries — the inverse
/// of the packing `apps::packed_product_code` applies, but re-packing the
/// unpacked form and comparing packed-to-packed is exact without inventing an
/// inverse (a wrong inverse would misattribute products).
#[cfg(windows)]
fn installer_key_codes() -> HashSet<String> {
    let mut out = HashSet::new();
    for alias in ["HKLM64", "HKLM32"] {
        let root = format!(r"{alias}\SOFTWARE\Classes\Installer\Products");
        for packed in crate::regscan::list_subkeys(&root) {
            if let Some(guid) = unpack_product_code(&packed) {
                out.insert(guid.to_uppercase());
            }
        }
    }
    out
}

/// Exact inverse of `apps::packed_product_code`: undo the nibble swap on the
/// last two groups, then reverse the first three.
pub fn unpack_product_code(packed: &str) -> Option<String> {
    let hex: String = packed
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_uppercase();
    if hex.len() != 32 {
        return None;
    }
    let mut out = String::with_capacity(32);
    for range in [0..8, 8..12, 12..16] {
        out.extend(hex[range].chars().rev());
    }
    for pair in hex.as_bytes()[16..].chunks(2) {
        out.push(pair[1] as char);
        out.push(pair[0] as char);
    }
    let guid = format!(
        "{}-{}-{}-{}-{}",
        &out[0..8],
        &out[8..12],
        &out[12..16],
        &out[16..20],
        &out[20..32]
    );
    Some(format!("{{{guid}}}"))
}

/// Full read-only reconciliation. `notes` records which views answered.
pub fn scan_msi_consistency() -> MsiConsistencyReport {
    let mut report = MsiConsistencyReport::default();
    if crate::scan_task::cancelled() {
        report.completeness = crate::scan_contract::ScanCompleteness::cancelled(0);
        return report;
    }
    // View 1: Uninstall records that carry a real msiexec /x{GUID} command.
    let uninstall: Vec<(String, String, String, Option<bool>)> =
        crate::apps::scan_installed_inventory()
            .iter()
            .filter_map(|app| {
                let cmd = if !app.uninstall_string.trim().is_empty() {
                    app.uninstall_string.as_str()
                } else {
                    app.quiet_uninstall_string.as_str()
                };
                super_msiexec_guid(cmd).map(|guid| {
                    let location = app.install_location.trim();
                    let exists = if location.is_empty() {
                        None
                    } else if crate::fsutil::is_local_drive_path(location) {
                        Some(std::path::Path::new(location).exists())
                    } else {
                        None
                    };
                    (
                        format!("{{{}}}", guid.to_uppercase()),
                        app.name.clone(),
                        app.install_location.clone(),
                        exists,
                    )
                })
            })
            .collect();
    report.completeness.roots_scanned += 1;
    // View 2: packed Installer\Products keys.
    let installer_keys = installer_key_codes();
    report.completeness.roots_scanned += 1;
    // View 3: the Windows Installer database. Unavailable (not elevated /
    // API failure) is a note, never "no products".
    let msi_db = match msi_db_products() {
        Ok(products) => {
            report.notes.push("msi_db_ok".into());
            report.completeness.roots_scanned += 1;
            products
        }
        Err(e) if e == "scan:cancelled" => {
            report.completeness = crate::scan_contract::ScanCompleteness::cancelled(
                report.completeness.roots_scanned,
            );
            return report;
        }
        Err(e) => {
            report.notes.push(format!("msi_db_unavailable:{e}"));
            report.completeness.status = crate::scan_contract::ScanStatus::Partial;
            report
                .completeness
                .failed_roots
                .push("MsiEnumProducts".into());
            Vec::new()
        }
    };
    let db_codes: HashSet<String> = msi_db
        .into_iter()
        .map(|(c, _, _)| c.to_lowercase())
        .collect();
    report.products = reconcile(
        &uninstall
            .iter()
            .map(|(c, n, l, e)| (c.to_lowercase(), n.clone(), l.clone(), *e))
            .collect::<Vec<_>>(),
        &installer_keys.iter().map(|c| c.to_lowercase()).collect(),
        &db_codes,
    );
    report
}

/// Same parse as `apps::msiexec_uninstall_guid` — re-imported here via the
/// public wrapper to avoid duplicating the tokenizer.
fn super_msiexec_guid(cmd: &str) -> Option<String> {
    crate::apps::msiexec_uninstall_guid_public(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUID: &str = "{593674B6-8B0D-4E23-8413-6B0E0DA19552}";

    #[test]
    fn packed_round_trip_is_exact() {
        let packed = crate::apps::packed_product_code(GUID).expect("packed");
        assert_eq!(unpack_product_code(&packed).as_deref(), Some(GUID));
        assert_eq!(unpack_product_code("nothex"), None);
    }

    #[test]
    fn reconcile_classes_the_three_views() {
        let uninstall = vec![
            (
                GUID.to_lowercase(),
                "Full".into(),
                r"C:\Program Files\Full".into(),
                Some(true),
            ),
            (
                "{AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE}".to_lowercase(),
                "Orphan".into(),
                r"C:\Gone".into(),
                Some(false),
            ),
            (
                "{99999999-8888-7777-6666-555555555555}".to_lowercase(),
                "HalfBroken".into(),
                r"C:\AlsoGone".into(),
                Some(false),
            ),
            (
                "{77777777-6666-5555-4444-333333333333}".to_lowercase(),
                "NoDir".into(),
                "".into(),
                None,
            ),
        ];
        let installer_keys: HashSet<String> = [
            GUID.to_lowercase(),
            "{11111111-2222-3333-4444-555555555555}".to_lowercase(),
            "{99999999-8888-7777-6666-555555555555}".to_lowercase(),
        ]
        .into_iter()
        .collect();
        let msi_db: HashSet<String> = [
            GUID.to_lowercase(),
            "{99999999-8888-7777-6666-555555555555}".to_lowercase(),
        ]
        .into_iter()
        .collect();
        let out = reconcile(&uninstall, &installer_keys, &msi_db);
        let by_code = |code: &str| out.iter().find(|d| d.product_code == code).unwrap();
        let full = by_code(GUID);
        assert_eq!(full.class, "consistent");
        assert_eq!(full.location_exists, Some(true));
        // Record without product key: the classic orphan record.
        let orphan = by_code("{AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE}");
        assert_eq!(orphan.class, "record_only");
        assert_eq!(orphan.location_exists, Some(false));
        // Key without record: hidden from Programs & Features.
        let key_only = by_code("{11111111-2222-3333-4444-555555555555}");
        assert_eq!(key_only.class, "key_only");
        // All three views present but the directory is gone.
        let broken = by_code("{99999999-8888-7777-6666-555555555555}");
        assert_eq!(broken.class, "missing_location");
        // No install location to judge: never guessed.
        assert_eq!(
            by_code("{77777777-6666-5555-4444-333333333333}").location_exists,
            None
        );
    }
}
