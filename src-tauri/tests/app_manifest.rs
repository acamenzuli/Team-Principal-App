//! The shipped executable's manifest, and that it can actually start.
//!
//! Both checks exist because of real failures.
//!
//! Supplying a custom `app.manifest` replaces Tauri's default one **wholesale**.
//! The default carries a dependency on Common Controls v6; mine did not, so the
//! loader bound comctl32 v5 from system32, `TaskDialogIndirect` was missing, and
//! the installed app died with "Entry Point Not Found" before `main` ran — no
//! window, no log, no stdout to explain it.
//!
//! CI *did* catch that. The launch check failed with an empty stdout and a
//! non-zero exit, and I misread it as a headless runner refusing to start a GUI
//! binary, then demoted the check to a diagnostic. The check was right. It is a
//! gate again.

#![cfg(windows)]

use std::process::Command;

fn built_exe() -> &'static str {
    env!("CARGO_BIN_EXE_team-principal")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Inspects the binary's bytes rather than launching it, so a manifest that is
/// missing entirely is distinguishable from one that is present but wrong.
#[test]
fn the_executable_carries_the_expected_manifest() {
    let bytes = std::fs::read(built_exe()).expect("could not read the built executable");

    // `<dpiAwareness` and `longPathAware` appear nowhere in the source, so
    // finding them can only mean the manifest resource is embedded.
    // (`PerMonitorV2` alone would false-positive: derived Debug puts that enum
    // variant name in the binary too.)
    for needle in [
        &b"<dpiAwareness"[..],
        b"PerMonitorV2",
        b"longPathAware",
        b"Microsoft.Windows.Common-Controls",
    ] {
        assert!(
            contains(&bytes, needle),
            "{:?} is missing from the embedded manifest. A custom app.manifest replaces \
             Tauri's default wholesale, so anything the default provided has to be \
             restated in ours.",
            String::from_utf8_lossy(needle)
        );
    }
}

/// The check that actually caught the Common Controls bug.
///
/// A manifest can be present and still leave the binary unable to load — a
/// missing dependency fails at import resolution, before any code runs. Only
/// starting the process proves otherwise.
///
/// `--check-dpi` exits before Tauri builds a window, so this needs no display
/// and works on a headless runner.
#[test]
fn the_executable_starts() {
    let out = Command::new(built_exe())
        .arg("--check-dpi")
        .output()
        .expect("could not run the app executable");

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();

    // Report everything on failure. The first version of this test discarded
    // the exit code and stderr, which is why a load failure was misdiagnosed as
    // a wrong DPI awareness.
    assert!(
        !stdout.is_empty(),
        "the executable produced no output and exited with {:?}. It did not reach main — \
         almost certainly a loader failure such as a missing manifest dependency. stderr: {stderr:?}",
        out.status.code()
    );
    assert_eq!(
        stdout,
        "PerMonitorV2",
        "the executable started but reports DPI awareness {stdout:?}. exit={:?} stderr={stderr:?}",
        out.status.code()
    );
    assert!(
        out.status.success(),
        "exit={:?} stderr={stderr:?}",
        out.status.code()
    );
}
