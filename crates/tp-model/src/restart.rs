//! Restarting a device without touching its cable.
//!
//! The restart itself is a Win32 call and lives in `src-tauri`. What lives
//! here is the two decisions around it that can be got wrong, kept where they
//! can be tested: which node in the device's ancestry to restart, and how the
//! outcome crosses back from the elevated process that does the restarting.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What pressing Reconnect produced, as the UI sees it.
///
/// Only the two outcomes that are not failures. Everything that went wrong is
/// an error with a sentence attached, because each failure has a different
/// thing the person should do next and a status alone cannot say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ReconnectOutcome {
    /// Stopped, started again, and seen running afterwards.
    Restarted,
    /// The administrator prompt was declined. Nothing was touched.
    Declined,
}

/// What restarting one device node produced.
///
/// The process that restarts a device is a separate, elevated one, and an
/// exit status is the whole of what comes back from it. So every outcome has a
/// number, and the numbers are the contract between the two processes — which
/// is why they are here, where a test can hold both ends of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartResult {
    /// Stopped, started again, and reported running by Windows afterwards.
    Restarted,
    /// Windows no longer lists the device. Unplugged, most likely.
    NotFound,
    /// Windows would not stop it. Something holds it open, or the caller is
    /// not an administrator after all.
    Refused,
    /// Stopped, and would not start again. The device is now disabled in
    /// Windows, which is the one outcome worse than doing nothing, and it is
    /// reported rather than swallowed.
    LeftDisabled,
    /// Started again as far as Windows is concerned, but not seen running
    /// within the wait. A device whose firmware has locked up looks like this;
    /// the cable is the remaining fix.
    NotBack,
    /// The helper was not given a device to restart.
    BadArguments,
}

impl RestartResult {
    /// The exit status the helper ends with. Zero is success, as every shell
    /// expects, and one is avoided because it is what a panic produces.
    pub fn exit_code(self) -> i32 {
        match self {
            RestartResult::Restarted => 0,
            RestartResult::BadArguments => 2,
            RestartResult::NotFound => 3,
            RestartResult::Refused => 4,
            RestartResult::LeftDisabled => 5,
            RestartResult::NotBack => 6,
        }
    }

    /// The other end of [`exit_code`](Self::exit_code). `None` for a status the
    /// helper never produces — a crash, or a different version of the app
    /// answering.
    pub fn from_exit_code(code: i32) -> Option<RestartResult> {
        [
            RestartResult::Restarted,
            RestartResult::BadArguments,
            RestartResult::NotFound,
            RestartResult::Refused,
            RestartResult::LeftDisabled,
            RestartResult::NotBack,
        ]
        .into_iter()
        .find(|r| r.exit_code() == code)
    }
}

