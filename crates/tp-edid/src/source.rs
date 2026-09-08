//! Locating a monitor's EDID blob on the machine.
//!
//! Windows exposes EDID through no display API at all. It lives in the
//! registry, under the device instance that owns the monitor:
//!
//! ```text
//! HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\<hardware id>\<instance>\Device Parameters
//!     EDID  (REG_BINARY)
//! ```
//!
//! `WmiMonitorBasicDisplayParams` also reports physical size, but only in whole
//! centimetres — a 1193 mm panel comes back as 119. The raw blob carries
//! millimetres in its detailed timing descriptor, which is the difference
//! between a computed FOV that is right and one that is a few tenths of a
//! degree off with nothing saying so.
//!
//! Deriving the key is a pure string transform, so it lives here and is tested
//! everywhere. Only the registry read itself is Windows-only, and that stays in
//! the app crate.

/// Turn a CCD monitor device path into the registry key holding its EDID.
///
/// The device path looks like:
///
/// ```text
/// \\?\DISPLAY#SAM7179#5&1234abcd&0&UID4353#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}
///          ^hardware id  ^instance id
/// ```
///
/// The two middle `#`-separated fields are exactly the two path components
/// under `Enum\DISPLAY`.
pub fn registry_key_for(monitor_device_path: &str) -> Option<String> {
    let mut parts = monitor_device_path.split('#');

    // The leading component is `\\?\DISPLAY` or `\\.\DISPLAY`; anything else
    // is not a monitor interface path and must not be guessed at.
    let prefix = parts.next()?;
    if !prefix.to_ascii_uppercase().ends_with("DISPLAY") {
        return None;
    }

    let hardware_id = parts.next().filter(|s| !s.is_empty())?;
    let instance_id = parts.next().filter(|s| !s.is_empty())?;

    // A path component containing a separator would let a malformed device
    // path walk to an unrelated registry key.
    if [hardware_id, instance_id]
        .iter()
        .any(|s| s.contains('\\') || s.contains('/'))
    {
        return None;
    }

    Some(format!(
        r"SYSTEM\CurrentControlSet\Enum\DISPLAY\{hardware_id}\{instance_id}\Device Parameters"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_the_key_from_a_device_path() {
        let path =
            r"\\?\DISPLAY#SAM7179#5&1234abcd&0&UID4353#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}";
        assert_eq!(
            registry_key_for(path).as_deref(),
            Some(
                r"SYSTEM\CurrentControlSet\Enum\DISPLAY\SAM7179\5&1234abcd&0&UID4353\Device Parameters"
            )
        );
    }

    #[test]
    fn accepts_the_dot_form_too() {
        let path = r"\\.\DISPLAY#DELA1CF#4&abc&0&UID256#{guid}";
        assert!(registry_key_for(path)
            .unwrap()
            .contains(r"DELA1CF\4&abc&0&UID256"));
    }

    #[test]
    fn refuses_anything_that_is_not_a_monitor_path() {
        // A GDI adapter name, not a monitor interface path.
        assert_eq!(registry_key_for(r"\\.\DISPLAY1"), None);
        assert_eq!(registry_key_for(""), None);
        assert_eq!(registry_key_for(r"\\?\USB#VID_1234#inst#{guid}"), None);
        // Present prefix but nothing after it.
        assert_eq!(registry_key_for(r"\\?\DISPLAY#"), None);
        assert_eq!(registry_key_for(r"\\?\DISPLAY#SAM7179#"), None);
    }

    #[test]
    fn refuses_a_path_that_would_escape_the_key() {
        // A device path is not attacker-controlled in practice, but building a
        // registry path by string concatenation without checking is how that
        // stops being true.
        assert_eq!(registry_key_for(r"\\?\DISPLAY#..\..\..#inst#{guid}"), None);
        assert_eq!(registry_key_for(r"\\?\DISPLAY#SAM#a/b#{guid}"), None);
    }
}
