//! Fixture format for the mock providers.
//!
//! Lives in `tp-model` rather than in the Tauri crate so the fixture files can
//! be deserialised against the real types by a test that runs on any platform.
//! A fixture that has silently drifted from the model is worse than no fixture,
//! because it makes CI green while the app is broken.

use serde::{Deserialize, Serialize};

use crate::{DetectedDevice, MonitorInfo};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fixture {
    pub name: String,
    /// What this fixture is for. Read by `--simulate` and printed in the UI
    /// banner, so a support screenshot says which scenario produced it.
    pub description: String,
    #[serde(default)]
    pub monitors: Vec<MonitorInfo>,
    #[serde(default)]
    pub devices: Vec<DetectedDevice>,
    /// Executables the mock should claim are already running, so the
    /// "Already running" path is testable without installing SimHub.
    #[serde(default)]
    pub running_processes: Vec<String>,
}
