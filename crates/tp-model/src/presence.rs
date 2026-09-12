//! Deciding what a peripheral's status actually is, and keeping it steady.
//!
//! On a sim rig "plugged in" and "ready to race" are different things, and
//! collapsing them into a boolean is why other tools mislead you. A vJoy device
//! whose feeder is not running enumerates perfectly and reads dead centre. A
//! wheelbase can be present at the HID layer for a second or two before
//! DirectInput lists it, which is exactly when a game would bind to the wrong
//! device.
//!
//! Both problems are decided here, in pure logic, so the rules are testable
//! without unplugging anything.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use std::collections::HashMap;

use crate::{BindingDrift, DetectedDevice, DeviceStatus};

/// Everything observed about one device at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    /// Ground truth for "is this physically plugged in".
    pub hid_present: bool,
    /// Whether DirectInput lists it — i.e. whether games can see it at all.
    pub dinput_present: bool,
    /// `None` when the device declares no vendor requirement.
    pub vendor_process_running: Option<bool>,
    /// `None` when the device is not a vJoy target.
    pub vjoy_feeder_running: Option<bool>,
}

impl Observation {
    /// The status a user should be shown.
    ///
    /// `Connecting` is not a loading spinner. It is the honest answer for a
    /// device that is present but cannot yet be raced with, and it covers four
    /// distinct real situations: still enumerating after a hotplug; visible to
    /// Windows but not yet to DirectInput; a vJoy target whose feeder is dead;
    /// and a peripheral whose vendor software has not started.
    pub fn status(self) -> DeviceStatus {
        if !self.hid_present {
            return DeviceStatus::Disconnected;
        }
        let ready = self.dinput_present
            && self.vendor_process_running.unwrap_or(true)
            && self.vjoy_feeder_running.unwrap_or(true);
        if ready {
            DeviceStatus::Connected
        } else {
            DeviceStatus::Connecting
        }
    }

    /// Why it is not ready, in words, for the row's detail line.
    pub fn reason(self) -> Option<&'static str> {
        if !self.hid_present {
            return Some("Not detected. Check the USB cable — it'll connect automatically.");
        }
        if !self.dinput_present {
            return Some("Plugged in, but games can't see it yet. Usually a few seconds.");
        }
        if self.vjoy_feeder_running == Some(false) {
            return Some("vJoy device with nothing feeding it — its axes will read dead centre.");
        }
        if self.vendor_process_running == Some(false) {
            return Some("Its vendor software isn't running, so not all of it will work.");
        }
        None
    }
}

/// Detect that a device's DirectInput identity has moved since a profile was
/// saved.
///
/// This is the failure that ruins a race and gives no clue why: the device
/// works perfectly, but the game's saved bindings now point at a different one,
/// so the shifter is the handbrake. Naming it before launch is most of the
/// value of the peripherals section.
pub fn detect_drift(
    expected_slot: Option<u32>,
    expected_guid: Option<&str>,
    actual_slot: Option<u32>,
    actual_guid: Option<&str>,
) -> Option<BindingDrift> {
    // Nothing was recorded to compare against: a profile saved before slots
    // were tracked, not evidence that anything moved.
    if expected_slot.is_none() && expected_guid.is_none() {
        return None;
    }
    let slot_moved = matches!((expected_slot, actual_slot), (Some(a), Some(b)) if a != b);
    // A GUID is the stronger signal — a slot can shift merely because another
    // device was unplugged, whereas the instance GUID identifies the device.
    let guid_moved = matches!((expected_guid, actual_guid), (Some(a), Some(b)) if a != b);

    (slot_moved || guid_moved).then(|| BindingDrift {
        expected_slot,
        actual_slot,
        expected_instance_guid: expected_guid.map(str::to_string),
        actual_instance_guid: actual_guid.map(str::to_string),
    })
}

/// Debounce for hotplug.
///
/// USB devices bounce while they enumerate: present, gone, present again,
/// sometimes twice, within a few hundred milliseconds. An undebounced UI
/// flickers between states and looks broken when nothing is wrong.
///
/// The clock is passed in rather than read, so a bounce sequence is a unit test
/// instead of somebody unplugging a wheel forty times.
#[derive(Debug, Clone)]
pub struct Debouncer {
    settle_ms: u64,
    /// The status currently being shown.
    stable: DeviceStatus,
    /// A candidate waiting out the settle window, and when it was first seen.
    pending: Option<(DeviceStatus, u64)>,
}

impl Debouncer {
    pub const DEFAULT_SETTLE_MS: u64 = 750;

