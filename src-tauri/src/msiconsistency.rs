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
    /// Stable class: `consistent` | `missing_location` | `record_only` |
    /// `key_only` | `db_only` | `db_missing` | `inconclusive`.
    /// Presence flags are positive evidence only; false does not prove absence
    /// when report completeness is Partial.
    pub class: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MsiConsistencyReport {
    pub products: Vec<MsiDiagnosis>,
    /// Per-source status: `uninstall_ok`, `installer_keys_ok`, `msi_db_ok`,
    /// or `<source>_unavailable:<reason>`. An ok note certifies only that source;
    /// overall completeness remains authoritative for the entire report.
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub completeness: crate::scan_contract::ScanCompleteness,
}

type UninstallRecord = (String, String, String, Option<bool>);
type DbProduct = (String, String, String);

/// Pure reconciliation over complete views. Invalid identities produce no
/// diagnosis; production records the validation error as an incomplete view.
pub fn reconcile(
    uninstall: &[UninstallRecord],
    installer_keys: &HashSet<String>,
    msi_db: &HashSet<String>,
) -> Vec<MsiDiagnosis> {
    let uninstall = validate_uninstall(Collected::complete(uninstall.to_vec()));
    let installer = validate_codes(Collected::complete(
        installer_keys.iter().cloned().collect(),
    ));
    let db = validate_codes(Collected::complete(msi_db.iter().cloned().collect()));
    reconcile_views(&uninstall, &installer, &db)
}

/// A view may contain positive observations without proving absence of any
/// other product. Completeness belongs to this invocation, never a global flag.
struct Collected<T> {
    rows: Vec<T>,
    failures: Vec<String>,
}

impl<T> Collected<T> {
    fn complete(rows: Vec<T>) -> Self {
        Self {
            rows,
            failures: Vec::new(),
        }
    }

    fn from_result(result: Result<Vec<T>, String>) -> Self {
        match result {
            Ok(rows) => Self::complete(rows),
            Err(e) => Self {
                rows: Vec::new(),
                failures: vec![e],
            },
        }
    }

    fn known(&self) -> bool {
        self.failures.is_empty()
    }
}

fn validate_uninstall(mut view: Collected<UninstallRecord>) -> Collected<UninstallRecord> {
    view.rows.retain_mut(|(code, _, _, _)| {
        if let Some(normalized) = normalize_product_code(code) {
            *code = normalized;
            true
        } else {
            view.failures.push("invalid_product_code".into());
            false
        }
    });
    view
}

fn validate_codes(mut view: Collected<String>) -> Collected<String> {
    view.rows = view
        .rows
        .into_iter()
        .filter_map(|code| {
            let normalized = normalize_product_code(&code);
            if normalized.is_none() {
                view.failures.push("invalid_product_code".into());
            }
            normalized
        })
        .collect();
    view
}

fn reconcile_views(
    uninstall: &Collected<UninstallRecord>,
    installer: &Collected<String>,
    db: &Collected<String>,
) -> Vec<MsiDiagnosis> {
    let installer_keys: HashSet<_> = installer.rows.iter().cloned().collect();
    let msi_db: HashSet<_> = db.rows.iter().cloned().collect();
    let mut codes = HashSet::new();
    let mut records = std::collections::HashMap::new();
    for record in &uninstall.rows {
        codes.insert(record.0.clone());
        records.entry(record.0.clone()).or_insert(record);
    }
    codes.extend(installer_keys.iter().chain(msi_db.iter()).cloned());
    let mut out: Vec<MsiDiagnosis> = codes
        .into_iter()
        .map(|code| {
            let record = records.get(&code);
            let name = record.map(|r| r.1.clone()).unwrap_or_default();
            let location = record.map(|r| r.2.clone()).unwrap_or_default();
            let has_uninstall_record = record.is_some();
            let has_installer_key = installer_keys.contains(&code);
            let in_msi_db = msi_db.contains(&code);
            // Reconciliation never touches disk. A missing location is an
            // independent positive observation, not absence from another view.
            let location_exists = record.and_then(|r| r.3);
            let unknown_membership = (!has_uninstall_record && !uninstall.known())
                || (!has_installer_key && !installer.known())
                || (!in_msi_db && !db.known());
            let class = if unknown_membership {
                "inconclusive"
            } else if has_uninstall_record && !has_installer_key {
                "record_only"
            } else if has_installer_key && !has_uninstall_record {
                "key_only"
            } else if location_exists == Some(false) {
                "missing_location"
            } else if in_msi_db && !has_uninstall_record && !has_installer_key {
                "db_only"
            } else if !in_msi_db && has_uninstall_record {
                "db_missing"
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
                return Ok(out);
            }
            if status != 0 {
                return Err(format!("MsiEnumProducts: {status}"));
            }
            let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            let code_raw = String::from_utf16_lossy(&buf[..end]);
            let code = normalize_product_code(&code_raw).ok_or("invalid_product_code")?;
            let name = msi_property(&code, "ProductName").unwrap_or_default();
            let location = msi_property(&code, "InstallLocation").unwrap_or_default();
            out.push((code, name, location));
        }
    }
    Err("MsiEnumProducts: entry budget exceeded".into())
}

