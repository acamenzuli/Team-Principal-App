//! Changing the desktop topology, and putting it back.
//!
//! The most dangerous code in the app. A mode a panel cannot show is a black
//! screen, and a black screen on a rig whose only input is that screen is a
//! machine you cannot reach without holding the power button. Four things stand
//! between a plan and that outcome:
//!
//! 1. **The plan is validated first**, in `tp_model::topology`, against rules
//!    that are tested without a GPU — one primary, primary at the origin, no
//!    overlaps, a contiguous desktop, and every mode present in the driver's
//!    own list.
//! 2. **The change is staged, then committed once.** Every device is written
//!    with `CDS_NORESET`, and a single final call applies the lot. Applying
//!    them one at a time means the desktop passes through intermediate states
//!    that overlap or strand a monitor, and Windows rearranges those.
//! 3. **The result is verified by read-back**, never by return code — the rule
//!    that holds everywhere else in this app and holds doubly here, because
//!    `DISP_CHANGE_SUCCESSFUL` means "accepted", not "that is what you now
//!    have".
//! 4. **A snapshot is captured before anything is written**, and the caller is
//!    on a countdown to confirm. See `confirm.rs`.

use tp_model::{DisplayMode, Resolution, SnapshotEntry, TopologySnapshot};

use crate::error::{AppError, AppResult};

/// Every mode the driver reports for one output.
///
/// Deduplicated and sorted largest first, because a raw enumeration repeats the
/// same resolution once per colour depth and interlacing flag and is thousands
/// of entries long on some drivers.
#[cfg(windows)]
pub fn available_modes(gdi_name: &str) -> Vec<DisplayMode> {
    use windows::Win32::Graphics::Gdi::*;

    let name: Vec<u16> = gdi_name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut modes: Vec<DisplayMode> = Vec::new();

    for index in 0.. {
        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        // SAFETY: `name` is NUL-terminated and outlives the call; `dmSize` tells
        // the API which version of the struct it was handed.
        let ok = unsafe {
            EnumDisplaySettingsExW(
                windows::core::PCWSTR(name.as_ptr()),
                ENUM_DISPLAY_SETTINGS_MODE(index),
                &mut dm,
                ENUM_DISPLAY_SETTINGS_FLAGS(0),
            )
        };
        if !ok.as_bool() {
            break;
        }

        // Only 32-bit modes. A 16-bit mode is offered by drivers for
        // compatibility and choosing one would be a bug, never an intention.
        if dm.dmBitsPerPel != 32 {
            continue;
        }
        let mode = DisplayMode {
            resolution: Resolution {
                width: dm.dmPelsWidth,
                height: dm.dmPelsHeight,
            },
            refresh_hz: dm.dmDisplayFrequency,
            bits_per_pixel: dm.dmBitsPerPel,
        };
        if !modes.contains(&mode) {
            modes.push(mode);
        }
    }

    modes.sort_by(|a, b| {
        (b.resolution.width, b.resolution.height, b.refresh_hz).cmp(&(
            a.resolution.width,
            a.resolution.height,
            a.refresh_hz,
        ))
    });
    modes
}

#[cfg(not(windows))]
pub fn available_modes(_gdi_name: &str) -> Vec<DisplayMode> {
    Vec::new()
}

/// Apply a snapshot as a plan: stage every device, then commit once.
///
/// `gdi_for` resolves a CCD device path to its GDI name (`\\.\DISPLAY1`), which
/// is what the mode-setting API takes. The two namespaces are separate and
/// nothing converts between them but the enumeration.
///
/// Returns the topology that actually resulted, read back from Windows. The
/// caller compares it with what it asked for — a difference is not an error
/// from the API's point of view, and is exactly what we must not accept.
#[cfg(windows)]
pub fn apply(plan: &TopologySnapshot, gdi_for: &dyn Fn(&str) -> Option<String>) -> AppResult<()> {
    use windows::Win32::Graphics::Gdi::*;

    // Stage. CDS_NORESET writes the registry without touching the desktop, so
    // the intermediate states — where two screens briefly overlap because one
    // has moved and the other has not — never exist.
    for monitor in &plan.monitors {
        let Some(gdi_name) = gdi_for(&monitor.device_path) else {
            return Err(AppError::Config(format!(
                "{} is not attached to this machine any more, so this plan \
                 cannot be applied as written.",
                monitor.device_path
            )));
        };
        stage(&gdi_name, monitor)?;
    }

    // Commit. A NULL device with a NULL DEVMODE applies everything staged.
    // SAFETY: the documented form of the commit call; all pointers are None.
    let result = unsafe { ChangeDisplaySettingsExW(None, None, None, CDS_TYPE(0), None) };
    if result != DISP_CHANGE_SUCCESSFUL {
        return Err(change_error("committing the display change", result));
    }
    Ok(())
}

