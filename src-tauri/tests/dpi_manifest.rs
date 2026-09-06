//! Verifies that the DPI manifest is embedded in the binary that actually
//! ships.
//!
//! This cannot be a unit test. `build.rs` embeds `app.manifest` into the app
//! executable; `cargo test` builds a separate harness binary with no manifest,
//! so a unit test asserting the *current* process is Per-Monitor V2 would
//! always read `Unaware` — which is exactly what happened the first time CI
//! ran this.
//!
//! The assertion below inspects the built executable's bytes rather than
//! launching it. That is deliberate: launching a GUI application on a headless
//! CI runner tests the runner's environment as much as the app, and a failure
//! there tells you nothing about whether the manifest is present. The bytes do.
//!
//! Whether Windows then *honours* the manifest is a runtime question, answered
//! on the real machine: the app checks its own awareness on every start, logs
//! it, and shows a red banner in the UI if it is not V2. `--check-dpi` prints
//! the same answer for support purposes.

#![cfg(windows)]

use std::process::Command;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn the_app_executable_carries_the_dpi_manifest() {
    let exe = env!("CARGO_BIN_EXE_team-principal");
    let bytes = std::fs::read(exe).expect("could not read the built executable");

    // `longPathAware` and `<dpiAwareness` appear nowhere in the source, so
    // finding them in the binary can only mean the manifest resource is
    // present. (`PerMonitorV2` alone would be a false positive — it is also an
    // enum variant name, and derived Debug puts that string in the binary.)
    assert!(
        contains(&bytes, b"<dpiAwareness"),
        "no <dpiAwareness> element in {exe} — app.manifest is not embedded. \
         Check tauri_build::WindowsAttributes::app_manifest in build.rs."
    );
    assert!(
        contains(&bytes, b"PerMonitorV2"),
        "the embedded manifest does not request PerMonitorV2"
    );
    assert!(
        contains(&bytes, b"longPathAware"),
        "the embedded manifest dropped longPathAware — deep Steam and UE5 \
         config paths will fail"
    );
}

/// Diagnostic, not a gate.
///
/// Runs the real executable and reports what it says about its own DPI
/// awareness. It does not fail the build when the process cannot start,
/// because a headless CI runner is not a sim rig and a GUI binary failing to
/// launch there says nothing about the manifest. What it says when it *does*
/// run is printed, so `cargo test -- --nocapture` is a one-line answer to
/// "is this build actually Per-Monitor V2 on this machine".
#[test]
fn report_runtime_dpi_awareness() {
    let exe = env!("CARGO_BIN_EXE_team-principal");
    match Command::new(exe).arg("--check-dpi").output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            println!(
                "--check-dpi: status={:?} stdout={stdout:?} stderr={stderr:?}",
                out.status.code()
            );
            if !stdout.is_empty() {
                assert_eq!(
                    stdout, "PerMonitorV2",
                    "the executable ran and reported {stdout:?}. The manifest is embedded \
                     (the test above proves it), so something is overriding the awareness \
                     at startup."
                );
            }
        }
        Err(e) => println!("--check-dpi could not be launched here: {e}"),
    }
}
