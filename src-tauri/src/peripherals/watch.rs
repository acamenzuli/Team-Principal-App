//! Live peripheral status.
//!
//! A background thread owns the device list. It rescans when the OS says a HID
//! interface arrived or departed, and on a slow reconciliation timer for the
//! things no event covers — a vendor process starting, a vJoy feeder dying,
//! DirectInput ordering settling after a hotplug.
//!
//! Every device's status goes through a [`Debouncer`] before it reaches the UI,
//! because USB devices bounce while they enumerate and an undebounced list
//! flickers between states and looks broken when nothing is wrong.
//!
//! The rule for what gets published lives in `tp_model::presence`, not here.
//! It is pure — a list in, a list out — and it sat in this crate until a
//! regression in it could only be caught by CI, because nothing in `src-tauri`
//! can be tested off Windows.
//!
//! The frontend subscribes to `peripherals://changed`. It never polls.

use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tp_model::{debounce, Catalog, Debouncer, DetectedDevice};

/// Managed state.
///
/// `None` when running against fixtures, where there is no hardware to watch
/// and the mock provider answers instead. Wrapped rather than managed as an
/// `Option<Watcher>` because a Tauri command's `State` must name a concrete
/// type.
pub struct PeripheralWatch(pub Option<Watcher>);

/// The event the frontend listens for.
pub const EVENT: &str = "peripherals://changed";

/// How often to reconcile things that emit no event.
const RECONCILE: Duration = Duration::from_secs(4);

#[derive(Clone)]
pub struct Watcher {
    /// The last published list, so a newly opened window gets the current state
    /// immediately rather than waiting for the next scan.
    latest: Arc<Mutex<Vec<DetectedDevice>>>,
    wake: Sender<()>,
}

impl Watcher {
    pub fn latest(&self) -> Vec<DetectedDevice> {
        self.latest.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Ask for a rescan now — used by an explicit Refresh in the UI.
    pub fn poke(&self) {
        let _ = self.wake.send(());
    }
}

/// Start watching. Returns immediately; the thread runs for the app's lifetime.
pub fn start(app: AppHandle) -> Watcher {
    let (wake, wakes) = mpsc::channel::<()>();
    let latest = Arc::new(Mutex::new(Vec::new()));
    let watcher = Watcher {
        latest: latest.clone(),
        wake: wake.clone(),
    };

    std::thread::Builder::new()
        .name("peripheral-watch".into())
        .spawn(move || {
            // Device arrival and removal, from the OS rather than from a timer.
            //
            // Registered *inside* the thread and held for its lifetime. It used
            // to be a local in this function, which returns immediately — so
            // the registration was dropped, and dropping it unregisters. Every
            // hotplug notification since has gone nowhere, leaving the
            // four-second reconcile as the only thing that ever noticed a
            // device being unplugged.
            #[cfg(windows)]
            let _notification = super::notify::register(wake.clone());
            let catalog = Catalog::seeded();
            let mut debouncers: HashMap<String, Debouncer> = HashMap::new();
            let started = Instant::now();

            loop {
                // Re-read per scan rather than captured once: renaming a
                // device has to show up on the next refresh, not the next
                // restart. The file is a few hundred bytes and this loop runs
                // on a hotplug or a slow reconcile, not in a hot path.
                let aliases = crate::settings::load().0.device_aliases;
                let scanned = super::enumerate(&catalog, &aliases).unwrap_or_default();
                let now_ms = started.elapsed().as_millis() as u64;
                let previous = latest.lock().map(|g| g.clone()).unwrap_or_default();
                let published = debounce(&mut debouncers, scanned, now_ms, &previous);

                // Publish when what the user would see differs from what they
                // last saw. The rule used to be "when a status changed", which
                // enumerated some of the reasons a row can differ and missed
                // the rest: renaming a device changes its name and nothing
                // else, so the new name sat in the file, correctly saved, and
                // never reached the screen until something was unplugged.
                let changed = match latest.lock() {
                    Ok(guard) => *guard != published,
                    // Poisoned: publish rather than go quiet. A stuck device
                    // list is worse than an extra event.
                    Err(_) => true,
                };

                if changed {
                    if let Ok(mut guard) = latest.lock() {
                        *guard = published.clone();
                    }
                    if let Err(e) = app.emit(EVENT, &published) {
                        tracing::warn!(error = %e, "could not publish peripheral change");
                    }
                }

                // Wake early on a hotplug notification, otherwise reconcile.
                match wakes.recv_timeout(RECONCILE) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            tracing::info!("peripheral watch stopped");
        })
        .expect("could not start the peripheral watch thread");

    watcher
}
