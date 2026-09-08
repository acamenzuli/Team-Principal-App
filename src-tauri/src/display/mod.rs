//! Display enumeration.
//!
//! Read-only in milestone 2: this module reports what the displays are and
//! never changes them. Topology and mode changes arrive in milestone 9, behind
//! the confirm-or-revert countdown and the panic hotkey, because a bad change
//! on a triple rig can leave the user with no visible screen and no way to
//! click anything.

pub mod edid_source;
pub mod enumerate;

pub use enumerate::{capture_snapshot, enumerate_monitors};
