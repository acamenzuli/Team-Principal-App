//! Devices that present themselves as more than one controller.
//!
//! An Asetek wheel has more inputs than some games will take from one device
//! — Automobilista 2 stops at 32 — so RaceHub has a *legacy input mode* that
//! splits the wheel into several HID game controllers, each inside the limit.
//! Windows then lists one wheel four times: same VID, same PID, same serial,
//! same name, told apart only by the collection number in the interface path.
//! That is not four devices, and it is not one either. It is one device in
//! four parts, and each part is something a game binds to on its own and the
//! person names on its own.
//!
//! Everything here is pure: given the identities in a scan, which of them are
//! parts of one device, which part each is, and what that says about the mode
//! the device is in.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::DetectedDevice;

/// Asetek SimSports.
pub const ASETEK_VID: u16 = 0x2433;

/// Which of its presentations a device is using.
///
/// Only for devices that have more than one. A device that always looks the
/// same is in no mode at all, and saying "normal mode" of it would invite the
/// question of what the other one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DeviceMode {
    /// One controller carrying every input.
    Normal,
    /// Several controllers, each carrying a share of the inputs. What older
    /// games with a per-device input limit need.
    Legacy,
}

/// One part of a device that Windows lists as several controllers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSection {
    /// What distinguishes this part in the interface path — `mi_01&col03` —
    /// and the only thing that does. Stable across reboots and ports, because
    /// it comes from the device's own descriptor.
    pub id: String,
    /// Its place among the parts present, counting from one, in the order
    /// Windows numbers them.
    pub index: u32,
    pub count: u32,
}

/// What sectioning needs to know about each device in a scan.
#[derive(Debug, Clone, Copy)]
pub struct ScannedIdentity<'a> {
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<&'a str>,
    pub instance_path: &'a str,
}

/// The interface and collection numbers in a HID interface path —
/// `mi_01&col03` — if it has any. Case is normalised, because Windows is not
/// consistent about it between APIs.
pub fn section_id(instance_path: &str) -> Option<String> {
    let lower = instance_path.to_ascii_lowercase();
    let (_, ids, _) = split_path(&lower)?;
    let parts: Vec<&str> = ids.split('&').filter(|t| is_section_token(t)).collect();
    (!parts.is_empty()).then(|| parts.join("&"))
}

/// Which physical device a scanned controller belongs to.
///
/// A serial number settles it: two collections with the same serial are one
/// device. Without one, the interface path has to. The parts of one device
/// share everything in the path except the interface and collection numbers
/// and the final index, while two separate un-serialled devices differ in the
/// instance hash between — so the path with those stripped is the family.
pub fn family_key(vid: u16, pid: u16, serial: Option<&str>, path: Option<&str>) -> String {
    if let Some(serial) = serial.map(str::trim).filter(|s| !s.is_empty()) {
        return format!("serial:{vid:04x}:{pid:04x}:{serial}");
    }
    match path.map(str::trim).filter(|s| !s.is_empty()) {
        Some(path) => format!("path:{}", stem(path)),
        None => format!("model:{vid:04x}:{pid:04x}"),
    }
}

/// Each scanned device's part, or `None` for a device Windows lists once.
///
/// Parts are numbered in the order of their section IDs, which is the order
/// Windows numbers the collections. Two devices that merely share a serial
/// but have no interface or collection numbers to tell them apart — cheap
/// hardware with a serial that is not unique — are not parts of anything,
/// and are left alone.
pub fn sections(scanned: &[ScannedIdentity]) -> Vec<Option<DeviceSection>> {
    let mut families: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, s) in scanned.iter().enumerate() {
        let key = family_key(s.vid, s.pid, s.serial, Some(s.instance_path));
        families.entry(key).or_default().push(i);
    }

    let mut out: Vec<Option<DeviceSection>> = vec![None; scanned.len()];
    for members in families.values() {
        if members.len() < 2 {
            continue;
        }
        let mut ordered: Vec<(String, usize)> = Vec::with_capacity(members.len());
        for &i in members {
            let Some(id) = section_id(scanned[i].instance_path) else {
                // No collection number, so nothing says which part this is.
                ordered.clear();
                break;
            };
            ordered.push((id, i));
        }
        ordered.sort();
        ordered.dedup_by(|a, b| a.0 == b.0);
        if ordered.len() != members.len() {
            continue;
        }
        let count = ordered.len() as u32;
        for (n, (id, i)) in ordered.into_iter().enumerate() {
            out[i] = Some(DeviceSection {
                id,
                index: n as u32 + 1,
                count,
            });
        }
    }
    out
}

