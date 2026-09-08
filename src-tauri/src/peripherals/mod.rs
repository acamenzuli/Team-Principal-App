//! Peripherals.
//!
//! Windows exposes game controllers through several layers that disagree with
//! each other, and the disagreements are exactly what breaks sim rigs. Each
//! layer is queried for the one thing it is authoritative about:
//!
//! | Layer | Authoritative for |
//! | --- | --- |
//! | HID / SetupAPI | whether the device is physically plugged in, and its VID, PID and serial |
//! | DirectInput | whether *games* can see it, and its enumeration slot |
//! | vJoy | whether a virtual device has anything feeding it |
//! | Vendor process | whether the peripheral's own software is up |
//!
//! The rules for turning those four answers into one status live in
//! `tp_model::presence`, where they are tested without hardware.

pub mod hid;
pub mod vjoy;

use tp_model::{Catalog, DetectedDevice, Observation};

use crate::error::AppResult;

/// Enumerate every game controller and decide its status.
pub fn enumerate(catalog: &Catalog) -> AppResult<Vec<DetectedDevice>> {
    let hid_devices: Vec<hid::HidDevice> = hid::enumerate()
        .into_iter()
        .filter(|d| d.is_game_controller())
        .collect();

    let vjoy = vjoy::probe();

    let mut out: Vec<DetectedDevice> = hid_devices
        .iter()
        .map(|d| {
            let is_virtual = vjoy::is_vjoy_device(d.vid, d.pid);
            let display_name = catalog.best_name(d.vid, d.pid, None, d.product.as_deref());

            // DirectInput ordering is not yet read; until it is, a device is
            // reported as present at HID and not yet confirmed visible to
            // games. Claiming otherwise would be exactly the lie this module
            // exists to prevent.
            let observation = Observation {
                hid_present: true,
                dinput_present: false,
                vendor_process_running: None,
                vjoy_feeder_running: is_virtual.then_some(false),
            };

            DetectedDevice {
                device: d.to_ref(display_name),
                manufacturer: d.manufacturer.clone(),
                raw_product_name: d.product.clone(),
                status: observation.status(),
                hid_present: true,
                dinput_present: false,
                dinput_slot: None,
                dinput_instance_guid: None,
                is_virtual,
                vjoy: is_virtual.then(|| vjoy.info_for(d.vid, d.pid)),
                binding_drift: None,
            }
        })
        .collect();

    // Stable order, so the list does not reshuffle between scans. Identical
    // devices are separated by their instance path, which is the only thing
    // that distinguishes two of the same un-serialled pedal set.
    out.sort_by(|a, b| {
        a.device
            .display_name
            .cmp(&b.device.display_name)
            .then_with(|| a.device.instance_path.cmp(&b.device.instance_path))
    });

    tracing::info!(count = out.len(), "enumerated game controllers");
    Ok(out)
}
