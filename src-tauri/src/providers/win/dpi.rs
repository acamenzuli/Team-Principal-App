//! DPI awareness verification.
//!
//! The manifest declares Per-Monitor V2 (see `app.manifest`). This module
//! *checks at runtime that it actually took effect*, because a Tauri upgrade,
//! a manifest that failed to embed, or a stray `SetProcessDpiAwareness` call
//! would otherwise silently drop us to system-DPI-aware — and every geometry
//! number in the app would then be scaled by an invisible factor.
//!
//! Cheap to check, catastrophic to miss, so it runs on every startup and is
//! asserted in a test.

use tp_model::Resolution;

/// What the process is actually running as, regardless of what the manifest says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Awareness {
    Unaware,
    SystemAware,
    PerMonitor,
    /// What we require.
    PerMonitorV2,
    UnawareGdiScaled,
    Unknown,
}

impl Awareness {
    /// Only V2 gives correct physical pixels across a mismatched multi-monitor
    /// setup. Anything else means the geometry math is being lied to.
    pub fn is_acceptable(self) -> bool {
        matches!(self, Awareness::PerMonitorV2)
    }
}

#[cfg(windows)]
pub fn current() -> Awareness {
    use windows::Win32::UI::HiDpi::*;

    // SAFETY: GetThreadDpiAwarenessContext returns a process-lifetime handle
    // and the comparison helpers are pure predicates over it.
    unsafe {
        let ctx = GetThreadDpiAwarenessContext();
        let is = |c: DPI_AWARENESS_CONTEXT| AreDpiAwarenessContextsEqual(ctx, c).as_bool();

        if is(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) {
            Awareness::PerMonitorV2
        } else if is(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE) {
            Awareness::PerMonitor
        } else if is(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE) {
            Awareness::SystemAware
        } else if is(DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED) {
            Awareness::UnawareGdiScaled
        } else if is(DPI_AWARENESS_CONTEXT_UNAWARE) {
            Awareness::Unaware
        } else {
            Awareness::Unknown
        }
    }
}

#[cfg(not(windows))]
pub fn current() -> Awareness {
    Awareness::Unknown
}

/// The full virtual desktop bounding box, in physical pixels.
///
/// With mismatched panel heights this box contains dead regions that map to no
/// panel — computing them is milestone 2's job, but the box itself is the
/// coordinate space every window rectangle lives in.
#[cfg(windows)]
pub fn virtual_desktop_size() -> Resolution {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    };
    // SAFETY: GetSystemMetrics is a pure read of a system value.
    unsafe {
        Resolution {
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(0) as u32,
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(0) as u32,
        }
    }
}

#[cfg(not(windows))]
pub fn virtual_desktop_size() -> Resolution {
    Resolution {
        width: 0,
        height: 0,
    }
}

/// Log the awareness state, loudly if it is wrong.
pub fn verify_and_log() -> Awareness {
    let a = current();
    if a.is_acceptable() {
        tracing::info!(awareness = ?a, "DPI awareness verified");
    } else if cfg!(windows) {
        tracing::error!(
            awareness = ?a,
            "process is NOT Per-Monitor V2 DPI aware — every geometry value is suspect. \
             Check that app.manifest was embedded."
        );
    }
    a
}

#[cfg(test)]
mod tests {
    /// Guards the manifest's *content*. Cheap, runs on every platform, and
    /// catches the most likely regression: someone editing app.manifest and
    /// dropping or misspelling the awareness declaration.
    ///
    /// It deliberately does NOT assert the running process is V2. The manifest
    /// is embedded into the app executable by build.rs; `cargo test` builds a
    /// separate harness binary that has no manifest, so such an assertion would
    /// always read `Unaware` and prove nothing. The real check on the real
    /// executable lives in `tests/dpi_manifest.rs`.
    #[test]
    fn manifest_declares_per_monitor_v2() {
        let manifest = include_str!("../../../app.manifest");
        assert!(
            manifest.contains("<dpiAwareness") && manifest.contains("PerMonitorV2"),
            "app.manifest no longer declares PerMonitorV2"
        );
        assert!(
            manifest.contains("true/pm"),
            "app.manifest dropped the 2005-namespace dpiAware; older loaders read only that one"
        );
        assert!(
            manifest.contains("longPathAware"),
            "app.manifest dropped longPathAware; deep Steam and UE5 config paths will fail"
        );
        assert!(
            manifest.contains("Microsoft.Windows.Common-Controls")
                && manifest.contains(r#"version="6.0.0.0""#),
            "app.manifest dropped the Common Controls v6 dependency. Without it the loader \
             binds comctl32 v5, TaskDialogIndirect is missing, and the app dies at load with \
             \"Entry Point Not Found\" before main runs."
        );
        // The resource compiler reads the manifest as a narrow string. A single
        // curly quote or em dash in a comment fails the build with
        // "Non-8-bit codepoint can't occur in a user-defined narrow string",
        // which is not an obvious message for what is really a typography slip.
        if let Some(c) = manifest.chars().find(|c| !c.is_ascii()) {
            panic!(
                "app.manifest contains the non-ASCII character {c:?} (U+{:04X}). \
                 The resource compiler cannot embed it; keep the manifest, comments \
                 included, to 7-bit ASCII.",
                c as u32
            );
        }
    }
}
