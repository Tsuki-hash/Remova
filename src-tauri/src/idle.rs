//! Idle software radar — rank installed apps that look unused and large.
//! Read-only; never deletes. Evidence is install age + install-dir mtime + size.

use crate::apps::InstalledApp;
use crate::constants::{IDLE_MIN_DAYS, IDLE_MIN_SIZE_KB, IDLE_RESULT_CAP};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleApp {
    pub app: InstalledApp,
    /// Days idle = min(install-age, dir-mtime-age) — recent file activity lowers idle.
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

/// Civil date → days since Unix epoch (proleptic Gregorian; handles leap years).
fn civil_to_epoch_days(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64;
    let m = m as i64;
    let d = d as i64;
    // Howard Hinnant's days_from_civil
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn parse_ymd(y: i32, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let now_days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|x| x.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    Some((now_days - civil_to_epoch_days(y, m, d)).max(0))
}

fn parse_install_date_days(install_date: &str) -> Option<i64> {
    let s = install_date.trim();
    if s.is_empty() {
        return None;
    }
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    // YYYYMMDD / YYYY/MM/DD
    if digits.len() >= 8 {
        let (y, m, d) = (
            digits[0..4].parse::<i32>().ok()?,
            digits[4..6].parse::<u32>().ok()?,
            digits[6..8].parse::<u32>().ok()?,
        );
        if let Some(n) = parse_ymd(y, m, d) {
            return Some(n);
        }
    }
    // M/D/YYYY or YYYY/M/D (registry leftovers)
    let parts: Vec<&str> = s.split(['/', '-', '.']).collect();
    if parts.len() == 3 {
        let a = parts[0].parse::<i32>().ok();
        let b = parts[1].parse::<u32>().ok();
        let c = parts[2].parse::<i32>().ok();
        if let (Some(a), Some(b), Some(c)) = (a, b, c) {
            // YYYY/M/D
            if a >= 1000 {
                return parse_ymd(a, b, c.max(0) as u32);
            }
            // M/D/YYYY
            if c >= 1000 {
                return parse_ymd(c, a.max(0) as u32, b);
            }
        }
    }
    None
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
        // Private cancel flag: estimate-batch cancel must not zero idle sizes.
        return crate::dirsize::walk_size_kb_with(
            std::path::Path::new(&app.install_location),
            &std::sync::atomic::AtomicBool::new(false),
        );
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
                    // min: any recent activity signal means the app is not idle.
                    Some(cur) => cur.min(d),
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
    fn civil_epoch_days_handles_leap_year() {
        // 2020-03-01 - 2020-02-28 = 2 days (leap Feb)
        let a = super::civil_to_epoch_days(2020, 2, 28);
        let b = super::civil_to_epoch_days(2020, 3, 1);
        assert_eq!(b - a, 2);
        // 2019-03-01 - 2019-02-28 = 1 day (non-leap)
        let c = super::civil_to_epoch_days(2019, 2, 28);
        let d = super::civil_to_epoch_days(2019, 3, 1);
        assert_eq!(d - c, 1);
    }

    #[test]
    fn parse_install_date_days_accepts_yyyymmdd() {
        assert!(parse_install_date_days("20200101").unwrap() > 1000);
        assert!(parse_install_date_days("").is_none());
        assert!(parse_install_date_days("garbage").is_none());
    }

    #[test]
    fn parse_install_date_days_accepts_mdy() {
        // 1/15/2020 should parse and be a large idle age
        assert!(parse_install_date_days("1/15/2020").unwrap() > 1000);
        assert!(parse_install_date_days("2020/1/15").unwrap() > 1000);
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

    #[test]
    fn rank_uses_min_of_install_age_and_dir_mtime() {
        // Fresh install dir (mtime = now) must not be idle even if InstallDate is ancient.
        let dir = std::env::temp_dir().join(format!("remova_idle_min_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("recent.bin"), b"x").unwrap();
        let old = app(
            "Active",
            "20150101",
            &dir.to_string_lossy(),
            2_000_000,
        );
        let ranked = rank_idle_apps(&[old]);
        assert!(
            ranked.is_empty(),
            "recent dir mtime must cancel install-age idle (got {:?})",
            ranked.iter().map(|r| r.idle_days).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
