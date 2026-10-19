//! Shared product constants — keep magic numbers out of business logic.

/// Official uninstaller wait cap.
pub const UNINSTALL_TIMEOUT_SECS: u64 = 300;

/// Safety Vault session retention (days).
pub const BACKUP_RETENTION_DAYS: u64 = 7;

/// AI batch explain item cap.
pub const AI_EXPLAIN_MAX_ITEMS: usize = 12;

/// Install-monitor path budget.
pub const INSTALLMON_PATH_BUDGET: usize = 80_000;

/// Install-monitor walk depth cap. Junctions are refused outright; this bounds
/// recursion on pathological (non-reparse) trees regardless of std version.
pub const INSTALLMON_MAX_WALK_DEPTH: usize = 64;

/// Orphan scan result cap.
pub const ORPHAN_RESULT_CAP: usize = 80;

/// Installer / tool-cache scan result cap.
pub const SPECIALTY_RESULT_CAP: usize = 80;

/// Children enumerated per shared cache root (`~/.cache`, JetBrains).
/// A shared root holds many tools' caches, so the cap bounds one level of
/// enumeration instead of the root ever becoming a single cleanup target.
pub const TOOL_CACHE_CHILD_CAP: usize = 60;

/// Per-phase result cap for the installer scan.
///
/// Downloads packages and updater directories are independent result sets that
/// used to share one budget, so a large Downloads folder could crowd updater
/// directories out entirely. Each phase is now ordered and truncated on its
/// own, and the scan reports up to twice this.
pub const INSTALLER_PHASE_CAP: usize = 60;

/// Leaf directory names that mark an updater cache inside a product directory.
pub const UPDATER_CACHE_LEAVES: &[&str] = &[
    "updater",
    "updates",
    "packages",
    "installcache",
    "squirreltemp",
];

/// Floor for the ambiguous download tiers (a PE executable with no installer
/// name, or an archive). Installers smaller than this are usually stubs.
pub const MIN_AMBIGUOUS_PACKAGE_BYTES: u64 = 1024 * 1024;

/// Idle-radar result cap.
pub const IDLE_RESULT_CAP: usize = 50;

/// Idle radar thresholds (soft evidence only).
pub const IDLE_MIN_DAYS: i64 = 90;
pub const IDLE_MIN_SIZE_KB: i64 = 100 * 1024;

/// Disk radar rows per parent root.
pub const DISK_RADAR_TOP_N: usize = 12;

/// History list cap.
pub const HISTORY_LIST_CAP: usize = 200;

/// History CSV export cap.
pub const HISTORY_CSV_CAP: usize = 500;

/// Scheduled-task manage list cap.
pub const MANAGE_TASK_LIST_CAP: usize = 500;

/// Update-check endpoint — single place to repoint the URL on repo transfer.
pub const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/Tsuki-hash/Remova/releases/latest";

/// Fallback link when the API response carries no `html_url`.
pub const GITHUB_RELEASES_PAGE: &str = "https://github.com/Tsuki-hash/Remova/releases";