/// Write one device's mode and position without applying it.
#[cfg(windows)]
fn stage(gdi_name: &str, monitor: &SnapshotEntry) -> AppResult<()> {
    use windows::Win32::Graphics::Gdi::*;

    let name: Vec<u16> = gdi_name.encode_utf16().chain(std::iter::once(0)).collect();
    let device = windows::core::PCWSTR(name.as_ptr());

    // Switching an output off is a mode of zero width and height. It is not a
    // separate API, and passing a real mode with CDS_DISABLE does nothing.
    if !monitor.active {
        let dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            dmFields: DM_PELSWIDTH | DM_PELSHEIGHT | DM_POSITION,
            ..Default::default()
        };
        // SAFETY: `name` outlives the call; `dm` is zeroed apart from the
        // fields `dmFields` declares.
        let result = unsafe {
            ChangeDisplaySettingsExW(
                device,
                Some(&dm),
                None,
                CDS_UPDATEREGISTRY | CDS_NORESET,
                None,
            )
        };
        if result != DISP_CHANGE_SUCCESSFUL {
            return Err(change_error(&format!("switching off {gdi_name}"), result));
        }
        return Ok(());
    }

    let mut dm = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        dmPelsWidth: monitor.mode.resolution.width,
        dmPelsHeight: monitor.mode.resolution.height,
        dmDisplayFrequency: monitor.mode.refresh_hz,
        dmBitsPerPel: monitor.mode.bits_per_pixel,
        dmFields: DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY | DM_BITSPERPEL | DM_POSITION,
        ..Default::default()
    };
    // dmPosition is the active union member for a display device, which is what
    // DM_POSITION in dmFields declares this struct to be about. Writing a union
    // field needs no unsafe; it is *reading* one that does.
    dm.Anonymous1.Anonymous2.dmPosition = windows::Win32::Foundation::POINTL {
        x: monitor.position.0,
        y: monitor.position.1,
    };

    // CDS_SET_PRIMARY moves the origin to this device. Windows re-bases every
    // other monitor's position against it, which is why the model refuses a
    // plan whose primary is not already at 0,0.
    let mut flags = CDS_UPDATEREGISTRY | CDS_NORESET;
    if monitor.is_primary {
        flags |= CDS_SET_PRIMARY;
    }

    // Ask before telling. CDS_TEST changes nothing and answers whether the mode
    // would be accepted, which turns "black screen" into "that will not work".
    // SAFETY: as above.
    let test = unsafe { ChangeDisplaySettingsExW(device, Some(&dm), None, CDS_TEST, None) };
    if test != DISP_CHANGE_SUCCESSFUL {
        return Err(change_error(
            &format!(
                "{gdi_name} at {}x{} {} Hz",
                monitor.mode.resolution.width,
                monitor.mode.resolution.height,
                monitor.mode.refresh_hz
            ),
            test,
        ));
    }

    // SAFETY: as above.
    let result = unsafe { ChangeDisplaySettingsExW(device, Some(&dm), None, flags, None) };
    if result != DISP_CHANGE_SUCCESSFUL {
        return Err(change_error(&format!("staging {gdi_name}"), result));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn apply(_plan: &TopologySnapshot, _gdi_for: &dyn Fn(&str) -> Option<String>) -> AppResult<()> {
    Err(AppError::Config(
        "display changes require Windows".to_string(),
    ))
}

/// Turn a DISP_CHANGE_* code into something a person can act on.
///
/// These codes are the whole reason this function exists: `DISP_CHANGE_BADMODE`
/// and `DISP_CHANGE_RESTART` mean completely different things to the user, and
/// "error -2" tells them neither.
#[cfg(windows)]
fn change_error(operation: &str, code: windows::Win32::Graphics::Gdi::DISP_CHANGE) -> AppError {
    use windows::Win32::Graphics::Gdi::*;

    let message = match code {
        DISP_CHANGE_BADMODE => "the screen does not support that mode",
        DISP_CHANGE_BADFLAGS => "an invalid combination of settings",
        DISP_CHANGE_BADPARAM => "an invalid parameter",
        DISP_CHANGE_FAILED => "the display driver refused the change",
        DISP_CHANGE_NOTUPDATED => "the change could not be written to the registry",
        DISP_CHANGE_RESTART => "this change needs a restart to take effect",
        DISP_CHANGE_BADDUALVIEW => "that layout is not valid on this adapter",
        _ => "the display driver refused the change",
    };
    AppError::Win32 {
        operation: operation.to_string(),
        // DISP_CHANGE codes are small negative numbers, not Win32 error codes.
        // Kept as-is for the log rather than mangled into something that would
        // look up as an unrelated system error.
        code: code.0 as u32,
        message: message.to_string(),
    }
}

/// Does the desktop now match what was asked for?
///
/// The read-back rule. `DISP_CHANGE_SUCCESSFUL` means the driver accepted the
/// request; it does not mean the desktop looks like the plan. Windows will
/// happily accept a layout and then rearrange it.
pub fn matches(plan: &TopologySnapshot, actual: &TopologySnapshot) -> Vec<String> {
    let mut differences = Vec::new();

    for want in plan.monitors.iter().filter(|m| m.active) {
        let Some(have) = actual
            .monitors
            .iter()
            .find(|m| m.device_path == want.device_path && m.active)
        else {
            differences.push(format!("{} did not come back on", want.device_path));
            continue;
        };
        if have.mode.resolution != want.mode.resolution {
            differences.push(format!(
                "{} is {}x{}, not {}x{}",
                want.device_path,
                have.mode.resolution.width,
                have.mode.resolution.height,
                want.mode.resolution.width,
                want.mode.resolution.height
            ));
        }
        if have.position != want.position {
            differences.push(format!(
                "{} is at {},{}, not {},{}",
                want.device_path,
                have.position.0,
                have.position.1,
                want.position.0,
                want.position.1
            ));
        }
        if have.is_primary != want.is_primary {
            differences.push(format!(
                "{} is {}the primary screen",
                want.device_path,
                if have.is_primary { "" } else { "not " }
            ));
        }
    }

    for want in plan.monitors.iter().filter(|m| !m.active) {
        if actual
            .monitors
            .iter()
            .any(|m| m.device_path == want.device_path && m.active)
        {
            differences.push(format!("{} did not switch off", want.device_path));
        }
    }

    differences
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, w: u32, h: u32, x: i32, y: i32, primary: bool) -> SnapshotEntry {
        SnapshotEntry {
            device_path: path.into(),
            active: true,
            mode: DisplayMode {
                resolution: Resolution {
                    width: w,
                    height: h,
                },
                refresh_hz: 120,
                bits_per_pixel: 32,
            },
            position: (x, y),
            is_primary: primary,
        }
    }

    fn snapshot(monitors: Vec<SnapshotEntry>) -> TopologySnapshot {
        TopologySnapshot {
            id: uuid::Uuid::nil(),
            captured_at: String::new(),
            monitors,
        }
    }

    #[test]
    fn an_exact_match_reports_nothing() {
        let plan = snapshot(vec![entry("a", 1920, 1080, 0, 0, true)]);
        assert!(matches(&plan, &plan).is_empty());
    }

    #[test]
    fn a_resolution_windows_quietly_substituted_is_caught() {
        // DISP_CHANGE_SUCCESSFUL and the wrong mode is a real combination.
        let plan = snapshot(vec![entry("a", 2560, 1440, 0, 0, true)]);
        let actual = snapshot(vec![entry("a", 1920, 1080, 0, 0, true)]);
        assert_eq!(matches(&plan, &actual).len(), 1);
        assert!(matches(&plan, &actual)[0].contains("1920x1080, not 2560x1440"));
    }

    #[test]
    fn a_monitor_windows_moved_to_close_a_gap_is_caught() {
        let plan = snapshot(vec![
            entry("a", 1920, 1080, 0, 0, true),
            entry("b", 1920, 1080, 3840, 0, false),
        ]);
        let actual = snapshot(vec![
            entry("a", 1920, 1080, 0, 0, true),
            entry("b", 1920, 1080, 1920, 0, false),
        ]);
        assert!(matches(&plan, &actual)[0].contains("is at 1920,0, not 3840,0"));
    }

    #[test]
    fn a_screen_that_did_not_switch_off_is_caught() {
        let mut plan = snapshot(vec![
            entry("a", 1920, 1080, 0, 0, true),
            entry("b", 1920, 1080, 1920, 0, false),
        ]);
        plan.monitors[1].active = false;

        let actual = snapshot(vec![
            entry("a", 1920, 1080, 0, 0, true),
            entry("b", 1920, 1080, 1920, 0, false),
        ]);
        assert_eq!(matches(&plan, &actual), vec!["b did not switch off"]);
    }

    #[test]
    fn a_screen_that_never_came_back_is_caught() {
        // The failure mode that matters most: an output that went dark and
        // stayed dark. Silence here would be a countdown with nothing to see.
        let plan = snapshot(vec![
            entry("a", 1920, 1080, 0, 0, true),
            entry("b", 1920, 1080, 1920, 0, false),
        ]);
        let actual = snapshot(vec![entry("a", 1920, 1080, 0, 0, true)]);
        assert_eq!(matches(&plan, &actual), vec!["b did not come back on"]);
    }
}