/// Only canonical 8-4-4-4-12 GUIDs, optionally enclosed in one brace pair.
/// Never strip arbitrary characters or accept compact/packed GUIDs here.
fn normalize_product_code(raw: &str) -> Option<String> {
    let text = raw.trim();
    let guid = if text.starts_with('{') && text.ends_with('}') {
        &text[1..text.len() - 1]
    } else {
        text
    };
    if guid.len() != 36
        || !guid.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return None;
    }
    Some(format!("{{{}}}", guid.to_ascii_uppercase()))
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

/// Keep the native (subkeys, value names) tuple intact through the injectable
/// boundary: value names are not packed ProductCodes.
fn installer_key_codes(
    mut snapshot: impl FnMut(&str) -> Result<(Vec<String>, Vec<String>), String>,
) -> Collected<String> {
    let mut view = Collected::complete(Vec::new());
    for alias in ["HKLM64", "HKLM32"] {
        let root = format!(r"{alias}\SOFTWARE\Classes\Installer\Products");
        match snapshot(&root) {
            Ok((keys, _values)) => {
                for packed in keys {
                    match unpack_product_code(&packed) {
                        Some(guid) => view.rows.push(guid),
                        None => view.failures.push(format!("{root}:invalid_packed_code")),
                    }
                }
            }
            Err(e) => view.failures.push(format!("{root}:{e}")),
        }
    }
    view
}

/// Exact inverse of `apps::packed_product_code`: undo the nibble swap on the
/// last two groups, then reverse the first three.
pub fn unpack_product_code(packed: &str) -> Option<String> {
    if packed.len() != 32 || !packed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let hex = packed.to_ascii_uppercase();
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
    normalize_product_code(&guid)
}

/// Adapt inventory rows at the same boundary used by production and fixtures.
/// The caller supplies per-invocation completeness, not a cached/global flag.
fn uninstall_records(
    apps: Collected<crate::apps::InstalledApp>,
    mut exists: impl FnMut(&str) -> Option<bool>,
) -> Collected<UninstallRecord> {
    let mut failures = apps.failures;
    let rows = apps
        .rows
        .into_iter()
        .filter_map(|app| {
            let cmd = if !app.uninstall_string.trim().is_empty() {
                &app.uninstall_string
            } else {
                &app.quiet_uninstall_string
            };
            let code = super_msiexec_guid(cmd);
            if code.is_none() && msiexec_product_argument(cmd).is_some() {
                failures.push("invalid_uninstall_product_code".into());
            }
            code.map(|code| {
                let location_exists = exists(app.install_location.trim());
                (code, app.name, app.install_location, location_exists)
            })
        })
        .collect();
    validate_uninstall(Collected { rows, failures })
}

