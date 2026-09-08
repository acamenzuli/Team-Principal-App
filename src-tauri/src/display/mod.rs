//! Display enumeration.
//!
//! `enumerate` reports what the displays are. `apply` changes them, and
//! `confirm` is what makes changing them survivable: every change is captured
//! first, verified by read-back, and reverted automatically unless the user
//! says otherwise — because a bad change on a triple rig can leave them with no
//! visible screen and no way to click anything.

pub mod apply;
pub mod confirm;
pub mod edid_source;
pub mod enumerate;
pub mod hotkey;

pub use enumerate::{capture_snapshot, enumerate_monitors};
