//! Peripherals. The three-state status model is the interesting part: on a sim
//! rig "plugged in" and "ready to race" are genuinely different things, and
//! collapsing them into a boolean is why other tools mislead you.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRef {
    pub vid: u16,
    pub pid: u16,
    /// The preferred disambiguator between two identical devices.
    pub serial: Option<String>,
    /// Fallback when a device reports no serial. Two identical un-serialled
    /// pedal sets can only be told apart by which port they are in.
    pub instance_path: Option<String>,
    /// Always renameable by the user, whatever the catalog or Windows says.
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DetectedDevice {
    pub device: DeviceRef,
    pub manufacturer: Option<String>,
    /// What Windows calls it, before the catalog cleans it up. Frequently
    /// "HID-compliant game controller", which is why the catalog exists.
    pub raw_product_name: Option<String>,
    pub status: DeviceStatus,
    /// Present at the HID layer.
    pub hid_present: bool,
    /// Listed by DirectInput — i.e. games can actually see it.
    pub dinput_present: bool,
    /// DirectInput enumeration slot. Games bind controls to this, and it can
    /// change across a reboot or a hotplug, silently destroying bindings.
    pub dinput_slot: Option<u32>,
    pub dinput_instance_guid: Option<String>,
    /// True when this is a vJoy virtual device rather than real hardware.
    pub is_virtual: bool,
    pub vjoy: Option<VJoyInfo>,
    /// Set when the device works but its DirectInput identity has moved since
    /// the profile was saved. A badge on top of Connected, not a fourth state.
    pub binding_drift: Option<BindingDrift>,
    /// What a user-given name for this device is stored against. On the wire so
    /// the UI can ask for a rename without reimplementing the identity rules —
    /// two implementations of "which device is this" would disagree eventually,
    /// and the symptom would be a name silently attaching to the wrong pedals.
    ///
    /// `#[serde(default)]` so fixture files written before this existed still
    /// load; an empty key simply means nothing to rename.
    #[serde(default)]
    pub alias_key: String,
    /// True when `display_name` is the user's own name rather than the
    /// catalog's or Windows'. Drives whether there is anything to reset.
    #[serde(default)]
    pub renamed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    /// Not present at the HID layer at all.
    Disconnected,
    /// Present but not yet usable. Covers a device still enumerating after
    /// hotplug, one DirectInput has not listed yet, a vJoy device whose feeder
    /// is not running so its axes are dead, and a peripheral whose vendor
    /// process is not up.
    Connecting,
    /// Present at HID, visible to DirectInput, and any declared vendor process
    /// or vJoy feeder is live.
    Connected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BindingDrift {
    pub expected_slot: Option<u32>,
    pub actual_slot: Option<u32>,
    pub expected_instance_guid: Option<String>,
    pub actual_instance_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VJoyInfo {
    /// vJoy device id, 1..=16.
    pub device_id: u8,
    pub driver_version: Option<String>,
    pub interface_version: Option<String>,
    /// vJoy reports its own status; a device can exist but be unowned, which
    /// means nothing is feeding it and its axes read as dead centre.
    pub status: VJoyStatus,
    /// Process expected to be feeding this device.
    pub feeder_process: Option<String>,
    pub feeder_running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum VJoyStatus {
    Own,
    Free,
    Busy,
    Missing,
    Unknown,
    /// Driver present but its version does not match the installed interface —
    /// a classic cause of devices that look fine and do nothing.
    VersionMismatch,
}

/// A live snapshot of one device's axes and buttons, for the input monitor.
/// Read shared, never exclusive, and suspended entirely while a game runs so
/// the app can never steal input from the game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct InputState {
    pub device: DeviceRef,
    /// Normalised to -1.0..=1.0.
    pub axes: Vec<AxisState>,
    pub buttons: Vec<bool>,
    pub hats: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AxisState {
    pub name: String,
    pub value: f64,
}

/// A timestamped status transition, so "my pedals dropped mid-race" is
/// diagnosable after the fact instead of being a mystery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEvent {
    pub at: String,
    pub device: DeviceRef,
    pub from: DeviceStatus,
    pub to: DeviceStatus,
    pub reason: String,
}

/// The key a user's own name for a device is stored against.
///
/// A USB device has no single stable identity, so this picks the best one
/// available and says which it used:
///
/// * **Serial**, when the device reports one. Survives being moved to another
///   port, another hub, another motherboard header.
/// * **Interface path**, when it does not. Two identical un-serialled pedal
///   sets are genuinely indistinguishable except by which port they are in, so
///   the port is the identity — and moving one to a different port loses its
///   name, which is the honest outcome rather than silently applying the name
///   to whatever is plugged in there next.
/// * **Model**, for a device with neither. Then the name belongs to every
///   device of that model, which is the most that can be said.
pub fn alias_key(vid: u16, pid: u16, serial: Option<&str>, instance_path: Option<&str>) -> String {
    if let Some(serial) = serial.map(str::trim).filter(|s| !s.is_empty()) {
        return format!("serial:{vid:04x}:{pid:04x}:{serial}");
    }
    if let Some(path) = instance_path.map(str::trim).filter(|s| !s.is_empty()) {
        // Windows is inconsistent about the case of interface paths between
        // APIs, and a key that changes case is a name that disappears.
        return format!("path:{}", path.to_ascii_lowercase());
    }
    format!("model:{vid:04x}:{pid:04x}")
}

impl DeviceRef {
    /// The key this device's user-given name is stored against.
    pub fn alias_key(&self) -> String {
        alias_key(
            self.vid,
            self.pid,
            self.serial.as_deref(),
            self.instance_path.as_deref(),
        )
    }
}

#[cfg(test)]
mod alias_key_tests {
    use super::*;

    fn dev(serial: Option<&str>, path: Option<&str>) -> DeviceRef {
        DeviceRef {
            vid: 0x0EB7,
            pid: 0x183B,
            serial: serial.map(str::to_string),
            instance_path: path.map(str::to_string),
            display_name: "whatever".into(),
        }
    }

    #[test]
    fn a_serial_beats_the_port_it_is_plugged_into() {
        let front = dev(Some("SN-9931"), Some(r"\\?\hid#vid_0eb7&pid_183b#7&1a"));
        let back = dev(Some("SN-9931"), Some(r"\\?\hid#vid_0eb7&pid_183b#7&2b"));
        assert_eq!(front.alias_key(), back.alias_key(), "same device, new port");
    }

    #[test]
    fn two_identical_unserialled_pedal_sets_are_told_apart_by_port() {
        let left = dev(None, Some(r"\\?\hid#vid_0eb7&pid_183b#7&1a"));
        let right = dev(None, Some(r"\\?\hid#vid_0eb7&pid_183b#7&2b"));
        assert_ne!(left.alias_key(), right.alias_key());
    }

    #[test]
    fn a_path_that_changes_case_is_the_same_device() {
        let lower = dev(None, Some(r"\\?\hid#vid_0eb7&pid_183b#7&1a"));
        let upper = dev(None, Some(r"\\?\HID#VID_0EB7&PID_183B#7&1A"));
        assert_eq!(lower.alias_key(), upper.alias_key());
    }

    #[test]
    fn an_empty_serial_is_not_an_identity() {
        let blank = dev(Some("   "), Some(r"\\?\hid#x"));
        assert!(blank.alias_key().starts_with("path:"));
    }

    #[test]
    fn with_neither_the_name_belongs_to_the_model() {
        assert_eq!(dev(None, None).alias_key(), "model:0eb7:183b");
    }
}

/// Which kind of control an input alias names.
///
/// Kept apart because the numbering restarts for each: button 1 and axis 1 are
/// different things on the same device, and a game's binding screen numbers
/// them that way too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Axis,
    Button,
    Hat,
}

impl InputKind {
    fn tag(self) -> &'static str {
        match self {
            InputKind::Axis => "axis",
            InputKind::Button => "button",
            InputKind::Hat => "hat",
        }
    }
}

