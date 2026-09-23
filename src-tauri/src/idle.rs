//! Idle software radar — rank installed apps that look unused and large.
//! Read-only; never deletes. Evidence is install age + install-dir mtime + size.

use crate::apps::InstalledApp;
use crate::constants::{IDLE_MIN_DAYS, IDLE_MIN_SIZE_KB, IDLE_RESULT_CAP};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleApp {
    pub app: InstalledApp,
    /// Days since the strongest idle signal (min of install-date age and dir mtime age).
    pub idle_days: i64,
    pub size_kb: i64,
    /// Human-readable evidence lines (already localized codes + details).
    pub evidence: Vec<IdleEvidence>,
    /// Higher = more idle + more reclaimable.
    pub score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleEvidence {
    pub code: String,
    pub detail: String,
}

fn parse_install_date_days(install_date: &str) -> Option<i64> {
    let s = install_date.trim();
    if s.is_empty() {
        return None;
    }
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    let (y, m, d) = (
        digits[0..4].parse::<i32>().ok()?,
        digits[4..6].parse::<u32>().ok()?,
        digits[6..8].parse::<u32>().ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Approximate: days since civil date via unix-like epoch offset (no chrono dep).
    let now_days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|x| x.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // 1970-01-01 → approximate civil day number
    let civil = (y as i64 - 1970) * 365 + (m as i64 - 1) * 30 + d as i64;
    Some((now_days - civil).max(0))
}

fn dir_mtime_age_days(path: &str) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .ok()?
        .as_secs()
        / 86_400;
    Some(age as i64)
}

fn size_kb_of(app: &InstalledApp) -> i64 {
    if app.estimated_size_kb > 0 {
        return app.estimated_size_kb;
    }
    if !app.install_location.is_empty() {
        return crate::dirsize::walk_size_kb(std::path::Path::new(&app.install_location));
    }
    0
}

/// Rank idle candidates. Soft evidence only — caller must confirm before uninstall.
pub fn rank_idle_apps(installed: &[InstalledApp]) -> Vec<IdleApp> {
    let mut out: Vec<IdleApp> = Vec::new();
    for app in installed {
        if app.name.trim().is_empty() {
            continue;
        }
        let mut evidence: Vec<IdleEvidence> = Vec::new();
        let mut idle_days: Option<i64> = None;

        if let Some(d) = parse_install_date_days(&app.install_date) {
            evidence.push(IdleEvidence {
                code: "idle_install_age".into(),
                detail: format!("install_date={}", app.install_date),
            });
            idle_days = Some(d);
        }
        if !app.install_location.is_empty() {
            if let Some(d) = dir_mtime_age_days(&app.install_location) {
                evidence.push(IdleEvidence {
                    code: "idle_dir_mtime".into(),
                    detail: format!("{}d", d),
                });
                idle_days = Some(match idle_days {
                    Some(cur) => cur.max(d),
                    None => d,
                });
            }
        }
        let Some(idle_days) = idle_days else {
            continue;
        };
        if idle_days < IDLE_MIN_DAYS {
            continue;
        }
        let size_kb = size_kb_of(app);
        if size_kb < IDLE_MIN_SIZE_KB {
            continue;
        }
        let score = idle_days * (size_kb as i64 / 1024).max(1).ilog2() as i64;
        out.push(IdleApp {
            app: app.clone(),
            idle_days,
            size_kb,
            evidence,
            score,
        });
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out.truncate(IDLE_RESULT_CAP);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, install_date: &str, loc: &str, size: i64) -> InstalledApp {
        InstalledApp {
            name: name.into(),
            version: String::new(),
            publisher: String::new(),
            install_location: loc.into(),
            uninstall_string: "x".into(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{name}"),
            estimated_size_kb: size,
            install_date: install_date.into(),
            display_icon: String::new(),
        }
    }

    #[test]
    fn parse_install_date_days_accepts_yyyymmdd() {
        assert!(parse_install_date_days("20200101").unwrap() > 1000);
        assert!(parse_install_date_days("").is_none());
        assert!(parse_install_date_days("garbage").is_none());
    }

    #[test]
    fn rank_skips_small_and_recent() {
        let small = app("Small", "20200101", "", 10);
        let recent = app("Recent", "29991231", "", 5_000_000);
        let ranked = rank_idle_apps(&[small, recent]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_includes_old_and_large() {
        let old = app("Old", "20190101", "", 2_000_000);
        let ranked = rank_idle_apps(&[old]);
        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].idle_days >= IDLE_MIN_DAYS);
        assert!(ranked[0].score > 0);
    }
}
