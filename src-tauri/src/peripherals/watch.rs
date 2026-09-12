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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tp_model::{debounce, transitions, Catalog, Debouncer, DetectedDevice, DeviceEvent};

/// How much history to keep per device.
///
/// A cable that drops every few minutes produces a long list and the recent
/// end is the useful one. Two hundred entries is days of ordinary use and
/// minutes of a genuinely faulty cable — which is exactly when somebody looks.
const HISTORY: usize = 200;

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
    /// Set when a person asked for the scan, as opposed to the timer or a
    /// hotplug notification.
    asked: Arc<AtomicBool>,
    /// What has happened to each device, newest last, keyed as `device_key`.
    history: Arc<Mutex<HashMap<String, Vec<DeviceEvent>>>>,
}

impl Watcher {
    pub fn latest(&self) -> Vec<DetectedDevice> {
        self.latest.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// What has happened to one device since the app started.
    pub fn history(&self, key: &str) -> Vec<DeviceEvent> {
        self.history
            .lock()
            .map(|h| h.get(key).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Ask for a rescan now — used by an explicit Rescan in the UI.
    ///
    /// Flagged as well as woken. Scans publish only when the list differs,
    /// which is right for a timer and wrong for a button: a rescan that finds
    /// exactly what it found last time is a correct answer, and a button that
    /// produces no answer at all is indistinguishable from a broken one.
    pub fn poke(&self) {
        self.asked.store(true, Ordering::Relaxed);
        let _ = self.wake.send(());
    }
}

/// Start watching. Returns immediately; the thread runs for the app's lifetime.
pub fn start(app: AppHandle) -> Watcher {
    let (wake, wakes) = mpsc::channel::<()>();
    let latest = Arc::new(Mutex::new(Vec::new()));
    let asked = Arc::new(AtomicBool::new(false));
    let history: Arc<Mutex<HashMap<String, Vec<DeviceEvent>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let watcher = Watcher {
        latest: latest.clone(),
        wake: wake.clone(),
        asked: asked.clone(),
        history: history.clone(),
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
                // Somebody pressed Rescan: answer them, changed or not.
                let requested = asked.swap(false, Ordering::Relaxed);
                let changed = requested
                    || match latest.lock() {
                        Ok(guard) => *guard != published,
                        // Poisoned: publish rather than go quiet. A stuck
                        // device list is worse than an extra event.
                        Err(_) => true,
                    };

                // Recorded from what was *published*, so the history says what
                // the person saw: a bounce the debouncer swallowed never
                // reached the screen and does not belong in a record of what
                // happened.
                for (key, event) in transitions(&previous, &published, &crate::now_iso8601()) {
                    tracing::info!(
                        device = %key,
                        from = ?event.from,
                        to = ?event.to,
                        "device status changed"
                    );
                    if let Ok(mut log) = history.lock() {
                        let entries = log.entry(key).or_default();
                        entries.push(event);
                        if entries.len() > HISTORY {
                            entries.remove(0);
                        }
                    }
                }

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
