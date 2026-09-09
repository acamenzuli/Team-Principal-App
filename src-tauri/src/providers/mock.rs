//! Fixture-driven mocks.
//!
//! A fixture is a JSON file describing a rig and its peripherals. The
//! interesting ones are not the happy paths — see `fixtures/` for mismatched
//! panel heights with dead zones, mixed DPI scaling, and hotplug sequences
//! where a device bounces twice before settling.

use std::path::Path;

use tp_model::{DetectedDevice, Fixture, MonitorInfo, TopologySnapshot};

use super::{DisplayProvider, PeripheralProvider, Providers};
use crate::error::{AppError, AppResult};

pub fn from_fixture(path: &Path) -> AppResult<Providers> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| AppError::Config(format!("could not read fixture {}: {e}", path.display())))?;
    let fixture: Fixture = serde_json::from_str(&raw)
        .map_err(|e| AppError::Config(format!("fixture {} is not valid: {e}", path.display())))?;

    tracing::info!(fixture = %fixture.name, monitors = fixture.monitors.len(),
                   devices = fixture.devices.len(), "loaded fixture: {}", fixture.description);

    Ok(Providers {
        display: Box::new(MockDisplayProvider {
            fixture: fixture.clone(),
        }),
        peripherals: Box::new(MockPeripheralProvider {
            fixture: fixture.clone(),
        }),
        simulated: true,
    })
}

pub struct MockDisplayProvider {
    fixture: Fixture,
}

impl DisplayProvider for MockDisplayProvider {
    fn enumerate(&self) -> AppResult<Vec<MonitorInfo>> {
        Ok(self.fixture.monitors.clone())
    }

    fn capture_snapshot(&self) -> AppResult<TopologySnapshot> {
        use tp_model::SnapshotEntry;
        Ok(TopologySnapshot {
            id: uuid::Uuid::new_v4(),
            captured_at: crate::now_iso8601(),
            monitors: self
                .fixture
                .monitors
                .iter()
                .map(|m| SnapshotEntry {
                    device_path: m.device_path.clone(),
                    active: true,
                    mode: m.current_mode,
                    position: (m.bounds.x, m.bounds.y),
                    is_primary: m.is_primary,
                })
                .collect(),
        })
    }
}

pub struct MockPeripheralProvider {
    fixture: Fixture,
}

impl PeripheralProvider for MockPeripheralProvider {
    fn enumerate(&self) -> AppResult<Vec<DetectedDevice>> {
        Ok(self.fixture.devices.clone())
    }
}
