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
//! The frontend subscribes to `peripherals://changed`. It never polls.

use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tp_model::{Catalog, Debouncer, DetectedDevice};

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

    // Device arrival and removal, from the OS rather than from a timer.
    #[cfg(windows)]
    let _notification = super::notify::register(wake.clone());

    std::thread::Builder::new()
        .name("peripheral-watch".into())
        .spawn(move || {
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
                let (published, changed) = debounce(&mut debouncers, scanned, now_ms);

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

/// Run each device's status through its debouncer, and report whether anything
/// the user can see actually changed.
///
/// Split out from the thread so the interesting behaviour — a bounce not
/// reaching the UI, a device disappearing, a new device appearing — is testable
/// without threads, timers or hardware.
fn debounce(
    debouncers: &mut HashMap<String, Debouncer>,
    scanned: Vec<DetectedDevice>,
    now_ms: u64,
) -> (Vec<DetectedDevice>, bool) {
    let mut changed = false;
    let mut seen: Vec<String> = Vec::with_capacity(scanned.len());

    let published: Vec<DetectedDevice> = scanned
        .into_iter()
        .map(|mut d| {
            let key = device_key(&d);
            seen.push(key.clone());

            match debouncers.get_mut(&key) {
                Some(debouncer) => {
                    if debouncer.observe(d.status, now_ms).is_some() {
                        changed = true;
                    }
                    // Show the settled status, not the instantaneous one.
                    d.status = debouncer.status();
                }
                None => {
                    // First sighting: publish immediately. Debouncing a device
                    // the user just plugged in would make the app feel slow at
                    // exactly the moment they are watching it.
                    debouncers.insert(key, Debouncer::new(d.status));
                    changed = true;
                }
            }
            d
        })
        .collect();

    // A device that stopped being reported is gone; drop its debouncer so a
    // later reconnect starts clean rather than inheriting a stale window.
    let before = debouncers.len();
    debouncers.retain(|k, _| seen.contains(k));
    if debouncers.len() != before {
        changed = true;
    }

    (published, changed)
}

/// Identity for tracking a device across scans.
///
/// The instance path distinguishes two otherwise identical devices, which
/// VID/PID alone cannot — and two identical un-serialled pedal sets is a real
/// configuration, not a hypothetical one.
fn device_key(d: &DetectedDevice) -> String {
    d.device
        .instance_path
        .clone()
        .unwrap_or_else(|| format!("{:04X}:{:04X}", d.device.vid, d.device.pid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_model::{DeviceRef, DeviceStatus};

    fn device(path: &str, status: DeviceStatus) -> DetectedDevice {
        DetectedDevice {
            device: DeviceRef {
                vid: 0x0EB7,
                pid: 0x0E04,
                serial: None,
                instance_path: Some(path.into()),
                display_name: "Test wheel".into(),
            },
            manufacturer: None,
            raw_product_name: None,
            status,
            hid_present: status != DeviceStatus::Disconnected,
            dinput_present: status == DeviceStatus::Connected,
            dinput_slot: None,
            dinput_instance_guid: None,
            is_virtual: false,
            vjoy: None,
            binding_drift: None,
            alias_key: tp_model::alias_key(0x0EB7, 0x0E04, None, Some(path)),
            renamed: false,
        }
    }

    #[test]
    fn a_new_device_is_published_at_once() {
        // Debouncing something the user just plugged in would make the app feel
        // slow at exactly the moment they are looking at it.
        let mut d = HashMap::new();
        let (published, changed) = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0);
        assert!(changed);
        assert_eq!(published[0].status, DeviceStatus::Connected);
    }

    #[test]
    fn a_bounce_does_not_reach_the_ui() {
        let mut d = HashMap::new();
        debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0);

        // Drops and returns well inside the settle window.
        let (published, changed) =
            debounce(&mut d, vec![device("a", DeviceStatus::Disconnected)], 100);
        assert!(!changed, "a 100 ms dropout is a bounce, not an event");
        assert_eq!(
            published[0].status,
            DeviceStatus::Connected,
            "still shown as connected"
        );

        let (_, changed) = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 300);
        assert!(!changed);
    }

    #[test]
    fn a_real_disconnection_does_reach_the_ui() {
        let mut d = HashMap::new();
        debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0);
        debounce(&mut d, vec![device("a", DeviceStatus::Disconnected)], 100);
        let (published, changed) =
            debounce(&mut d, vec![device("a", DeviceStatus::Disconnected)], 1000);
        assert!(changed);
        assert_eq!(published[0].status, DeviceStatus::Disconnected);
    }

    #[test]
    fn a_device_that_vanishes_from_the_scan_clears_its_state() {
        // Otherwise a reconnect inherits a half-elapsed settle window and the
        // first status after replugging is decided by the old one.
        let mut d = HashMap::new();
        debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0);
        assert_eq!(d.len(), 1);

        let (published, changed) = debounce(&mut d, vec![], 1000);
        assert!(changed, "a device disappearing is a change");
        assert!(published.is_empty());
        assert!(d.is_empty(), "its debouncer went with it");
    }

    #[test]
    fn identical_devices_are_tracked_separately() {
        // Two of the same un-serialled pedal set is a real configuration. Keyed
        // by VID/PID alone they would share one debouncer and one status.
        let mut d = HashMap::new();
        debounce(
            &mut d,
            vec![
                device("port-a", DeviceStatus::Connected),
                device("port-b", DeviceStatus::Connecting),
            ],
            0,
        );
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn a_steady_state_publishes_nothing() {
        // The UI should not be woken four times a second to be told nothing
        // happened.
        let mut d = HashMap::new();
        debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0);
        let (_, changed) = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 4000);
        assert!(!changed);
    }
}