/// Which mode a device is in, for the devices that have one.
///
/// Asetek's steering wheels are the known case: the ones seen so far are
/// `0xF4xx`, with wheelbases at `0xF1xx` and pedals at `0xF3xx`, and only the
/// wheels have the legacy input mode. A wheel in several parts is in it; a
/// wheel in one part is not. Anything else is in no mode, whatever its
/// parts, because "legacy mode" is a name RaceHub uses and not a fact about
/// every device that has more than one collection.
pub fn mode_of(vid: u16, pid: u16, section: Option<&DeviceSection>) -> Option<DeviceMode> {
    let has_modes = vid == ASETEK_VID && (pid & 0xFF00) == 0xF400;
    has_modes.then(|| {
        if section.is_some() {
            DeviceMode::Legacy
        } else {
            DeviceMode::Normal
        }
    })
}

/// Drop the parts of a device that belong to the mode it is not in.
///
/// When a wheel leaves legacy mode, three of its four parts stop being
/// reported and one carries on. The presence rule shows a device that has
/// stopped being reported as gone rather than removing it — right for an
/// unplugged pedal set, wrong here: nothing was unplugged, and three red rows
/// under a wheel that is working is a page that cries wolf. So, within one
/// family, when the parts that are present say which mode the device is in,
/// the absent rows that belong to the other mode are removed. A part that is
/// absent while its siblings are still present is a different thing: that
/// one really is gone, and it stays red.
pub fn without_other_mode(published: Vec<DetectedDevice>) -> Vec<DetectedDevice> {
    // Per family: whether any present member is a part of something.
    let mut present: BTreeMap<String, bool> = BTreeMap::new();
    for d in published.iter().filter(|d| d.hid_present) {
        let split = present.entry(family_of(d)).or_insert(false);
        *split |= d.section.is_some();
    }

    published
        .into_iter()
        .filter(|d| {
            if d.hid_present {
                return true;
            }
            match present.get(&family_of(d)) {
                // Nothing of it is present: unplugged, and shown as such.
                None => true,
                Some(&present_split) => d.section.is_some() == present_split,
            }
        })
        .collect()
}

fn family_of(d: &DetectedDevice) -> String {
    family_key(
        d.device.vid,
        d.device.pid,
        d.device.serial.as_deref(),
        d.device.instance_path.as_deref(),
    )
}

fn is_section_token(token: &str) -> bool {
    token.starts_with("mi_") || token.starts_with("col")
}

/// `\\?\hid#vid_2433&pid_f402&mi_01&col03#b&77c5cda&0&0002#{guid}` as
/// (everything up to and including `hid#`, the identifiers, the rest).
fn split_path(lower: &str) -> Option<(&str, &str, &str)> {
    let start = lower.find("hid#")? + 4;
    let (head, rest) = lower.split_at(start);
    let (ids, tail) = match rest.find('#') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    Some((head, ids, tail))
}

