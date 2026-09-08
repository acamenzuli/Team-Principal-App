//! Reading a device's identity out of a DirectInput product GUID.
//!
//! DirectInput reports two GUIDs per device. The *instance* GUID identifies
//! this particular device on this machine and is what a game stores when you
//! bind a control. The *product* GUID identifies the model — and for any HID
//! device Windows builds it to a fixed pattern:
//!
//! ```text
//! {PPPPVVVV-0000-0000-0000-504944564944}
//!  ^^^^ PID  ^^^^ VID              "PIDVID" in ASCII
//! ```
//!
//! That is what lets a DirectInput entry be matched to the HID device it came
//! from without guessing by name — names are frequently identical across a pair
//! of pedals, and frequently useless ("HID-compliant game controller").
//!
//! Pure bit work, so it is tested here rather than only on a machine with a
//! wheel plugged into it.

/// The trailing bytes Windows uses for every HID-derived product GUID: the
/// ASCII "PIDVID".
pub const PIDVID_TAIL: [u8; 8] = [0x00, 0x00, 0x50, 0x49, 0x44, 0x56, 0x49, 0x44];

/// Extract VID and PID from a DirectInput product GUID.
///
/// Returns `None` for a GUID that does not follow the HID pattern — a virtual
/// or legacy device, where inventing a VID/PID would silently mis-match it to
/// somebody else's hardware.
pub fn vid_pid_from_product_guid(
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
) -> Option<(u16, u16)> {
    if data2 != 0 || data3 != 0 || data4 != PIDVID_TAIL {
        return None;
    }
    let vid = (data1 & 0xFFFF) as u16;
    let pid = ((data1 >> 16) & 0xFFFF) as u16;
    Some((vid, pid))
}

/// Format an instance GUID the way Windows does, for storing in a profile and
/// comparing later.
pub fn format_guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        data1,
        data2,
        data3,
        data4[0],
        data4[1],
        data4[2],
        data4[3],
        data4[4],
        data4[5],
        data4[6],
        data4[7]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Fanatec wheelbase: VID 0x0EB7, PID 0x0E04.
    const FANATEC_DATA1: u32 = 0x0E04_0EB7;

    #[test]
    fn extracts_vid_and_pid_from_a_real_product_guid() {
        let (vid, pid) = vid_pid_from_product_guid(FANATEC_DATA1, 0, 0, PIDVID_TAIL).unwrap();
        assert_eq!(vid, 0x0EB7, "VID is the low half");
        assert_eq!(pid, 0x0E04, "PID is the high half");
    }

    #[test]
    fn handles_a_vjoy_device() {
        let (vid, pid) = vid_pid_from_product_guid(0xBEAD_1234, 0, 0, PIDVID_TAIL).unwrap();
        assert_eq!((vid, pid), (0x1234, 0xBEAD));
    }

    #[test]
    fn refuses_a_guid_that_is_not_hid_derived() {
        // Getting this wrong would match a legacy or virtual device to whatever
        // hardware happens to share those bytes.
        assert_eq!(
            vid_pid_from_product_guid(FANATEC_DATA1, 1, 0, PIDVID_TAIL),
            None
        );
        assert_eq!(
            vid_pid_from_product_guid(FANATEC_DATA1, 0, 9, PIDVID_TAIL),
            None
        );
        assert_eq!(
            vid_pid_from_product_guid(FANATEC_DATA1, 0, 0, [0; 8]),
            None,
            "the PIDVID tail is what makes the pattern recognisable"
        );
    }

    #[test]
    fn the_tail_really_spells_pidvid() {
        // Documents where the magic constant comes from, so nobody has to
        // wonder whether it was transcribed correctly.
        assert_eq!(&PIDVID_TAIL[2..], b"PIDVID");
    }

    #[test]
    fn guids_format_the_way_windows_writes_them() {
        let s = format_guid(
            0x0E04_0EB7,
            0x0000,
            0x0000,
            [0x00, 0x00, 0x50, 0x49, 0x44, 0x56, 0x49, 0x44],
        );
        assert_eq!(s, "{0E040EB7-0000-0000-0000-504944564944}");
    }

    #[test]
    fn a_formatted_guid_is_stable_for_comparison() {
        // Drift detection compares these as strings, so the same GUID must
        // always render identically.
        let a = format_guid(1, 2, 3, [4, 5, 6, 7, 8, 9, 10, 11]);
        let b = format_guid(1, 2, 3, [4, 5, 6, 7, 8, 9, 10, 11]);
        assert_eq!(a, b);
        assert_ne!(a, format_guid(1, 2, 3, [4, 5, 6, 7, 8, 9, 10, 12]));
    }
}
