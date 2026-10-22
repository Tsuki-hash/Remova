//! Orphan Windows Firewall rules (SOP §8-1).
//!
//! Rules are REG_SZ values under the FirewallRules key, pipe-separated
//! `key=value` fields. A rule whose `App=` path no longer exists cannot be
//! doing anything useful for an installed product; scanning is read-only, and
//! deletion is a separate command that re-verifies every value name and backs
//! the key up first.

use serde::{Deserialize, Serialize};

pub const FIREWALL_RULES_KEY: &str = r"HKLM64\SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules";
const RESULT_CAP: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallRule {
    /// Registry value name — the identity used for deletion.
    pub value_name: String,
    pub rule_name: String,
    /// Executable path from the `App=` field (empty rules are skipped).
    pub app_path: String,
    pub direction: String,
    pub action: String,
    pub protocol: String,
    /// Stable code the UI localises.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallScanResult {
    pub rules: Vec<FirewallRule>,
    #[serde(default)]
    pub omitted: u32,
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
        rule_name,
        app_path,
        direction: field(&fields, "Dir").unwrap_or("").to_string(),
        action: field(&fields, "Action").unwrap_or("").to_string(),
        protocol: field(&fields, "Protocol").unwrap_or("").to_string(),
        reason: FIREWALL_APP_MISSING.into(),
    })
}

/// Rules whose `App=` executable no longer exists. `try_exists` errors (ACL,
/// offline path) are treated as "still unknown" and never offered.
pub fn scan_orphan_firewall_rules() -> FirewallScanResult {
    let mut rules = Vec::new();
    for (value_name, raw) in crate::regscan::list_values(FIREWALL_RULES_KEY) {
        if crate::scan_task::cancelled() {
            break;
        }
        let Some(rule) = parse_rule(&value_name, &raw) else {
            continue;
        };
        if std::path::Path::new(&rule.app_path)
            .try_exists()
            .is_ok_and(|exists| !exists)
        {
            rules.push(rule);
        }
    }
    finalize_rules(rules)
}

/// Sort before capping: registry enumeration order must not decide which 200
/// rules are shown, otherwise a fresh delete re-scan can disagree with the
/// list the user selected from.
fn finalize_rules(mut rules: Vec<FirewallRule>) -> FirewallScanResult {
    rules.sort_by(|a, b| {
        a.rule_name
            .to_lowercase()
            .cmp(&b.rule_name.to_lowercase())
            .then_with(|| a.value_name.cmp(&b.value_name))
    });
    let omitted = rules.len().saturating_sub(RESULT_CAP) as u32;
    rules.truncate(RESULT_CAP);
    FirewallScanResult { rules, omitted }
}

fn ensure_still_orphans(wanted: &[String], fresh: &[FirewallRule]) -> Result<(), String> {
    let allowed: std::collections::HashSet<&str> =
        fresh.iter().map(|r| r.value_name.as_str()).collect();
    if let Some(unknown) = wanted.iter().find(|n| !allowed.contains(n.as_str())) {
        return Err(format!("firewall rule is no longer an orphan: {unknown}"));
    }
    Ok(())
}

/// Delete the given rule values after a fresh scan proves each one is still an
/// orphan. All keys are exported to a Safety Vault session before any delete.
pub fn delete_firewall_rules(value_names: &[String]) -> Result<FirewallCleanupReport, String> {
    let _guard = crate::executor::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if crate::executor::reboot::any_pending() {
        return Err(crate::executor::reboot::PENDING_MESSAGE.to_string());
    }
    let mut wanted: Vec<String> = value_names
        .iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    wanted.sort();
    wanted.dedup();
    if wanted.is_empty() {
        return Err("no rules selected".into());
    }
    let fresh = scan_orphan_firewall_rules();
    ensure_still_orphans(&wanted, &fresh.rules)?;

    let session = crate::backup::create_session("firewall-rules")
        .map_err(|e| format!("backup session failed: {e}"))?;
    let items: Vec<crate::scanner::CleanupItem> = wanted
        .iter()
        .map(|name| registry_item(&format!("{FIREWALL_RULES_KEY}|{name}")))
        .collect();
    let (_ok, fail, errors) = crate::backup::backup_items(&items, &session);
    if fail > 0 {
        return Err(format!(
            "backup failed; firewall rules kept: {}",
            errors.join("; ")
        ));
    }

    let mut removed = Vec::new();
    let mut failed = Vec::new();
    for name in &wanted {
        match crate::regops::delete_value(FIREWALL_RULES_KEY, name) {
            Ok(true) => removed.push(name.clone()),
            Ok(false) => failed.push(format!("{name}: already absent")),
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    Ok(FirewallCleanupReport {
        removed,
        failed,
        backup_dir: session.to_string_lossy().into_owned(),
    })
}

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
        let rule = |name: &str| FirewallRule {
            value_name: name.into(),
            rule_name: name.into(),
            app_path: r"C:\Tools\gone.exe".into(),
            direction: "In".into(),
            action: "Allow".into(),
            protocol: "6".into(),
            reason: FIREWALL_APP_MISSING.into(),
        };
        let fresh = vec![rule("a")];
        assert!(ensure_still_orphans(&["a".to_string()], &fresh).is_ok());
        assert!(ensure_still_orphans(&["a".to_string(), "b".to_string()], &fresh).is_err());
    }

    #[test]
    fn result_cap_is_applied_after_deterministic_sorting() {
        let rule = |n: u32| FirewallRule {
            value_name: format!("value-{n:03}"),
            rule_name: format!("Rule {n:03}"),
            app_path: format!(r"C:\Tools\gone-{n}.exe"),
            direction: "In".into(),
            action: "Allow".into(),
            protocol: "6".into(),
            reason: FIREWALL_APP_MISSING.into(),
        };
        let mut input: Vec<FirewallRule> = (0..205).rev().map(rule).collect();
        input.reverse();
        let result = finalize_rules(input);
        assert_eq!(result.rules.len(), RESULT_CAP);
        assert_eq!(result.omitted, 5);
        assert_eq!(result.rules[0].rule_name, "Rule 000");
        assert_eq!(
            result.rules.last().map(|r| r.rule_name.as_str()),
            Some("Rule 199")
        );
    }
}
