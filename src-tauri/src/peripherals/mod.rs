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

pub mod dinput;
pub mod hid;
pub mod monitor;
#[cfg(windows)]
pub mod notify;
pub mod vjoy;
pub mod watch;

use tp_model::{Catalog, DetectedDevice, Observation};

use crate::error::AppResult;

/// Enumerate every game controller and decide its status.
pub fn enumerate(
    catalog: &Catalog,
    aliases: &std::collections::BTreeMap<String, String>,
) -> AppResult<Vec<DetectedDevice>> {
    let hid_devices: Vec<hid::HidDevice> = hid::enumerate()
        .into_iter()
        .filter(|d| d.is_game_controller())
        .collect();

    let vjoy = vjoy::probe();
    let di_devices = dinput::enumerate();

    // How many of each VID/PID have already been matched, so a pair of
    // identical pedals gets one DirectInput slot each rather than both taking
    // the first. Order within a model is the only thing available to pair them
    // by, and it is at least stable within a single scan.
    let mut taken: std::collections::HashMap<(u16, u16), usize> = std::collections::HashMap::new();

    let mut out: Vec<DetectedDevice> = hid_devices
        .iter()
        .map(|d| {
            let is_virtual = vjoy::is_vjoy_device(d.vid, d.pid);
            // The user's own name wins over the catalog and over Windows —
            // `best_name` has taken one since it was written, and this is
            // where it finally gets one.
            let alias_key = tp_model::alias_key(
                d.vid,
                d.pid,
                d.serial.as_deref(),
                Some(d.instance_path.as_str()),
            );
            let alias = aliases.get(&alias_key).map(String::as_str);
            let display_name = catalog.best_name(d.vid, d.pid, alias, d.product.as_deref());

            let nth = taken.entry((d.vid, d.pid)).or_insert(0);
            let di = di_devices
                .iter()
                .filter(|x| x.vid == d.vid && x.pid == d.pid)
                .nth(*nth);
            *nth += 1;

            let observation = Observation {
                hid_present: true,
                dinput_present: di.is_some(),
                // A profile declares vendor and feeder requirements; a bare
                // scan has none to check. None means "nothing declared", not
                // "checked and failed".
                vendor_process_running: None,
                vjoy_feeder_running: None,
            };

            DetectedDevice {
                device: d.to_ref(display_name),
                manufacturer: d.manufacturer.clone(),
                raw_product_name: d
                    .product
                    .clone()
                    .or_else(|| di.and_then(|x| x.product_name.clone())),
                status: observation.status(),
                hid_present: true,
                dinput_present: di.is_some(),
                dinput_slot: di.map(|x| x.slot),
                dinput_instance_guid: di.map(|x| x.instance_guid.clone()),
                is_virtual,
                vjoy: is_virtual.then(|| vjoy.info_for(d.vid, d.pid)),
                // Drift is a comparison against a saved profile, so it is
                // decided when a profile is in hand, not during a bare scan.
                binding_drift: None,
                renamed: alias.is_some(),
                alias_key,
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

    tracing::info!(
        hid = hid_devices.len(),
        dinput = di_devices.len(),
        "enumerated game controllers"
    );
    Ok(out)
}
