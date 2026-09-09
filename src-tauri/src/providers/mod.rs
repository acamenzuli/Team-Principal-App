//! The four seams between the app and the operating system.
//!
//! Everything platform-specific lives behind one of these traits, and every
//! one has a fixture-driven mock. That is not for purity — it is because there
//! is no triple-monitor sim rig in CI, and hotplug sequences ("device arrives,
//! bounces twice, settles") cannot be tested by unplugging things by hand.
//!
//! Provider selection is a *runtime* flag rather than a cargo feature, so a
//! single shipped binary can run `--simulate` against fixtures for support
//! purposes.

pub mod mock;
#[cfg(windows)]
pub mod win;

use tp_model::{DetectedDevice, MonitorInfo, TopologySnapshot};

use crate::error::AppResult;

/// Read-only in milestone 2; topology changes arrive in milestone 9 behind the
/// confirm-or-revert flow.
pub trait DisplayProvider: Send + Sync {
    fn enumerate(&self) -> AppResult<Vec<MonitorInfo>>;
    fn capture_snapshot(&self) -> AppResult<TopologySnapshot>;

    /// The virtual desktop bounding box and the dead regions inside it.
    ///
    /// Provided by the trait rather than by each implementation: it is pure
    /// math over the enumerated rectangles, so the real and mock providers must
    /// not be able to disagree about it.
    fn desktop_layout(&self) -> AppResult<Option<tp_geometry::DesktopLayout>> {
        let monitors = self.enumerate()?;
        let rects: Vec<_> = monitors.iter().map(|m| m.bounds).collect();
        Ok(tp_geometry::desktop_layout(&rects))
    }
}

pub trait PeripheralProvider: Send + Sync {
    fn enumerate(&self) -> AppResult<Vec<DetectedDevice>>;
}

// There were WindowProvider and ProcessProvider traits here. They were seams
// designed in milestone 1 for code that had not been written, and by the time
// it was, the real implementations had no use for them: window control needs a
// window handle, a style, a rectangle and a read-back, not a "find me an exe"
// call, and process checks are one function in `launcher::gates`. Nothing ever
// called either trait, and both were still returning "not implemented yet" for
// milestones that had shipped. A seam nobody uses is a claim about the
// architecture that is not true, so they are gone.

/// The set of providers the app is running against. Held in Tauri state.
pub struct Providers {
    pub display: Box<dyn DisplayProvider>,
    pub peripherals: Box<dyn PeripheralProvider>,
    /// True when running against fixtures. Surfaced in the UI so a screenshot
    /// from a support ticket can never be mistaken for real hardware.
    pub simulated: bool,
}

impl Providers {
    /// Pick real or mock providers. `fixture` is the `--mock <path>` argument.
    pub fn select(fixture: Option<&std::path::Path>) -> AppResult<Providers> {
        if let Some(path) = fixture {
            tracing::warn!(fixture = %path.display(), "running against fixtures, not real hardware");
            return mock::from_fixture(path);
        }
        #[cfg(windows)]
        {
            Ok(Providers {
                display: Box::new(win::WinDisplayProvider::new()),
                peripherals: Box::new(win::WinPeripheralProvider::new()),
                simulated: false,
            })
        }
        #[cfg(not(windows))]
        {
            // Developing the UI on a non-Windows machine is legitimate; running
            // the real providers there is not. Fail loudly rather than
            // pretending to detect hardware.
            Err(crate::error::AppError::Config(
                "real providers require Windows; pass --mock <fixture.json>".into(),
            ))
        }
    }
}