/// Which node in a device's ancestry to restart. Returns an index into
/// `chain`.
///
/// `chain` is the instance ID of the HID node first, then its parent, its
/// grandparent, and so on up the Plug and Play tree.
///
/// A USB game controller is rarely one node. A composite device — a wheelbase
/// with a HID interface, a serial interface and a firmware-update interface —
/// is one node for the physical device, one child per interface, and the HID
/// collection is a child of *that*. Restarting only the HID collection
/// rebuilds a driver object without the device noticing; restarting the
/// physical device is what unplugging it does. So the target is the topmost
/// ancestor that is still this device: the end of an unbroken run, from the
/// HID node upward, of nodes carrying the same vendor and product IDs. The hub
/// above carries different ones, and the run stops there.
///
/// A device whose IDs are not written in the USB form — Bluetooth spells them
/// differently — matches nothing above it, so the HID node itself is
/// restarted. That is still a restart, just a shallower one.
pub fn node_to_restart(chain: &[String], vid: u16, pid: u16) -> usize {
    let same_device = format!("VID_{vid:04X}&PID_{pid:04X}");
    let mut target = 0;
    for (index, id) in chain.iter().enumerate().skip(1) {
        if id.to_ascii_uppercase().contains(&same_device) {
            target = index;
        } else {
            break;
        }
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_composite_device_is_restarted_as_a_whole() {
        // A shifter with three interfaces. The HID collection is two levels
        // below the physical device, and the hub above that is not ours.
        let ids = chain(&[
            r"HID\VID_346E&PID_001E&MI_02\9&39D906E1&0&0000",
            r"USB\VID_346E&PID_001E&MI_02\7&2A1B3C4D&0&0002",
            r"USB\VID_346E&PID_001E\39001F001353575536333120",
            r"USB\VID_05E3&PID_0610\6&1A2B3C4D&0&3",
            r"USB\ROOT_HUB30\5&ABCDEF&0&0",
        ]);
        assert_eq!(node_to_restart(&ids, 0x346E, 0x001E), 2);
    }

    #[test]
    fn a_plain_device_is_restarted_at_its_usb_node() {
        let ids = chain(&[
            r"HID\VID_04D8&PID_E557\8&1234ABCD&0&0000",
            r"USB\VID_04D8&PID_E557\6&FEDCBA98&0&4",
            r"USB\ROOT_HUB30\5&ABCDEF&0&0",
        ]);
        assert_eq!(node_to_restart(&ids, 0x04D8, 0xE557), 1);
    }

    #[test]
    fn the_run_stops_at_the_first_node_that_is_not_this_device() {
        // Two identical pedal sets on one hub of their own brand would share
        // IDs with the hub. What must never happen is skipping over a foreign
        // node to reach something matching above it.
        let ids = chain(&[
            r"HID\VID_0EB7&PID_183B\8&1&0&0000",
            r"USB\VID_0EB7&PID_183B\6&2&0&1",
            r"USB\VID_05E3&PID_0610\6&3&0&3",
            r"USB\VID_0EB7&PID_183B\6&4&0&2",
        ]);
        assert_eq!(node_to_restart(&ids, 0x0EB7, 0x183B), 1);
    }

    #[test]
    fn a_bluetooth_device_falls_back_to_its_own_node() {
        let ids = chain(&[
            r"HID\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002046D_PID&B016\8&1&0&0000",
            r"BTHENUM\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002046D_PID&B016\7&2&0&0",
            r"BTH\MS_BTHBRB\6&3&0&1",
        ]);
        assert_eq!(node_to_restart(&ids, 0x046D, 0xB016), 0);
    }

    #[test]
    fn case_does_not_matter() {
        // Windows is inconsistent about the case of instance IDs between APIs,
        // exactly as it is with interface paths.
        let ids = chain(&[
            r"hid\vid_346e&pid_001e\9&1&0&0000",
            r"usb\vid_346e&pid_001e\39001f00",
            r"usb\root_hub30\5&2&0&0",
        ]);
        assert_eq!(node_to_restart(&ids, 0x346E, 0x001E), 1);
    }

    #[test]
    fn a_chain_of_one_is_its_own_target() {
        let ids = chain(&[r"HID\VID_346E&PID_001E\9&1&0&0000"]);
        assert_eq!(node_to_restart(&ids, 0x346E, 0x001E), 0);
    }

    #[test]
    fn every_result_survives_the_trip_through_an_exit_code() {
        for result in [
            RestartResult::Restarted,
            RestartResult::NotFound,
            RestartResult::Refused,
            RestartResult::LeftDisabled,
            RestartResult::NotBack,
            RestartResult::BadArguments,
        ] {
            assert_eq!(
                RestartResult::from_exit_code(result.exit_code()),
                Some(result)
            );
        }
    }

    #[test]
    fn only_success_is_zero_and_nothing_is_one() {
        // Zero is what every caller reads as success. One is what a Rust panic
        // exits with, and a crash must never be mistaken for an answer.
        assert_eq!(RestartResult::Restarted.exit_code(), 0);
        assert_eq!(RestartResult::from_exit_code(1), None);
        assert_eq!(RestartResult::from_exit_code(99), None);
    }
}
