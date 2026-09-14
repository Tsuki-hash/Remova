//! Startup / service / scheduled-task listing and enable-disable (P1-2).

use serde::{Deserialize, Serialize};

use crate::safety::critical_service_names;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManageItem {
    pub name: String,
    pub detail: String,
    pub location: String,
    pub enabled: bool,
}

const RUN_KEYS: &[(&str, &str, &str)] = &[
    (
        "HKLM64",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "64",
    ),
    (
        "HKLM32",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "32",
    ),
    (
        "HKCU",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "64",
    ),
    (
        "HKCU",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
        "64",
    ),
];

pub fn list_startup_items() -> Vec<ManageItem> {
    let mut out = Vec::new();
    for (alias, sub, _view) in RUN_KEYS {
        let key = format!(r"{}\{}", alias, sub);
        for (vname, vdata) in crate::regscan::list_values(&key) {
            let display = vname.trim_end_matches(".remova-disabled").to_string();
            // Prefer StartupApproved flag; fall back to legacy rename suffix.
            let enabled = startup_approved_enabled(&key, &display)
                .unwrap_or(!vname.ends_with(".remova-disabled"));
            out.push(ManageItem {
                name: display,
                detail: vdata.chars().take(160).collect(),
                location: format!("{key}::{vname}"),
                enabled,
            });
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Read Explorer StartupApproved\Run binary for a value name. None if missing.
fn startup_approved_enabled(run_key: &str, value_name: &str) -> Option<bool> {
    let low = run_key.to_uppercase().replace('/', "\\");
    let sa_key = if low.contains("\\RUNONCE") {
        return None;
    } else if low.contains("HKLM32") {
        r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    } else if low.starts_with("HKLM") {
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    } else {
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    };
    crate::regscan::read_binary(sa_key, value_name).map(|b| {
        // 0x03 in first byte = disabled; anything else (incl. missing/0x02) = enabled
        !(b.len() >= 1 && b[0] == 0x03)
    })
}

pub fn list_services() -> Vec<ManageItem> {
    let mut out = Vec::new();
    let keys = [
        ("HKLM64", r"SYSTEM\CurrentControlSet\Services"),
        ("HKLM32", r"SYSTEM\CurrentControlSet\Services"),
    ];
    let mut seen = std::collections::HashSet::new();
    for (alias, sub) in keys {
        let root = format!(r"{alias}\{sub}");
        for svc in crate::regscan::list_subkeys(&root) {
            if svc.contains('\\') {
                continue;
            }
            if critical_service_names()
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&svc))
            {
                continue;
            }
            let path = format!(r"{root}\{svc}");
            if !seen.insert(svc.to_lowercase()) {
                continue;
            }
            let start = crate::regscan::read_dword(&path, "Start").unwrap_or(3);
            // 2=auto, 3=manual, 4=disabled
            let enabled = start != 4;
            let display_name = crate::regscan::read_string(&path, "DisplayName")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| svc.clone());
            out.push(ManageItem {
                name: svc,
                detail: display_name,
                location: path,
                enabled,
            });
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Parse `schtasks /query /fo CSV /v` rows (best-effort).
pub fn list_scheduled_tasks() -> Vec<ManageItem> {
    #[cfg(not(windows))]
    {
        Vec::new()
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        let Ok(out) = Command::new("schtasks")
            .args(["/query", "/fo", "CSV", "/v"])
            .output()
        else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut items = Vec::new();
        let mut lines = text.lines();
        let Some(header) = lines.next() else {
            return items;
        };
        let headers: Vec<String> = header
            .split(',')
            .map(|h| h.trim_matches('"').to_lowercase())
            .collect();
        let idx_task = headers.iter().position(|h| h.contains("taskname") || h == "任务名");
        let idx_status = headers
            .iter()
            .position(|h| h.contains("status") || h == "状态");
        let idx_comment = headers
            .iter()
            .position(|h| h.contains("comment") || h.contains("comment") || h == "注释");
        for line in lines {
            let cols = split_csv_line(line);
            let Some(ti) = idx_task else { continue };
            let Some(name) = cols.get(ti) else { continue };
            if name.is_empty() || name.eq_ignore_ascii_case("taskname") {
                continue;
            }
            // Skip Microsoft\Windows noise lightly
            let lower = name.to_lowercase();
            if lower.starts_with("\\microsoft\\windows\\") {
                continue;
            }
            let status = idx_status.and_then(|i| cols.get(i).cloned()).unwrap_or_default();
            let comment = idx_comment
                .and_then(|i| cols.get(i).cloned())
                .unwrap_or_default();
            let enabled = !status.eq_ignore_ascii_case("disabled") && !status.eq_ignore_ascii_case("已禁用");
            items.push(ManageItem {
                name: name.clone(),
                detail: if comment.is_empty() {
                    status
                } else {
                    comment.chars().take(120).collect()
                },
                location: name.clone(),
                enabled,
            });
        }
        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        items.truncate(500);
        items
    }
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_q && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_q = !in_q;
                }
            }
            ',' if !in_q => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Enable/disable a Run startup value via Explorer `StartupApproved\Run` binary flag.
/// Falls back to renaming a legacy `.remova-disabled` value if present.
///
/// Windows reads every value under Run at logon; only StartupApproved actually blocks it.
/// Binary layout (12 bytes): byte0 = 0x02 enabled / 0x03 disabled.
pub fn set_startup_enabled(location: &str, enabled: bool) -> Result<(), String> {
    let Some((key, vname)) = location.rsplit_once("::") else {
        return Err("bad startup location".into());
    };
    let base = vname.trim_end_matches(".remova-disabled");
    let cur_disabled = vname.ends_with(".remova-disabled");

    // Migrate legacy rename-based disable back to original name first.
    if cur_disabled {
        crate::regops::rename_reg_value(key, vname, base)?;
    }

    write_startup_approved(key, base, enabled)
}

fn write_startup_approved(run_key: &str, value_name: &str, enabled: bool) -> Result<(), String> {
    // Map Run key → matching StartupApproved hive/view.
    // HKCU\...\Run → HKCU\...\Explorer\StartupApproved\Run
    // HKLM64/32\...\Run → same hive Explorer\StartupApproved\Run (Run32 for 32-bit view)
    let low = run_key.to_uppercase().replace('/', "\\");
    let sa_key = if low.contains("\\RUNONCE") {
        // RunOnce has no StartupApproved companion; leave value intact (one-shot).
        return Ok(());
    } else if low.contains("HKLM32") {
        r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    } else if low.starts_with("HKLM") {
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    } else {
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
    };

    let mut buf = [0u8; 12];
    buf[0] = if enabled { 0x02 } else { 0x03 };
    crate::regops::write_reg_binary(sa_key, value_name, &buf)
}

/// Set service Start=4 (disabled) or 3 (manual) — not auto to avoid surprise.
pub fn set_service_start_disabled(name: &str, disable: bool) -> Result<(), String> {
    if name.trim().is_empty() || name.contains('\\') {
        return Err("bad service name".into());
    }
    if critical_service_names()
        .iter()
        .any(|c| c.eq_ignore_ascii_case(name))
    {
        return Err("critical system service protected".into());
    }
    let start: u32 = if disable { 4 } else { 3 };
    crate::regops::write_service_start(name, start)
}

pub fn set_task_enabled(task_name: &str, enabled: bool) -> Result<(), String> {
    if task_name.trim().is_empty() {
        return Err("empty task".into());
    }
    #[cfg(not(windows))]
    {
        let _ = (task_name, enabled);
        Err("not windows".into())
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        let action = if enabled { "/enable" } else { "/disable" };
        let st = Command::new("schtasks")
            .args(["/change", "/tn", task_name, action])
            .output()
            .map_err(|e| e.to_string())?;
        if st.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&st.stderr).trim().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn csv_split_quotes() {
        let cols = super::split_csv_line(r#""C:\a","say ""hi""","x""#);
        assert_eq!(cols[0], r"C:\a");
        assert_eq!(cols[1], r#"say "hi""#);
        assert_eq!(cols[2], "x");
    }

    #[cfg(windows)]
    #[test]
    fn list_services_skips_critical() {
        let svcs = super::list_services();
        for s in &svcs {
            assert!(!crate::safety::critical_service_names()
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&s.name)));
        }
    }

    #[cfg(windows)]
    #[test]
    fn list_startup_runs() {
        let items = super::list_startup_items();
        for i in &items {
            assert!(i.location.contains("::"));
        }
    }
}
