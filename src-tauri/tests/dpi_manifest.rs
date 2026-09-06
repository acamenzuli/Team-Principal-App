//! Verifies DPI awareness on the binary that actually ships.
//!
//! This cannot be a unit test. `build.rs` embeds `app.manifest` into the app
//! executable; `cargo test` builds a separate harness binary with no manifest,
//! so a unit test asserting the current process is Per-Monitor V2 would always
//! read `Unaware` — which is exactly what happened the first time CI ran this.
//!
//! So instead: run the real executable with `--check-dpi` and read what it
//! says about itself. If a Tauri upgrade, a build.rs change or a manifest edit
//! ever drops the declaration, this fails.

#![cfg(windows)]

use std::process::Command;

#[test]
fn the_app_executable_is_per_monitor_v2() {
    let exe = env!("CARGO_BIN_EXE_team-principal");
    let out = Command::new(exe)
        .arg("--check-dpi")
        .output()
        .expect("could not run the app executable");

    let reported = String::from_utf8_lossy(&out.stdout).trim().to_string();

    assert!(
        out.status.success(),
        "the shipped executable reports DPI awareness {reported:?}, not PerMonitorV2. \
         Every monitor rectangle and every geometry value it produces is scaled by an \
         invisible factor. Check that app.manifest is still embedded by build.rs."
    );
    assert_eq!(reported, "PerMonitorV2");
}