    pub fn new(initial: DeviceStatus) -> Self {
        Self {
            settle_ms: Self::DEFAULT_SETTLE_MS,
            stable: initial,
            pending: None,
        }
    }

    pub fn with_settle_ms(initial: DeviceStatus, settle_ms: u64) -> Self {
        Self {
            settle_ms,
            stable: initial,
            pending: None,
        }
    }

    pub fn status(&self) -> DeviceStatus {
        self.stable
    }

    /// Feed an observation. Returns the new status only when it actually
    /// changed, so callers can log and emit an event on transitions alone.
    pub fn observe(&mut self, observed: DeviceStatus, now_ms: u64) -> Option<DeviceStatus> {
        if observed == self.stable {
            // Back to where we already were: whatever was pending was a bounce.
            self.pending = None;
            return None;
        }

        match self.pending {
            // A different candidate: restart the window rather than letting an
            // earlier one carry its elapsed time over.
            Some((candidate, _)) if candidate != observed => {
                self.pending = Some((observed, now_ms));
                None
            }
            Some((candidate, since)) if now_ms.saturating_sub(since) >= self.settle_ms => {
                self.stable = candidate;
                self.pending = None;
                Some(candidate)
            }
            Some(_) => None,
            None => {
                self.pending = Some((observed, now_ms));
                None
            }
        }
    }
}

// ------------------------------------------------------------ publishing

/// Run each device's status through its debouncer and return the list as the
/// user should see it.
///
/// It deliberately does not decide whether to publish. Whether anything
/// changed is answered by comparing this list against the last one published —
/// a flag assembled here could only enumerate the reasons somebody thought of,
/// and the reason missed was a device being renamed.
///
/// Split out from the thread so the interesting behaviour — a bounce not
/// reaching the UI, a device disappearing, a new device appearing — is testable
/// without threads, timers or hardware.
pub fn debounce(
    debouncers: &mut HashMap<String, Debouncer>,
    scanned: Vec<DetectedDevice>,
    now_ms: u64,
    previous: &[DetectedDevice],
) -> Vec<DetectedDevice> {
    let mut seen: Vec<String> = Vec::with_capacity(scanned.len());

    let mut published: Vec<DetectedDevice> = scanned
        .into_iter()
        .map(|mut d| {
            let key = device_key(&d);
            seen.push(key.clone());

            match debouncers.get_mut(&key) {
                Some(debouncer) => {
                    debouncer.observe(d.status, now_ms);
                    let settled = debouncer.status();

                    // While a status is being held back, hold back the evidence
                    // for it too. `hid_present`, the DirectInput slot and the
                    // rest come straight off the scan and flap with the same
                    // bounce the status does — publishing the settled status
                    // beside instantaneous evidence would show "Connected" on a
                    // row that also says it is absent from HID, which is a
                    // contradiction rather than a delay.
                    if settled != d.status {
                        if let Some(held) = previous.iter().find(|p| device_key(p) == key) {
                            let named = d.device.display_name.clone();
                            let alias_key = d.alias_key.clone();
                            let renamed = d.renamed;
                            d = held.clone();
                            // Naming is not evidence of presence, and a rename
                            // during a bounce should still reach the screen.
                            d.device.display_name = named;
                            d.alias_key = alias_key;
                            d.renamed = renamed;
                        }
                    }
                    // Show the settled status, not the instantaneous one.
                    d.status = settled;
                }
                None => {
                    // First sighting: publish immediately. Debouncing a device
                    // the user just plugged in would make the app feel slow at
                    // exactly the moment they are watching it.
                    debouncers.insert(key, Debouncer::new(d.status));
                }
            }
            d
        })
        .collect();

    // A device that has stopped being reported is *shown as gone*, not removed.
    //
    // Enumeration only returns what is plugged in, so a device that is
    // unplugged simply stops appearing — and a row quietly vanishing from a
    // table is not something anybody notices. "Unplug it and watch it go red"
    // is the behaviour worth having, because the question this page answers is
    // whether you can race right now, and the answer changed.
    //
    // It goes through the same debouncer as everything else, so a cable that
    // wobbles for 300 ms does not paint the screen red and back.
    for gone in previous {
        let key = device_key(gone);
        if seen.contains(&key) {
            continue;
        }

        let debouncer = debouncers
            .entry(key)
            .or_insert_with(|| Debouncer::new(DeviceStatus::Disconnected));
        debouncer.observe(DeviceStatus::Disconnected, now_ms);

        let mut row = gone.clone();
        row.status = debouncer.status();
        // Absent means absent: the evidence has to agree with the status, or
        // the row reads "Disconnected, slot 3", which is a contradiction.
        if row.status == DeviceStatus::Disconnected {
            row.hid_present = false;
            row.dinput_present = false;
            row.dinput_slot = None;
            row.dinput_instance_guid = None;
        }
        published.push(row);
    }

    // One order, decided here, because the list is now two sources joined:
    // what was scanned and what is missing from it.
    published.sort_by(|a, b| {
        a.device
            .display_name
            .to_lowercase()
            .cmp(&b.device.display_name.to_lowercase())
            .then_with(|| a.device.instance_path.cmp(&b.device.instance_path))
    });

    published
}

