//! Geometry — the core of Team Principal.
//!
//! Everything here is pure math over a rig description. No Win32, no Tauri, no
//! knowledge of any specific game. Adapters consume this crate's output; they
//! never do their own trigonometry.
//!
//! Milestone 1 ships the foundations that other milestones build on: unit
//! handling and the curvature model. The FOV solver, per-screen projection,
//! bezel math and best-fit solver land in milestone 3.

pub mod curvature;
pub mod parse;

pub use curvature::{solve as solve_curvature, CurveSolution};
pub use parse::{format_length, parse_length, ParseLengthError};
