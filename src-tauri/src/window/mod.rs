//! Window control — the SRWE replacement.
//!
//! Find the game's window, strip its chrome, put it exactly where the rig model
//! says, and keep it there.
//!
//! ## Everything is verified by read-back
//!
//! `SetWindowLongPtrW` and `SetWindowPos` against a window owned by a
//! higher-integrity process are refused by User Interface Privilege Isolation,
//! and the refusal is *quiet*: `SetWindowLongPtrW` returns the previous style
//! on success and zero on failure, and zero is also a legitimate previous
//! style. There is no return value that means "this worked".
//!
//! So nothing here trusts a return value. Every change is read back and
//! compared with what was asked for, and a mismatch is reported as a specific,
//! actionable failure — usually "this game runs elevated, so Team Principal
//! has to as well".

pub mod apply;
pub mod find;
pub mod watchdog;

pub use apply::{apply_geometry, read_geometry, AppliedWindow};
pub use find::{enumerate_windows, find_target};
pub use watchdog::{Watchdog, WatchdogHandle};