/// Identity for tracking a device across scans.
///
/// The instance path distinguishes two otherwise identical devices, which
/// VID/PID alone cannot — and two identical un-serialled pedal sets is a real
/// configuration, not a hypothetical one.
pub fn device_key(d: &DetectedDevice) -> String {
    d.device
        .instance_path
        .clone()
        .unwrap_or_else(|| format!("{:04X}:{:04X}", d.device.vid, d.device.pid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use DeviceStatus::*;

    fn obs(hid: bool, dinput: bool) -> Observation {
        Observation {
            hid_present: hid,
            dinput_present: dinput,
            ..Default::default()
        }
    }

    #[test]
    fn unplugged_is_disconnected() {
        assert_eq!(obs(false, false).status(), Disconnected);
        // Even if something stale still claims DirectInput sees it.
        assert_eq!(obs(false, true).status(), Disconnected);
    }

    #[test]
    fn present_but_invisible_to_games_is_connecting() {
        // The window in which a game would bind to the wrong device.
        assert_eq!(obs(true, false).status(), Connecting);
        assert_eq!(obs(true, true).status(), Connected);
    }

    #[test]
    fn a_vjoy_device_with_no_feeder_is_not_ready() {
        // It enumerates perfectly and its axes read dead centre. Calling that
        // Connected is the exact lie this state model exists to avoid.
        let dead = Observation {
            hid_present: true,
            dinput_present: true,
            vjoy_feeder_running: Some(false),
            ..Default::default()
        };
        assert_eq!(dead.status(), Connecting);
        assert!(dead.reason().unwrap().contains("dead centre"));

        let fed = Observation {
            vjoy_feeder_running: Some(true),
            ..dead
        };
        assert_eq!(fed.status(), Connected);
        assert_eq!(fed.reason(), None);
    }

    #[test]
    fn a_missing_vendor_process_holds_it_at_connecting() {
        let d = Observation {
            hid_present: true,
            dinput_present: true,
            vendor_process_running: Some(false),
            ..Default::default()
        };
        assert_eq!(d.status(), Connecting);
        assert!(d.reason().unwrap().contains("vendor software"));
    }

    #[test]
    fn no_declared_requirement_is_not_a_failed_one() {
        // None means "this device declares nothing", not "it failed".
        assert_eq!(obs(true, true).status(), Connected);
    }

    #[test]
    fn reasons_say_what_to_do_not_what_broke() {
        let r = obs(false, false).reason().unwrap();
        assert!(r.contains("USB cable"), "{r}");
        assert!(!r.contains("0x"), "no error codes: {r}");
    }

    // ------------------------------------------------------------- debounce

    #[test]
    fn a_bounce_never_reaches_the_ui() {
        // Arrives, drops, arrives again, all inside the settle window. This is
        // what a real USB device does, and the user should see one transition.
        let mut d = Debouncer::new(Disconnected);
        assert_eq!(d.observe(Connected, 0), None);
        assert_eq!(d.observe(Disconnected, 100), None);
        assert_eq!(d.observe(Connected, 200), None);
        assert_eq!(d.observe(Connected, 400), None);
        // Only once the candidate has held for the full window.
        assert_eq!(d.observe(Connected, 951), Some(Connected));
        assert_eq!(d.status(), Connected);
    }

    #[test]
    fn a_settled_change_does_come_through() {
        let mut d = Debouncer::new(Disconnected);
        assert_eq!(d.observe(Connected, 0), None);
        assert_eq!(d.observe(Connected, 749), None, "not yet");
        assert_eq!(
            d.observe(Connected, 750),
            Some(Connected),
            "exactly at the window"
        );
    }

    #[test]
    fn returning_to_the_shown_status_cancels_the_candidate() {
        // A device that flickers away and comes back should not then be one
        // observation away from reporting the flicker.
        let mut d = Debouncer::new(Connected);
        assert_eq!(d.observe(Disconnected, 0), None);
        assert_eq!(d.observe(Connected, 100), None, "cancels");
        assert_eq!(d.observe(Disconnected, 200), None, "window restarts");
        assert_eq!(d.observe(Disconnected, 800), None, "600 ms is not enough");
        assert_eq!(d.observe(Disconnected, 951), Some(Disconnected));
    }

    #[test]
    fn a_changed_candidate_restarts_the_window() {
        // Disconnected -> Connecting -> Connected in quick succession must not
        // let Connecting's elapsed time promote Connected early.
        let mut d = Debouncer::new(Disconnected);
        assert_eq!(d.observe(Connecting, 0), None);
        assert_eq!(d.observe(Connected, 700), None, "new candidate, new window");
        assert_eq!(d.observe(Connected, 1300), None, "only 600 ms so far");
        assert_eq!(d.observe(Connected, 1450), Some(Connected));
    }

    #[test]
    fn a_device_that_drops_mid_session_is_reported() {
        let mut d = Debouncer::new(Connected);
        assert_eq!(d.observe(Disconnected, 10_000), None);
        assert_eq!(d.observe(Disconnected, 10_800), Some(Disconnected));
    }

    // ---------------------------------------------------------------- drift

    #[test]
    fn a_moved_slot_is_drift() {
        let drift = detect_drift(Some(2), None, Some(4), None).expect("moved");
        assert_eq!(drift.expected_slot, Some(2));
        assert_eq!(drift.actual_slot, Some(4));
    }

    #[test]
    fn an_unchanged_identity_is_not_drift() {
        assert_eq!(
            detect_drift(Some(2), Some("{guid-a}"), Some(2), Some("{guid-a}")),
            None
        );
    }

    #[test]
    fn nothing_recorded_is_not_drift() {
        // A profile saved before slots were tracked must not light up red.
        assert_eq!(detect_drift(None, None, Some(4), Some("{guid}")), None);
    }

    #[test]
    fn a_changed_guid_is_drift_even_at_the_same_slot() {
        // The stronger signal: the slot happens to match but it is a different
        // device sitting in it.
        let drift = detect_drift(Some(1), Some("{guid-a}"), Some(1), Some("{guid-b}"))
            .expect("different device");
        assert_eq!(drift.actual_instance_guid.as_deref(), Some("{guid-b}"));
    }

    #[test]
    fn an_unplugged_device_is_not_reported_as_drifted() {
        // Absent is a status, not a binding problem, and saying both would be
        // two alarms for one cause.
        assert_eq!(detect_drift(Some(2), Some("{guid-a}"), None, None), None);
    }

    use crate::{DetectedDevice, DeviceRef};

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
            alias_key: crate::alias_key(0x0EB7, 0x0E04, None, Some(path)),
            renamed: false,
        }
    }

    /// Renamed, so a test can prove a name change is publishable.
    fn renamed(path: &str, status: DeviceStatus, name: &str) -> DetectedDevice {
        let mut d = device(path, status);
        d.device.display_name = name.into();
        d.renamed = true;
        d
    }

    #[test]
    fn a_new_device_is_published_at_once() {
        // Debouncing something the user just plugged in would make the app feel
        // slow at exactly the moment they are looking at it.
        let mut d = HashMap::new();
        let published = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0, &[]);
        assert_ne!(published, Vec::new(), "a first sighting is a change");
        assert_eq!(published[0].status, DeviceStatus::Connected);
    }

    #[test]
    fn a_bounce_does_not_reach_the_ui() {
        let mut d = HashMap::new();
        let first = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0, &[]);

        // Drops and returns well inside the settle window. Publication is
        // decided by comparing with the last list, so "does not reach the UI"
        // means exactly "produces the same list" — every field of it, not just
        // the status.
        let during = debounce(
            &mut d,
            vec![device("a", DeviceStatus::Disconnected)],
            100,
            &first,
        );
        assert_eq!(during, first, "a 100 ms dropout is a bounce, not an event");
        assert_eq!(
            during[0].status,
            DeviceStatus::Connected,
            "still shown as connected"
        );
        assert!(
            during[0].hid_present,
            "and not connected-but-absent-from-HID, which is a contradiction"
        );

        let back = debounce(
            &mut d,
            vec![device("a", DeviceStatus::Connected)],
            300,
            &during,
        );
        assert_eq!(back, first);
    }

    #[test]
    fn a_real_disconnection_does_reach_the_ui() {
        let mut d = HashMap::new();
        let first = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0, &[]);
        let during = debounce(
            &mut d,
            vec![device("a", DeviceStatus::Disconnected)],
            100,
            &first,
        );
        let settled = debounce(
            &mut d,
            vec![device("a", DeviceStatus::Disconnected)],
            1000,
            &during,
        );
        assert_ne!(settled, first);
        assert_eq!(settled[0].status, DeviceStatus::Disconnected);
    }

    #[test]
    fn renaming_a_device_is_a_change_even_though_its_status_did_not_move() {
        // The bug this replaced: publication was decided by status alone, so a
        // rename was saved correctly and never appeared until something was
        // unplugged. Nothing about the device moved — only what it is called.
        let mut d = HashMap::new();
        let before = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0, &[]);
        let after = debounce(
            &mut d,
            vec![renamed("a", DeviceStatus::Connected, "Left pedal box")],
            50,
            &before,
        );

        assert_ne!(before, after);
        assert_eq!(after[0].device.display_name, "Left pedal box");
        assert_eq!(
            after[0].status,
            DeviceStatus::Connected,
            "and the rename did not disturb the settled status"
        );
    }

    #[test]
    fn unplugging_a_device_turns_its_row_red_rather_than_deleting_it() {
        // Enumeration only returns what is plugged in, so an unplugged device
        // simply stops appearing — and a row quietly vanishing from a table is
        // not something anybody notices. The question this page answers is
        // whether you can race right now, and that answer just changed.
        let mut d = HashMap::new();
        let before = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0, &[]);

        // Gone from the scan. Inside the settle window it is still shown as it
        // was, because a cable that wobbles is not a disconnection.
        let during = debounce(&mut d, vec![], 100, &before);
        assert_eq!(during.len(), 1, "the row is still there");
        assert_eq!(during[0].status, DeviceStatus::Connected, "not yet");

        // Still gone once the window has passed: now it is red.
        let settled = debounce(&mut d, vec![], 1000, &during);
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].status, DeviceStatus::Disconnected);
        assert_ne!(settled, before, "and that reaches the UI");
    }

    #[test]
    fn a_disconnected_row_does_not_claim_a_directinput_slot() {
        // "Disconnected, slot 3" is a contradiction: the evidence has to agree
        // with the status or the row is telling two stories.
        let mut d = HashMap::new();
        let mut connected = device("a", DeviceStatus::Connected);
        connected.dinput_slot = Some(3);
        connected.dinput_instance_guid = Some("{guid}".into());

        let before = debounce(&mut d, vec![connected], 0, &[]);
        debounce(&mut d, vec![], 100, &before);
        let settled = debounce(&mut d, vec![], 1000, &before);

        assert_eq!(settled[0].status, DeviceStatus::Disconnected);
        assert!(!settled[0].hid_present);
        assert!(!settled[0].dinput_present);
        assert_eq!(settled[0].dinput_slot, None);
        assert_eq!(settled[0].dinput_instance_guid, None);
    }

    #[test]
    fn plugging_it_back_in_turns_it_green_again() {
        let mut d = HashMap::new();
        let before = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0, &[]);
        // Two scans: the first starts the settle window, the second finds it
        // still absent once the window has passed.
        let going = debounce(&mut d, vec![], 100, &before);
        let gone = debounce(&mut d, vec![], 1000, &going);
        assert_eq!(gone[0].status, DeviceStatus::Disconnected);

        // Back in the scan. It takes one settle window to go green, and that
        // is the point rather than a delay to apologise for: a device coming
        // back *is* the bounce case — present, gone, present again while it
        // enumerates — and painting the row green on the first sighting is how
        // a list ends up flickering.
        let returning = debounce(
            &mut d,
            vec![device("a", DeviceStatus::Connected)],
            2000,
            &gone,
        );
        assert_eq!(returning.len(), 1, "one row, not a second copy");

        let back = debounce(
            &mut d,
            vec![device("a", DeviceStatus::Connected)],
            3000,
            &returning,
        );
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].status, DeviceStatus::Connected);
        assert!(back[0].hid_present, "with its evidence back too");
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
            &[],
        );
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn a_steady_state_publishes_nothing() {
        // The UI should not be woken four times a second to be told nothing
        // happened — which, under the comparison rule, means a quiet scan must
        // produce a list equal to the last one.
        let mut d = HashMap::new();
        let first = debounce(&mut d, vec![device("a", DeviceStatus::Connected)], 0, &[]);
        let again = debounce(
            &mut d,
            vec![device("a", DeviceStatus::Connected)],
            4000,
            &first,
        );
        assert_eq!(first, again);
    }
}
