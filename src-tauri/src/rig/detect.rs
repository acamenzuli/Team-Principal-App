//! Building a starter rig from the monitors actually plugged in.
//!
//! This is the difference between a product and a form. Everything Windows and
//! EDID can answer is filled in — which monitors exist, how they are arranged,
//! their native resolution, their physical size in millimetres. What is left is
//! only what no API can know: bezel thickness, screen angle, and how far back
//! the driver sits.
//!
//! The mapping from detected monitors to rig screens is pure logic, so it lives
//! in `tp-model` where it is tested on any platform. This module is only the
//! glue to the provider.

use tp_model::{MonitorInfo, RigModel};

/// Build a rig from detected monitors, keeping any measurements the user has
/// already entered for the same physical panels.
pub fn from_monitors(monitors: &[MonitorInfo], existing: Option<&RigModel>) -> RigModel {
    tp_model::rig_from_monitors(monitors, existing, &crate::now_iso8601())
}