/// Every test injection enters at the real collection boundary, before tuple
/// decoding, GUID normalization, completeness propagation and reconciliation.
fn collect_report(
    inventory: impl FnOnce() -> Collected<crate::apps::InstalledApp>,
    snapshot: impl FnMut(&str) -> Result<(Vec<String>, Vec<String>), String>,
    database: impl FnOnce() -> Result<Vec<DbProduct>, String>,
    exists: impl FnMut(&str) -> Option<bool>,
) -> MsiConsistencyReport {
    let mut report = MsiConsistencyReport::default();
    let uninstall = uninstall_records(inventory(), exists);
    let installer = installer_key_codes(snapshot);
    let raw_db = Collected::from_result(database());
    let db = validate_codes(Collected {
        rows: raw_db.rows.into_iter().map(|(c, _, _)| c).collect(),
        failures: raw_db.failures,
    });
    for (label, failures) in [
        ("uninstall", &uninstall.failures),
        ("installer_keys", &installer.failures),
        ("msi_db", &db.failures),
    ] {
        if failures.is_empty() {
            report.completeness.roots_scanned += 1;
            report.notes.push(format!("{label}_ok"));
        } else {
            report.completeness.status = crate::scan_contract::ScanStatus::Partial;
            report.completeness.failed_roots.push(label.into());
            report
                .notes
                .extend(failures.iter().map(|e| format!("{label}_unavailable:{e}")));
        }
    }
    if report.notes.iter().any(|n| n.contains("scan:cancelled")) {
        report.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
        return report;
    }
    report.products = reconcile_views(&uninstall, &installer, &db);
    report
}

/// Full read-only reconciliation. Unknown views supply positive evidence only.
pub fn scan_msi_consistency() -> MsiConsistencyReport {
    if crate::scan_task::cancelled() {
        return MsiConsistencyReport {
            completeness: crate::scan_contract::ScanCompleteness::cancelled(0),
            ..Default::default()
        };
    }
    let mut report = collect_report(
        || Collected {
            rows: crate::apps::scan_installed_inventory(),
            // The Vec-only inventory API cannot prove per-request completeness.
            // Do not infer missing Uninstall records until that API carries it.
            failures: vec!["inventory_completeness_unknown".into()],
        },
        crate::regscan::snapshot_names,
        msi_db_products,
        |location| {
            if location.is_empty() || !crate::fsutil::is_local_drive_path(location) {
                None
            } else {
                std::path::Path::new(location).try_exists().ok()
            }
        },
    );
    if crate::scan_task::cancelled() {
        report.products.clear();
        report.completeness.status = crate::scan_contract::ScanStatus::Cancelled;
    }
    report
}

/// Keep the shared executable/token validation, but require the entire /x
/// argument to be a GUID: first_guid alone also accepts embedded brace groups.
fn super_msiexec_guid(cmd: &str) -> Option<String> {
    let parsed = crate::apps::msiexec_uninstall_guid_public(cmd)?;
    let code = normalize_product_code(msiexec_product_argument(cmd)?)?;
    (normalize_product_code(&parsed)? == code).then_some(code)
}

fn msiexec_product_argument(cmd: &str) -> Option<&str> {
    let cmd = cmd.trim();
    let (exe, rest) = if let Some(quoted) = cmd.strip_prefix('"') {
        let end = quoted.find('"')?;
        (&quoted[..end], &quoted[end + 1..])
    } else {
        cmd.split_at(cmd.find(char::is_whitespace).unwrap_or(cmd.len()))
    };
    let basename = exe.rsplit(['\\', '/']).next()?;
    if !basename.eq_ignore_ascii_case("msiexec") && !basename.eq_ignore_ascii_case("msiexec.exe") {
        return None;
    }
    let mut args = rest.split_whitespace();
    while let Some(arg) = args.next() {
        if arg.eq_ignore_ascii_case("/x") {
            return Some(args.next().unwrap_or(""));
        }
        if arg.get(..2).is_some_and(|s| s.eq_ignore_ascii_case("/x")) {
            return Some(&arg[2..]);
        }
    }
    None
}

#[cfg(test)]
#[path = "msiconsistency/collection_tests.rs"]
mod collection_tests;

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