/// The path with the section numbers and the final index removed: what the
/// parts of one un-serialled device have in common and two devices do not.
fn stem(instance_path: &str) -> String {
    let lower = instance_path.to_ascii_lowercase();
    let Some((head, ids, tail)) = split_path(&lower) else {
        return lower;
    };
    let kept: Vec<&str> = ids.split('&').filter(|t| !is_section_token(t)).collect();
    // `#b&77c5cda&0&0002#{guid}`: the instance, without its last index or
    // the interface class.
    let instance = tail.split('#').nth(1).unwrap_or("");
    let instance = match instance.rfind('&') {
        Some(i) => &instance[..i],
        None => instance,
    };
    format!("{head}{}#{instance}", kept.join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;

    static WHEEL: [&str; 4] = [
        r"\\?\hid#vid_2433&pid_f402&mi_01&col01#b&77c5cda&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}",
        r"\\?\hid#vid_2433&pid_f402&mi_01&col03#b&77c5cda&0&0002#{4d1e55b2-f16f-11cf-88cb-001111000030}",
        r"\\?\hid#vid_2433&pid_f402&mi_01&col04#b&77c5cda&0&0003#{4d1e55b2-f16f-11cf-88cb-001111000030}",
        r"\\?\hid#vid_2433&pid_f402&mi_01&col05#b&77c5cda&0&0004#{4d1e55b2-f16f-11cf-88cb-001111000030}",
    ];
    const BASE: &str =
        r"\\?\hid#vid_2433&pid_f104&col01#6&ab45d67&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
    const PEDALS: &str =
        r"\\?\hid#vid_2433&pid_f303#9&223705d3&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";

    fn asetek<'a>(pid: u16, serial: Option<&'a str>, path: &'a str) -> ScannedIdentity<'a> {
        ScannedIdentity {
            vid: ASETEK_VID,
            pid,
            serial,
            instance_path: path,
        }
    }

    #[test]
    fn the_section_is_the_interface_and_collection() {
        assert_eq!(section_id(WHEEL[1]).as_deref(), Some("mi_01&col03"));
        assert_eq!(section_id(BASE).as_deref(), Some("col01"));
        assert_eq!(section_id(PEDALS), None);
        assert_eq!(
            section_id(r"\\?\HID#VID_2433&PID_F402&MI_01&COL03#B&77C5CDA&0&0002").as_deref(),
            Some("mi_01&col03"),
            "case is Windows' choice, not the device's"
        );
    }

    #[test]
    fn a_wheel_in_legacy_mode_is_four_parts_of_one_device() {
        let scan: Vec<ScannedIdentity> = WHEEL
            .iter()
            .map(|p| asetek(0xF402, Some("E081000993"), p))
            .collect();
        let parts = sections(&scan);
        let ids: Vec<(String, u32, u32)> = parts
            .iter()
            .map(|part| part.clone().map(|s| (s.id, s.index, s.count)).unwrap())
            .collect();
        assert_eq!(
            ids,
            vec![
                ("mi_01&col01".into(), 1, 4),
                ("mi_01&col03".into(), 2, 4),
                ("mi_01&col04".into(), 3, 4),
                ("mi_01&col05".into(), 4, 4),
            ]
        );
    }

    #[test]
    fn parts_are_numbered_by_collection_whatever_order_windows_lists_them_in() {
        let scan = vec![
            asetek(0xF402, Some("E081000993"), WHEEL[3]),
            asetek(0xF402, Some("E081000993"), WHEEL[0]),
        ];
        let parts = sections(&scan);
        assert_eq!(parts[0].as_ref().map(|s| s.index), Some(2));
        assert_eq!(parts[1].as_ref().map(|s| s.index), Some(1));
    }

    #[test]
    fn a_device_listed_once_is_not_a_part_of_anything() {
        // The wheel in normal mode, beside the base and the pedals.
        let scan = vec![
            asetek(0xF402, Some("E081000993"), WHEEL[0]),
            asetek(0xF104, Some("A1"), BASE),
            asetek(0xF303, Some("P1"), PEDALS),
        ];
        assert!(sections(&scan).iter().all(Option::is_none));
    }

    #[test]
    fn un_serialled_parts_are_grouped_by_their_shared_path() {
        let a = r"\\?\hid#vid_1234&pid_0001&col01#7&1a2b3c&0&0000#{g}";
        let b = r"\\?\hid#vid_1234&pid_0001&col02#7&1a2b3c&0&0001#{g}";
        // The same model, plugged into another port: a second device.
        let other = r"\\?\hid#vid_1234&pid_0001&col01#7&9f9f9f&0&0000#{g}";
        let scan = vec![
            ScannedIdentity {
                vid: 0x1234,
                pid: 1,
                serial: None,
                instance_path: a,
            },
            ScannedIdentity {
                vid: 0x1234,
                pid: 1,
                serial: None,
                instance_path: b,
            },
            ScannedIdentity {
                vid: 0x1234,
                pid: 1,
                serial: None,
                instance_path: other,
            },
        ];
        let parts = sections(&scan);
        let place = |i: usize| parts[i].as_ref().map(|s| (s.index, s.count));
        assert_eq!(place(0), Some((1, 2)));
        assert_eq!(place(1), Some((2, 2)));
        assert_eq!(parts[2], None, "another instance is another device");
    }

    #[test]
    fn two_devices_sharing_a_serial_with_nothing_to_tell_them_apart_are_left_alone() {
        // Cheap hardware ships with a serial that is not unique. Two of them
        // are two devices, not two parts, and there is no collection number
        // to say which would be which.
        let a = r"\\?\hid#vid_1234&pid_0001#7&1a2b3c&0&0000#{g}";
        let b = r"\\?\hid#vid_1234&pid_0001#7&9f9f9f&0&0000#{g}";
        let scan = vec![
            ScannedIdentity {
                vid: 0x1234,
                pid: 1,
                serial: Some("12345"),
                instance_path: a,
            },
            ScannedIdentity {
                vid: 0x1234,
                pid: 1,
                serial: Some("12345"),
                instance_path: b,
            },
        ];
        assert!(sections(&scan).iter().all(Option::is_none));
    }

    #[test]
    fn only_asetek_wheels_have_a_mode() {
        let part = DeviceSection {
            id: "mi_01&col03".into(),
            index: 2,
            count: 4,
        };
        let legacy = Some(DeviceMode::Legacy);
        let normal = Some(DeviceMode::Normal);
        assert_eq!(mode_of(ASETEK_VID, 0xF402, Some(&part)), legacy);
        assert_eq!(mode_of(ASETEK_VID, 0xF402, None), normal);
        // The base and the pedals have no such mode, and saying "normal" of
        // them would invite the question of what the other one is.
        assert_eq!(mode_of(ASETEK_VID, 0xF104, None), None);
        assert_eq!(mode_of(ASETEK_VID, 0xF303, None), None);
        // Another maker's split device is in parts, not in a mode.
        assert_eq!(mode_of(0x1234, 0x0001, Some(&part)), None);
    }

    use crate::{DetectedDevice, DeviceRef, DeviceStatus};

    fn wheel_part(path: &str, section: Option<DeviceSection>, present: bool) -> DetectedDevice {
        DetectedDevice {
            device: DeviceRef {
                vid: ASETEK_VID,
                pid: 0xF402,
                serial: Some("E081000993".into()),
                instance_path: Some(path.into()),
                display_name: "Forte Formula Pro wheel".into(),
            },
            manufacturer: None,
            raw_product_name: None,
            status: if present {
                DeviceStatus::Connected
            } else {
                DeviceStatus::Disconnected
            },
            hid_present: present,
            dinput_present: present,
            dinput_slot: None,
            dinput_instance_guid: None,
            is_virtual: false,
            vjoy: None,
            binding_drift: None,
            alias_key: String::new(),
            renamed: false,
            mode: mode_of(ASETEK_VID, 0xF402, section.as_ref()),
            section,
        }
    }

    fn part(id: &str, index: u32, count: u32) -> Option<DeviceSection> {
        Some(DeviceSection {
            id: id.into(),
            index,
            count,
        })
    }

    #[test]
    fn leaving_legacy_mode_does_not_leave_three_red_rows() {
        // The wheel went back to normal mode: one part carries on, alone and
        // therefore no longer a part, and the other three stopped being
        // reported. They are not disconnected. They do not exist in this mode.
        let published = vec![
            wheel_part(WHEEL[0], None, true),
            wheel_part(WHEEL[1], part("mi_01&col03", 2, 4), false),
            wheel_part(WHEEL[2], part("mi_01&col04", 3, 4), false),
            wheel_part(WHEEL[3], part("mi_01&col05", 4, 4), false),
        ];
        let shown = without_other_mode(published);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].device.instance_path.as_deref(), Some(WHEEL[0]));
    }

    #[test]
    fn a_part_that_drops_out_in_legacy_mode_really_is_gone() {
        // Three of four parts present: the fourth is missing, not in another
        // mode, and it stays on the page as disconnected.
        let published = vec![
            wheel_part(WHEEL[0], part("mi_01&col01", 1, 3), true),
            wheel_part(WHEEL[1], part("mi_01&col03", 2, 4), false),
            wheel_part(WHEEL[2], part("mi_01&col04", 2, 3), true),
            wheel_part(WHEEL[3], part("mi_01&col05", 3, 3), true),
        ];
        let shown = without_other_mode(published);
        assert_eq!(shown.len(), 4);
        assert_eq!(shown[1].status, DeviceStatus::Disconnected);
    }

    #[test]
    fn an_unplugged_device_is_still_shown_as_gone() {
        // Nothing of the family is present, so this is the ordinary case and
        // the ordinary rule — shown red, not removed — stands.
        let published = vec![wheel_part(WHEEL[0], None, false)];
        assert_eq!(without_other_mode(published).len(), 1);
    }

    #[test]
    fn a_stale_whole_device_row_goes_when_the_parts_arrive() {
        // The other direction, for a device whose single-mode path is not one
        // of its part paths: the parts are present, the old whole-device row
        // is not, and it belongs to the mode the device has left.
        let whole = r"\\?\hid#vid_2433&pid_f402#b&77c5cda&0&0000#{g}";
        let published = vec![
            wheel_part(whole, None, false),
            wheel_part(WHEEL[0], part("mi_01&col01", 1, 2), true),
            wheel_part(WHEEL[1], part("mi_01&col03", 2, 2), true),
        ];
        let shown = without_other_mode(published);
        assert_eq!(shown.len(), 2);
        assert!(shown.iter().all(|d| d.section.is_some()));
    }
}
