//! Launch orchestration.
//!
//! A profile is a dependency graph, not a list. The scheduling rules live in
//! `tp_model::graph` where they are tested; this module runs what that says is
//! runnable, checks readiness gates, and tears everything down afterwards.

pub mod discovery;
pub mod gates;
pub mod job;
pub mod process;
pub mod run;

pub use discovery::{discover, launch_uri, GameSource, InstalledGame};
