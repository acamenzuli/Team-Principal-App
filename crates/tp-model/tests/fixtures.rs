//! Every fixture must deserialise against the real model types.
//!
//! A fixture that has silently drifted from the schema is worse than no
//! fixture: it makes CI green while the app is broken. `deny_unknown_fields` on
//! `Fixture` means a renamed field fails here rather than being ignored.

use std::path::{Path, PathBuf};

use tp_model::Fixture;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rigs")
}

fn load_all() -> Vec<(String, Fixture)> {
    let dir = fixture_dir();
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("fixtures/rigs must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let raw = std::fs::read_to_string(&path).expect("readable fixture");
        let parsed: Fixture = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("{name} does not match the model: {e}"));
        out.push((name, parsed));
    }
    out
}

#[test]
fn every_fixture_matches_the_model() {
    let all = load_all();
    assert!(
        !all.is_empty(),
        "no fixtures found — the mock providers have nothing to run against"
    );
    for (name, f) in &all {
        assert!(
            !f.description.trim().is_empty(),
            "{name} has no description"
        );
        assert!(
            !f.monitors.is_empty(),
            "{name} has no monitors; a rig fixture with no displays tests nothing"
        );
    }
}

#[test]
fn exactly_one_primary_monitor_per_fixture() {
    // Windows always has exactly one primary. A fixture with none or two would
    // exercise a state the real provider can never produce.
    for (name, f) in load_all() {
        let primaries = f.monitors.iter().filter(|m| m.is_primary).count();
        assert_eq!(primaries, 1, "{name} has {primaries} primary monitors");
    }
}

#[test]
fn monitor_identities_are_unique_within_a_fixture() {
    // Binding is by EDID identity, so two monitors that are indistinguishable
    // would make the binding ambiguous — which is a real situation, but one
    // the app must be told about explicitly rather than meeting by accident.
    for (name, f) in load_all() {
        let mut seen = std::collections::HashSet::new();
        for m in &f.monitors {
            let key = (
                m.identity.manufacturer_id.clone(),
                m.identity.product_code,
                m.identity.serial.clone(),
                m.identity.serial_number,
            );
            assert!(
                seen.insert(key),
                "{name}: two monitors share an EDID identity"
            );
        }
    }
}

#[test]
fn the_mismatched_fixture_actually_mismatches() {
    // This fixture exists to exercise dead regions. If someone "tidies" it into
    // a clean baseline it stops testing anything, so assert its whole point.
    let (_, f) = load_all()
        .into_iter()
        .find(|(n, _)| n == "mismatched-heights.json")
        .expect("mismatched-heights.json");

    let tops: std::collections::HashSet<i32> = f.monitors.iter().map(|m| m.bounds.y).collect();
    assert!(
        tops.len() > 1,
        "panels all sit on the same baseline — no dead regions to find"
    );

    let pitches: Vec<f64> = f
        .monitors
        .iter()
        .filter_map(|m| {
            m.physical_size
                .map(|p| m.native_resolution.width as f64 / p.width.0)
        })
        .collect();
    let min = pitches.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = pitches.iter().cloned().fold(0.0, f64::max);
    assert!(
        (max - min) / min > 0.10,
        "pixel pitch varies by only {:.1}% — this fixture should trip the PPI warning",
        100.0 * (max - min) / min
    );
}

#[test]
fn the_mixed_dpi_fixture_actually_mixes_dpi() {
    let (_, f) = load_all()
        .into_iter()
        .find(|(n, _)| n == "mixed-dpi.json")
        .expect("mixed-dpi.json");
    let scales: std::collections::HashSet<u32> = f
        .monitors
        .iter()
        .map(|m| (m.dpi_scale * 100.0) as u32)
        .collect();
    assert!(
        scales.len() > 1,
        "every monitor is at the same scale — nothing is being tested"
    );
}

#[test]
fn the_peripheral_fixture_covers_every_status() {
    use tp_model::DeviceStatus::*;
    let (_, f) = load_all()
        .into_iter()
        .find(|(n, _)| n == "hotplug-and-drift.json")
        .expect("hotplug-and-drift.json");

    for want in [Connected, Connecting, Disconnected] {
        assert!(
            f.devices.iter().any(|d| d.status == want),
            "no device in state {want:?} — the three-state UI is not fully exercised"
        );
    }
    assert!(
        f.devices.iter().any(|d| d.binding_drift.is_some()),
        "no device with DirectInput drift — the badge that makes this product worth money"
    );
    assert!(
        f.devices
            .iter()
            .any(|d| d.vjoy.as_ref().is_some_and(|v| !v.feeder_running)),
        "no vJoy device with a dead feeder"
    );
}
