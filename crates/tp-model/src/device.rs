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
