//! Shared model types for Team Principal.
//!
//! This crate is the IPC contract between the Rust backend and the React
//! frontend. It deliberately depends on neither `windows` nor `tauri`, which
//! means:
//!
//! * the TypeScript generator is a fast standalone build;
//! * these types compile and test on any platform, so CI can check them
//!   without a Windows runner;
//! * nothing platform-specific can leak into the wire format by accident.

pub mod detect;
pub mod device;
pub mod display;
pub mod fixture;
pub mod ipc;
pub mod preferences;
pub mod profile;
pub mod rig;
pub mod units;

pub use detect::rig_from_monitors;
pub use device::*;
pub use display::*;
pub use fixture::Fixture;
pub use ipc::*;
pub use preferences::*;
pub use profile::*;
pub use rig::*;
pub use units::*;
