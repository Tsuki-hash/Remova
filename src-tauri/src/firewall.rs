//! Orphan Windows Firewall rules (SOP §8-1).
//!
//! Rules are REG_SZ values under the FirewallRules key, pipe-separated
//! `key=value` fields. A rule whose `App=` path no longer exists cannot be
//! doing anything useful for an installed product; scanning is read-only, and
//! deletion is a separate command that verifies and backs up each selected value.

use serde::{Deserialize, Serialize};

pub const FIREWALL_RULES_KEY: &str = r"HKLM64\SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules";
const RESULT_CAP: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallRule {
    pub value_name: String,
    pub revision: String,
    pub rule_name: String,
    /// Executable path from the `App=` field (empty rules are skipped).
    pub app_path: String,
    pub direction: String,
    pub action: String,
    pub protocol: String,
    /// Stable code the UI localises.
    pub reason: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Deserialize)]
pub struct FirewallSelection {
    pub value_name: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallScanResult {
    pub rules: Vec<FirewallRule>,
    #[serde(default)]
    pub omitted: u32,
    #[serde(default)]
    pub completeness: crate::scan_contract::ScanCompleteness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallCleanupReport {
    pub removed: Vec<String>,
    pub failed: Vec<String>,
    pub backup_dir: String,
}

pub const FIREWALL_APP_MISSING: &str = "firewall_app_missing";

fn field<'a>(parts: &'a [&'a str], key: &str) -> Option<&'a str> {
    parts
        .iter()
        .find_map(|p| p.strip_prefix(&format!("{key}=")))
}

fn parse_rule(value_name: &str, raw: &str) -> Option<FirewallRule> {
    let mut parts = raw.split('|');
    let version = parts.next()?;
    if !version.starts_with('v') || !version[1..].chars().next()?.is_ascii_digit() {
        return None;
    }
    let fields: Vec<&str> = parts.collect();
    let app_path = field(&fields, "App")
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .trim()
        .to_string();
    if app_path.is_empty() || app_path.contains('%') {
        return None;
    }
    let name = field(&fields, "Name")
        .map(str::trim)
        .filter(|n| !n.is_empty());
    // Resource-named rules are Windows built-ins; SOP §8-1 excludes them even
    // when their App path is missing.
    if name.is_some_and(|n| n.starts_with('@')) {
        return None;
    }
    let rule_name = name.unwrap_or(value_name).to_string();
    Some(FirewallRule {
        value_name: value_name.to_string(),
        revision: String::new(),
        rule_name,
        app_path,
        direction: field(&fields, "Dir").unwrap_or("").to_string(),
        action: field(&fields, "Action").unwrap_or("").to_string(),
        protocol: field(&fields, "Protocol").unwrap_or("").to_string(),
        reason: FIREWALL_APP_MISSING.into(),
    })
}

fn rule_from_value(name: &str, bytes: &[u8], kind: u32) -> Option<FirewallRule> {
    // REG_SZ is UTF-16LE. Reject embedded NULs and expansion rules.
    let (words, trailing) = bytes.as_chunks::<2>();
    if kind != 1 || !trailing.is_empty() {
        return None;
    }
    let mut words: Vec<u16> = words.iter().copied().map(u16::from_le_bytes).collect();
    if words.last() == Some(&0) {
        words.pop();
    }
    if words.contains(&0) {
        return None;
    }
    let mut rule = parse_rule(name, &String::from_utf16(&words).ok()?)?;
    rule.revision = crate::path_seal::map_digest(bytes);
    (!rule.revision.is_empty()).then_some(rule)
}

fn app_missing(rule: &FirewallRule) -> bool {
    // Resolve locally without following a junction that could lead to SMB.
    let path = std::path::Path::new(&rule.app_path);
    crate::fsutil::is_local_drive_path(&rule.app_path)
        && crate::fsutil::pin_local_scan_path(path)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

/// Public parse wrapper for the portable-candidate cross-reference; the
/// fields beyond `App=` are display-only there.
pub fn parse_rule_public(value_name: &str, raw: &str) -> Option<FirewallRule> {
    parse_rule(value_name, raw)
}

/// Only rules with paths confirmed missing are offered.
pub fn scan_orphan_firewall_rules() -> FirewallScanResult {
    let mut rules = Vec::new();
    let mut completeness = crate::scan_contract::ScanCompleteness::complete();
    let values = match crate::regscan::snapshot_names(FIREWALL_RULES_KEY) {
        Ok((_, values)) => values,
        Err(e) if e == "scan:cancelled" => {
            completeness = crate::scan_contract::ScanCompleteness::cancelled(0);
            return FirewallScanResult {
                rules,
                omitted: 0,
                completeness,
            };
        }
        Err(_) => {
            completeness =
                crate::scan_contract::ScanCompleteness::failed(FIREWALL_RULES_KEY.into());
            return FirewallScanResult {
                rules,
                omitted: 0,
                completeness,
            };
        }
    };
    completeness.roots_scanned = 1;
    for value_name in values {
        if crate::scan_task::cancelled() {
            completeness.status = crate::scan_contract::ScanStatus::Cancelled;
            break;
        }
        let Ok(Some((raw, kind))) =
            crate::regops::read_reg_value_bytes(FIREWALL_RULES_KEY, &value_name)
        else {
            // A value disappearing or becoming unreadable mid-snapshot means
            // the rule list is partial; do not treat it as a clean absence.
            completeness.status = crate::scan_contract::ScanStatus::Partial;
            completeness.failed_roots.push(FIREWALL_RULES_KEY.into());
            continue;
        };
        let Some(rule) = rule_from_value(&value_name, &raw, kind) else {
            continue;
        };
        if app_missing(&rule) {
            rules.push(rule);
        }
    }
    finalize_rules(rules, completeness)
}

/// Sort before capping: registry enumeration order must not decide which 200
/// rules are shown, otherwise a fresh delete re-scan can disagree with the
/// list the user selected from.
fn finalize_rules(
    mut rules: Vec<FirewallRule>,
    completeness: crate::scan_contract::ScanCompleteness,
) -> FirewallScanResult {
    rules.sort_by(|a, b| {
        a.rule_name
            .to_lowercase()
            .cmp(&b.rule_name.to_lowercase())
            .then_with(|| a.value_name.cmp(&b.value_name))
    });
    let omitted = rules.len().saturating_sub(RESULT_CAP) as u32;
    rules.truncate(RESULT_CAP);
    FirewallScanResult {
        rules,
        omitted,
        completeness,
    }
}

fn validate_selection(
    selection: &FirewallSelection,
    value: Option<(Vec<u8>, u32)>,
) -> Result<(), String> {
    if value
        .and_then(|(raw, kind)| rule_from_value(&selection.value_name, &raw, kind))
        .is_some_and(|rule| rule.revision == selection.revision && app_missing(&rule))
    {
        return Ok(());
    }
    Err(stale_rule(selection))
}

fn stale_rule(selection: &FirewallSelection) -> String {
    format!(
        "firewall rule is no longer an orphan or has changed: {}",
        selection.value_name
    )
}

/// Revalidate selected values and retain their registry keys through backup and deletion.
pub fn delete_firewall_rules(
    selections: &[FirewallSelection],
) -> Result<FirewallCleanupReport, String> {
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::executor::begin_cleanup_control();
    if crate::executor::reboot::any_pending() {
        return Err(crate::executor::reboot::PENDING_MESSAGE.to_string());
    }
    delete_verified_rules(selections)
}

#[cfg(not(windows))]
fn delete_verified_rules(_: &[FirewallSelection]) -> Result<FirewallCleanupReport, String> {
    Err("firewall cleanup requires Windows".into())
}

#[cfg(windows)]
fn delete_verified_rules(
    selections: &[FirewallSelection],
) -> Result<FirewallCleanupReport, String> {
    let mut wanted = selections.to_vec();
    wanted.sort_unstable();
    wanted.dedup();
    if wanted.is_empty() {
        return Err("no rules selected".into());
    }
    if wanted.len() > RESULT_CAP
        || wanted
            .windows(2)
            .any(|pair| pair[0].value_name == pair[1].value_name)
        || wanted
            .iter()
            .any(|entry| entry.value_name.trim().is_empty())
    {
        return Err("invalid firewall selection".into());
    }

    let mut sources = Vec::with_capacity(wanted.len());
    for selection in &wanted {
        let path = format!("{FIREWALL_RULES_KEY}|{}", selection.value_name);
        let source =
            crate::regops::RegistrySource::capture(&path)?.ok_or_else(|| stale_rule(selection))?;
        validate_selection(selection, source.verified_value()?)?;
        sources.push((registry_item(&path), source));
    }
    let session = crate::backup::create_session("firewall-rules")
        .map_err(|e| format!("backup session failed: {e}"))?;
    let items: Vec<_> = sources.iter().map(|(item, _)| item.clone()).collect();
    let (ok, fail, errors) = crate::backup::backup_items(&items, &session);
    if fail > 0 || ok as usize != items.len() {
        return Err(format!(
            "backup failed; firewall rules kept: {}",
            errors.join("; ")
        ));
    }

    let (seal, _seal_pin) = crate::path_seal::hold_verified_seal(&session)?;
    let mut export_pins = Vec::with_capacity(sources.len());
    for (item, source) in &sources {
        let relative = crate::backup::registry_restore_snapshot(&item.path, true);
        let (bytes, pin) = crate::backup::read_backup_metadata(
            &session.join(&relative),
            "seal:registry_snapshot_too_large",
            crate::executor::cleanup_cancelled,
        )?;
        let digest = crate::path_seal::map_digest(&bytes);
        if seal.reg_digests.get(&relative) != Some(&digest) || !source.matches_backup(&digest) {
            return Err("registry source could not be bound to its backup".into());
        }
        export_pins.push(pin);
    }

    let mut removed = Vec::new();
    let mut failed = Vec::new();
    for ((item, source), selection) in sources.iter().zip(&wanted) {
        let result = (|| {
            validate_selection(selection, source.verified_value()?)?;
            source
                .delete_value(&item.path)
                .ok_or("expected a firewall registry value")?
        })();
        match result {
            Ok(true) => removed.push(selection.value_name.clone()),
            Ok(false) => failed.push(format!("{}: already absent", selection.value_name)),
            Err(e) => failed.push(format!("{}: {e}", selection.value_name)),
        }
    }
    Ok(FirewallCleanupReport {
        removed,
        failed,
        backup_dir: session.to_string_lossy().into_owned(),
    })
}

#[cfg(windows)]
fn registry_item(path: &str) -> crate::scanner::CleanupItem {
    use crate::scanner::{CleanupItem, Confidence, ItemKind, RiskLevel};
    CleanupItem {
        path: path.into(),
        kind: ItemKind::Registry,
        score: 60,
        confidence: Confidence::Suspected,
        risk: RiskLevel::Medium,
        reason: "orphan firewall rule".into(),
        evidence: vec![],
        shared: false,
        service_bound: false,
        user_data: false,
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v2_rule_fields() {
        let raw = "v2.10|Action=Allow|Active=TRUE|Dir=In|Protocol=6|LPort=445|App=C:\\Tools\\gone.exe|Name=Vendor File Share|Desc=x";
        let rule = parse_rule("Vendor-Share-In", raw).expect("rule");
        assert_eq!(rule.value_name, "Vendor-Share-In");
        assert_eq!(rule.rule_name, "Vendor File Share");
        assert_eq!(rule.app_path, r"C:\Tools\gone.exe");
        assert_eq!(rule.direction, "In");
        assert_eq!(rule.action, "Allow");
        assert_eq!(rule.protocol, "6");
    }

    #[test]
    fn strips_quotes_from_app_paths() {
        let rule = parse_rule(
            "quoted-app",
            r#"v2.10|Action=Allow|Dir=In|App="C:\Program Files\Vendor\app.exe""#,
        )
        .expect("quoted App path");
        assert_eq!(rule.app_path, r"C:\Program Files\Vendor\app.exe");
    }

    #[test]
    fn ignores_rules_without_an_exe_or_with_resources() {
        assert!(parse_rule("a", "v2.10|Action=Allow|Dir=In|Name=NoApp").is_none());
        assert!(parse_rule("a", "v2.10|App=%SystemRoot%\\x.exe|Name=x").is_none());
        assert!(parse_rule("a", "not-a-rule").is_none());
        assert!(
            parse_rule(
                "value-name",
                "v2.10|App=C:\\x.exe|Name=@firewallapi.dll,-28752"
            )
            .is_none(),
            "resource-named built-in rules must not be offered"
        );
    }

    #[test]
    fn delete_selection_must_still_be_orphaned_in_a_fresh_scan() {
        let dir = crate::test_support::TempDir::new("firewall-revision");
        let exe = dir.path().join("gone.exe");
        let encode = |action| {
            format!("v2.10|App={}|Action={action}|", exe.display())
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        let raw = encode("Allow");
        let rule = rule_from_value("rule", &raw, 1).unwrap();
        let selected = FirewallSelection {
            value_name: rule.value_name,
            revision: rule.revision,
        };
        assert!(validate_selection(&selected, Some((raw.clone(), 1))).is_ok());
        assert!(validate_selection(&selected, None).is_err());
        assert!(validate_selection(&selected, Some((encode("Block"), 1))).is_err());
        assert!(validate_selection(&selected, Some((raw.clone(), 2))).is_err());
        std::fs::write(&exe, b"reinstalled").unwrap();
        assert!(validate_selection(&selected, Some((raw, 1))).is_err());
    }

    #[test]
    fn a_pending_reboot_blocks_firewall_deletion_before_any_scan() {
        crate::executor::reboot::reset_for_test();
        crate::executor::reboot::mark_pending("source\u{0}key\u{0}App");
        let err = delete_firewall_rules(&[]).unwrap_err();
        assert_eq!(err, crate::executor::reboot::PENDING_MESSAGE);
        crate::executor::reboot::reset_for_test();
    }

    #[test]
    fn result_cap_is_applied_after_deterministic_sorting() {
        let rule = |n: u32| FirewallRule {
            value_name: format!("value-{n:03}"),
            revision: String::new(),
            rule_name: format!("Rule {n:03}"),
            app_path: format!(r"C:\Tools\gone-{n}.exe"),
            direction: "In".into(),
            action: "Allow".into(),
            protocol: "6".into(),
            reason: FIREWALL_APP_MISSING.into(),
        };
        let mut input: Vec<FirewallRule> = (0..205).rev().map(rule).collect();
        input.reverse();
        let result = finalize_rules(input, crate::scan_contract::ScanCompleteness::complete());
        assert_eq!(result.rules.len(), RESULT_CAP);
        assert_eq!(result.omitted, 5);
        assert_eq!(result.rules[0].rule_name, "Rule 000");
        assert_eq!(
            result.rules.last().map(|r| r.rule_name.as_str()),
            Some("Rule 199")
        );
    }
}