/// The key one control's name is stored against.
///
/// Built on the device's own alias key, so a name given to "button 3" follows
/// the device the same way the device's name does — and two identical button
/// boxes keep their own names for their own buttons rather than sharing one
/// set.
pub fn input_alias_key(device_key: &str, kind: InputKind, index: u32) -> String {
    format!("{device_key}|{}|{index}", kind.tag())
}

#[cfg(test)]
mod input_alias_tests {
    use super::*;

    const DEVICE: &str = "serial:346e:001e:39001F00";

    #[test]
    fn the_same_number_on_different_kinds_is_a_different_control() {
        assert_ne!(
            input_alias_key(DEVICE, InputKind::Button, 1),
            input_alias_key(DEVICE, InputKind::Axis, 1),
        );
    }

    #[test]
    fn two_identical_devices_keep_their_own_names() {
        let left = input_alias_key("path:\\\\?\\hid#a", InputKind::Button, 3);
        let right = input_alias_key("path:\\\\?\\hid#b", InputKind::Button, 3);
        assert_ne!(left, right);
    }

    #[test]
    fn a_control_key_is_built_on_the_device_key() {
        // So a device identified by serial keeps its control names across a
        // replug, exactly as its own name does.
        assert!(input_alias_key(DEVICE, InputKind::Hat, 0).starts_with(DEVICE));
    }
}
