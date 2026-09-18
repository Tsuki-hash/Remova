//! Shared product constants (BE-06) — keep magic numbers out of business logic.

/// Official uninstaller wait cap.
pub const UNINSTALL_TIMEOUT_SECS: u64 = 300;

/// Safety Vault session retention (days).
pub const BACKUP_RETENTION_DAYS: u64 = 7;

/// AI batch explain item cap.
pub const AI_EXPLAIN_MAX_ITEMS: usize = 12;

/// Install-monitor path budget.
pub const INSTALLMON_PATH_BUDGET: usize = 80_000;

/// Orphan scan result cap.
pub const ORPHAN_RESULT_CAP: usize = 80;

/// History list cap.
pub const HISTORY_LIST_CAP: usize = 200;

/// History CSV export cap.
pub const HISTORY_CSV_CAP: usize = 500;

/// Scheduled-task manage list cap.
pub const MANAGE_TASK_LIST_CAP: usize = 500;
