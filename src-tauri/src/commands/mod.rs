//! Tauri command handlers split out of `lib.rs` by domain.
//!
//! These are thin delegating wrappers: they validate the IPC shape, then call the owning domain
//! module. Anything that needs the process-wide statics in `lib.rs` (size-estimate cancellation,
//! pending right-click analyze) deliberately stayed there.

pub mod ai_cmd;
pub mod backup_cmd;
pub mod context_menu;
pub mod history_cmd;
pub mod ignore_cmd;
pub mod manage_cmd;
pub mod update;

pub use ai_cmd::*;
pub use backup_cmd::*;
pub use context_menu::*;
pub use history_cmd::*;
pub use ignore_cmd::*;
pub use manage_cmd::*;
pub use update::*;
