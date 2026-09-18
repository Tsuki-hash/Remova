//! Structured Remova errors (BE-01): stable codes for UI mapping, human message for logs.

use std::fmt;

/// Lightweight structured error used by domain modules and Tauri commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovaError {
    /// Stable machine code, e.g. `safety:protected`, `manage:protected_registry`, `backup:path`.
    pub code: String,
    /// Human-readable detail (may be English; frontend maps `code` to i18n when known).
    pub message: String,
}

impl RemovaError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Serialize for IPC: `code::message` (frontend `formatError` can split on `::`).
    pub fn to_ipc(&self) -> String {
        if self.message.is_empty() {
            self.code.clone()
        } else {
            format!("{}::{}", self.code, self.message)
        }
    }
}

impl fmt::Display for RemovaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_ipc())
    }
}

impl std::error::Error for RemovaError {}

impl From<RemovaError> for String {
    fn from(e: RemovaError) -> Self {
        e.to_ipc()
    }
}

pub fn safety_err(msg: impl Into<String>) -> RemovaError {
    RemovaError::new("safety:protected", msg)
}

pub fn manage_err(code: &str, msg: impl Into<String>) -> RemovaError {
    RemovaError::new(format!("manage:{code}"), msg)
}

pub fn backup_err(msg: impl Into<String>) -> RemovaError {
    RemovaError::new("backup:failed", msg)
}

pub fn restore_err(msg: impl Into<String>) -> RemovaError {
    RemovaError::new("restore:failed", msg)
}

/// Split IPC error string into `(code, message)` when formatted as `code::message`.
pub fn split_ipc(s: &str) -> (&str, &str) {
    match s.split_once("::") {
        Some((c, m)) => (c, m),
        None => ("", s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_roundtrip() {
        let e = RemovaError::new("manage:protected_registry", "bad key");
        assert_eq!(e.to_ipc(), "manage:protected_registry::bad key");
        assert_eq!(
            split_ipc(&e.to_ipc()),
            ("manage:protected_registry", "bad key")
        );
        let s: String = e.into();
        assert!(s.starts_with("manage:protected_registry::"));
    }

    #[test]
    fn helpers_encode_prefix() {
        assert!(safety_err("no").to_ipc().starts_with("safety:protected::"));
        assert!(manage_err("protected", "svc")
            .to_ipc()
            .starts_with("manage:protected::"));
    }
}
