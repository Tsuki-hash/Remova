//! Session-scoped reboot-pending registry for MSI 1641.
//!
//! A 1641 exit means the uninstaller already initiated a restart. Residual
//! deletion must not run until the machine has actually rebooted, but the
//! official uninstall and the residual cleanup happen in separate IPC calls
//! (deep uninstall, batch retry). This registry carries that fact across
//! calls. It is in-memory on purpose: a real reboot ends the process, so the
//! flag cannot leak into the next session.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

pub(crate) const INITIATED_MESSAGE: &str =
    "official uninstaller initiated reboot; residual cleanup skipped";
pub(crate) const PENDING_MESSAGE: &str = "system reboot pending; residual cleanup postponed";

fn pending() -> &'static Mutex<HashSet<String>> {
    static PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn mark_pending(app_key: &str) {
    if app_key.is_empty() {
        return;
    }
    if let Ok(mut set) = pending().lock() {
        set.insert(app_key.to_string());
    }
}

pub(crate) fn is_pending(app_key: &str) -> bool {
    !app_key.is_empty()
        && pending()
            .lock()
            .map(|set| set.contains(app_key))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn pending_is_scoped_to_one_app_key() {
        let key = "reboot-test\u{0}source\u{0}app";
        super::mark_pending(key);
        assert!(super::is_pending(key));
        assert!(!super::is_pending("reboot-test\u{0}source\u{0}other"));
        assert!(!super::is_pending(""));
    }
}
