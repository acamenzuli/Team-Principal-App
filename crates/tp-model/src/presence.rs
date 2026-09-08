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

use crate::{BindingDrift, DeviceStatus};

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
}
