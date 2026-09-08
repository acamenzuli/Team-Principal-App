//! The known-device catalog.
//!
//! Windows names a great many sim racing peripherals "HID-compliant game
//! controller", and vJoy devices "vJoy Device". Neither tells anyone which
//! pedal set they are looking at, and a rig with six of them becomes six
//! identical rows.
//!
//! The catalog maps VID/PID to a real name. It ships with a seed, is
//! extensible from a JSON file so a new wheelbase can arrive as a content
//! update rather than a release, and every entry is renameable by the user —
//! whose name always wins, because it is their rig.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    pub manufacturer: String,
    /// What the thing is, so the UI can group and the launcher can suggest
    /// which peripherals a profile probably needs.
    pub kind: DeviceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Wheelbase,
    Pedals,
    Handbrake,
    Shifter,
    ButtonBox,
    Wheel,
    Virtual,
    Other,
}

/// VID/PID pairs, which are the only stable identity a USB device has.
pub type DeviceKey = (u16, u16);

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: BTreeMap<DeviceKey, CatalogEntry>,
}

impl Catalog {
    /// The built-in catalog.
    ///
    /// Deliberately small and honest. Every entry here is a vendor ID that is
    /// publicly documented or verifiable from a USB descriptor; guessing a
    /// PID produces a confidently wrong label, which is worse than "HID-
    /// compliant game controller" because the user stops checking.
    pub fn seeded() -> Self {
        let mut c = Self::default();
        for (vid, pid, name, manufacturer, kind) in [
            // vJoy's virtual device. Fixed and well known.
            (
                0x1234,
                0xBEAD,
                "vJoy Device",
                "Shaul Eizikovich",
                DeviceKind::Virtual,
            ),
            // Vendor IDs below are the identifiers these makers ship under.
            // Where a specific product is not confirmed the entry names the
            // vendor only, which is still far better than nothing.
            (
                0x0EB7,
                0x0E04,
                "Fanatec wheelbase",
                "Endor AG",
                DeviceKind::Wheelbase,
            ),
            (
                0x0EB7,
                0x183B,
                "Fanatec pedals",
                "Endor AG",
                DeviceKind::Pedals,
            ),
            (
                0x346E,
                0x0000,
                "Moza wheelbase",
                "Moza Racing",
                DeviceKind::Wheelbase,
            ),
            (
                0x16D0,
                0x0D5A,
                "Simucube",
                "Granite Devices",
                DeviceKind::Wheelbase,
            ),
            (
                0x0483,
                0x5750,
                "STM32 HID device",
                "STMicroelectronics",
                DeviceKind::Other,
            ),
        ] {
            c.insert(CatalogEntry {
                vid,
                pid,
                name: name.into(),
                manufacturer: manufacturer.into(),
                kind,
            });
        }
        c
    }

    pub fn insert(&mut self, entry: CatalogEntry) {
        self.entries.insert((entry.vid, entry.pid), entry);
    }

    /// Merge a catalog loaded from disk over the seed. Later wins, so a content
    /// update can correct a built-in entry without a new build.
    pub fn merge(&mut self, entries: impl IntoIterator<Item = CatalogEntry>) {
        for e in entries {
            self.insert(e);
        }
    }

    pub fn get(&self, vid: u16, pid: u16) -> Option<&CatalogEntry> {
        self.entries.get(&(vid, pid))
    }

    /// The best name available, in order of trust:
    ///
    /// 1. what the user called it — their rig, their words;
    /// 2. the catalog, which knows what "HID-compliant game controller" means;
    /// 3. what the device says about itself, if it is not a generic placeholder;
    /// 4. the VID/PID, which at least identifies it uniquely.
    pub fn best_name(
        &self,
        vid: u16,
        pid: u16,
        user_name: Option<&str>,
        reported: Option<&str>,
    ) -> String {
        if let Some(name) = user_name.map(str::trim).filter(|s| !s.is_empty()) {
            return name.to_string();
        }
        if let Some(entry) = self.get(vid, pid) {
            return entry.name.clone();
        }
        if let Some(name) = reported.map(str::trim).filter(|s| !is_placeholder(s)) {
            return name.to_string();
        }
        format!("Unknown device {vid:04X}:{pid:04X}")
    }

    pub fn kind(&self, vid: u16, pid: u16) -> DeviceKind {
        self.get(vid, pid)
            .map(|e| e.kind)
            .unwrap_or(DeviceKind::Other)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Names Windows invents when it has nothing useful to say.
pub fn is_placeholder(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n.is_empty()
        || n == "vjoy device"
        || n.contains("hid-compliant")
        || n.contains("usb input device")
        || n.contains("generic usb joystick")
        || n.contains("game controller")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_name_beats_everything() {
        let c = Catalog::seeded();
        assert_eq!(
            c.best_name(
                0x1234,
                0xBEAD,
                Some("Sim-Lab handbrake"),
                Some("vJoy Device")
            ),
            "Sim-Lab handbrake"
        );
    }

    #[test]
    fn the_catalog_beats_a_windows_placeholder() {
        let c = Catalog::seeded();
        assert_eq!(
            c.best_name(0x1234, 0xBEAD, None, Some("HID-compliant game controller")),
            "vJoy Device"
        );
    }

    #[test]
    fn a_real_reported_name_beats_nothing() {
        let c = Catalog::seeded();
        assert_eq!(
            c.best_name(0xAAAA, 0xBBBB, None, Some("Invicta S-Series")),
            "Invicta S-Series"
        );
    }

    #[test]
    fn an_unknown_device_is_named_by_its_ids_not_by_a_guess() {
        let c = Catalog::seeded();
        // Never invent a product name. A wrong label is worse than an
        // uninformative one, because the user stops checking it.
        assert_eq!(
            c.best_name(0xAAAA, 0xBBBB, None, Some("HID-compliant game controller")),
            "Unknown device AAAA:BBBB"
        );
        assert_eq!(
            c.best_name(0xAAAA, 0xBBBB, None, None),
            "Unknown device AAAA:BBBB"
        );
    }

    #[test]
    fn whitespace_is_not_a_name() {
        let c = Catalog::seeded();
        assert_eq!(
            c.best_name(0x1234, 0xBEAD, Some("   "), None),
            "vJoy Device"
        );
    }

    #[test]
    fn a_loaded_catalog_can_correct_the_seed() {
        // The point of shipping adapter and catalog data separately: a new
        // wheelbase should not need a new build.
        let mut c = Catalog::seeded();
        assert_eq!(c.best_name(0x0EB7, 0x0E04, None, None), "Fanatec wheelbase");
        c.merge([CatalogEntry {
            vid: 0x0EB7,
            pid: 0x0E04,
            name: "Fanatec CSL DD".into(),
            manufacturer: "Endor AG".into(),
            kind: DeviceKind::Wheelbase,
        }]);
        assert_eq!(c.best_name(0x0EB7, 0x0E04, None, None), "Fanatec CSL DD");
    }

    #[test]
    fn placeholders_are_recognised() {
        for name in [
            "HID-compliant game controller",
            "USB Input Device",
            "vJoy Device",
            "Generic USB Joystick",
            "   ",
        ] {
            assert!(is_placeholder(name), "{name} should be a placeholder");
        }
        for name in ["Invicta S-Series", "LaPrima", "Simucube 2 Pro"] {
            assert!(!is_placeholder(name), "{name} is a real name");
        }
    }
}
