//! Real monitor enumeration.
//!
//! Three APIs, each for the one thing it is actually good at:
//!
//! | Source | What it gives |
//! | --- | --- |
//! | CCD (`QueryDisplayConfig`) | the stable device path, the friendly name, and the source-to-target mapping |
//! | `EnumDisplaySettingsExW` | the mode the panel is *currently* running, and its position in virtual desktop space |
//! | EDID via the registry | physical size in millimetres, and the identity a rig screen binds to |
//!
//! None of them gives all three. In particular EDID cannot be trusted for
//! resolution — its detailed timing fields cap at 4095 pixels and 655.35 MHz,
//! so a 5120x1440 panel at 240 Hz cannot express its native mode there at all.
//! Windows owns resolution; EDID owns physical size and identity.
//!
//! Read-only. Nothing here changes a display setting; that is milestone 9,
//! behind the confirm-or-revert flow.

#[cfg(windows)]
use tp_model::{DisplayMode, EdidIdentity, SnapshotEntry};
use tp_model::{MonitorInfo, TopologySnapshot};

#[cfg(not(windows))]
use crate::error::AppError;
use crate::error::AppResult;

#[cfg(windows)]
pub fn enumerate_monitors() -> AppResult<Vec<MonitorInfo>> {
    use std::collections::HashMap;

    let targets = win::query_display_config()?;
    let mut out = Vec::with_capacity(targets.len());

    // GDI adapter name -> the CCD target attached to it.
    let by_gdi: HashMap<String, win::CcdTarget> = targets
        .into_iter()
        .map(|t| (t.gdi_name.clone(), t))
        .collect();

    for (gdi_name, target) in by_gdi {
        let Some(mode) = win::current_mode(&gdi_name) else {
            // An active CCD path with no readable mode is odd but not fatal:
            // skip the monitor and say so rather than failing the whole scan
            // and leaving the user with no displays at all.
            tracing::warn!(%gdi_name, "no current mode; skipping this monitor");
            continue;
        };

        let edid =
            super::edid_source::read_edid(&target.device_path).and_then(
                |blob| match tp_edid::parse(&blob) {
                    Ok(e) => Some(e),
                    Err(e) => {
                        tracing::warn!(%gdi_name, error = %e, "EDID present but unparseable");
                        None
                    }
                },
            );

        if let Some(e) = &edid {
            if !e.checksum_ok {
                // Not fatal: shipping monitors get this wrong, and refusing the
                // panel would make the user's screen silently absent.
                tracing::warn!(%gdi_name, "EDID checksum is wrong; using it anyway");
            }
        }

        out.push(build_monitor_info(&gdi_name, &target, mode, edid));
    }

    // Left to right, so the UI order matches the physical rig without anyone
    // having to sort it later.
    out.sort_by_key(|m| (m.bounds.x, m.bounds.y));
    Ok(out)
}

#[cfg(not(windows))]
pub fn enumerate_monitors() -> AppResult<Vec<MonitorInfo>> {
    Err(AppError::Config(
        "real monitor enumeration requires Windows; pass --mock <fixture.json>".into(),
    ))
}

/// Assemble one monitor from the three sources.
///
/// Split out from the Win32 walk so the merge rules — which source wins for
/// which field, and what happens when EDID is missing — are one readable
/// function rather than being buried in unsafe code.
#[cfg(windows)]
fn build_monitor_info(
    gdi_name: &str,
    target: &win::CcdTarget,
    mode: win::CurrentMode,
    edid: Option<tp_edid::Edid>,
) -> MonitorInfo {
    let identity = edid
        .as_ref()
        .map(|e| e.identity(Some(target.device_path.clone())))
        .unwrap_or_else(|| unknown_identity(&target.device_path));

    // CCD's friendly name is usually the real model. When it is absent or the
    // generic placeholder, EDID's monitor-name descriptor is better.
    let friendly_name = [
        target
            .friendly_name
            .as_deref()
            .filter(|n| !is_placeholder(n)),
        edid.as_ref().and_then(|e| e.monitor_name.as_deref()),
        target.friendly_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .next()
    .unwrap_or("Unknown display")
    .to_string();

    MonitorInfo {
        device_path: target.device_path.clone(),
        gdi_name: gdi_name.to_string(),
        friendly_name,
        identity,
        // EDID cannot be trusted for this; see the module docs.
        native_resolution: mode.resolution,
        current_mode: DisplayMode {
            resolution: mode.resolution,
            refresh_hz: mode.refresh_hz,
            bits_per_pixel: mode.bits_per_pixel,
        },
        bounds: mode.bounds,
        is_primary: mode.bounds.x == 0 && mode.bounds.y == 0,
        dpi_scale: mode.dpi_scale,
        physical_size: edid.as_ref().and_then(|e| e.physical_size_model()),
    }
}

/// "Generic PnP Monitor" and friends tell the user nothing.
#[cfg(windows)]
fn is_placeholder(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n.is_empty()
        || n.contains("generic pnp")
        || n.contains("default monitor")
        || n == "generic non-pnp monitor"
}

/// A stand-in identity for a display with no readable EDID.
///
/// Deliberately not blank: the device path still distinguishes this monitor
/// from its neighbours within a session, so the app stays usable. It will be
/// flagged as unbindable, because it cannot survive a re-cable.
#[cfg(windows)]
fn unknown_identity(device_path: &str) -> EdidIdentity {
    EdidIdentity {
        manufacturer_id: "???".into(),
        product_code: 0,
        serial: None,
        serial_number: 0,
        week_year: (0, 0),
        cached_device_path: Some(device_path.to_string()),
    }
}

#[cfg(windows)]
pub fn capture_snapshot(monitors: &[MonitorInfo]) -> AppResult<TopologySnapshot> {
    Ok(TopologySnapshot {
        id: uuid::Uuid::new_v4(),
        captured_at: crate::now_iso8601(),
        monitors: monitors
            .iter()
            .map(|m| SnapshotEntry {
                device_path: m.device_path.clone(),
                active: true,
                mode: m.current_mode,
                position: (m.bounds.x, m.bounds.y),
                is_primary: m.is_primary,
            })
            .collect(),
    })
}

#[cfg(not(windows))]
pub fn capture_snapshot(_monitors: &[MonitorInfo]) -> AppResult<TopologySnapshot> {
    Err(AppError::Config("snapshots require Windows".into()))
}

#[cfg(windows)]
mod win {
    //! The Win32 calls, kept as small and as flat as they can be.

    use tp_model::{PixelRect, Resolution};
    use windows::Win32::Devices::Display::*;
    use windows::Win32::Foundation::{ERROR_SUCCESS, POINT, WIN32_ERROR};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    use crate::error::{AppError, AppResult};

    pub struct CcdTarget {
        /// `\\.\DISPLAY1`
        pub gdi_name: String,
        /// `\\?\DISPLAY#SAM7179#5&...#{guid}` — what EDID lookup needs.
        pub device_path: String,
        pub friendly_name: Option<String>,
    }

    pub struct CurrentMode {
        pub resolution: Resolution,
        pub refresh_hz: u32,
        pub bits_per_pixel: u32,
        pub bounds: PixelRect,
        pub dpi_scale: f64,
    }

    fn win32(operation: &str, code: WIN32_ERROR) -> AppError {
        AppError::Win32 {
            operation: operation.to_string(),
            code: code.0,
            message: std::io::Error::from_raw_os_error(code.0 as i32).to_string(),
        }
    }

    /// Walk the active CCD paths, resolving each to a GDI name and device path.
    pub fn query_display_config() -> AppResult<Vec<CcdTarget>> {
        let mut path_count = 0u32;
        let mut mode_count = 0u32;

        // SAFETY: both out-params are valid for the call's duration.
        let status = unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        };
        if status != ERROR_SUCCESS {
            return Err(win32("GetDisplayConfigBufferSizes", status));
        }

        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];

        // SAFETY: the buffers are sized by the call above and the counts passed
        // in match their lengths; QueryDisplayConfig writes at most that many
        // entries and updates the counts to what it wrote.
        let status = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(win32("QueryDisplayConfig", status));
        }
        paths.truncate(path_count as usize);

        let mut out = Vec::with_capacity(paths.len());
        for path in &paths {
            let Some(source) = source_name(path) else {
                continue;
            };
            let Some(target) = target_name(path) else {
                continue;
            };
            out.push(CcdTarget {
                gdi_name: source,
                device_path: target.0,
                friendly_name: target.1,
            });
        }
        Ok(out)
    }

    /// `DISPLAYCONFIG_SOURCE_DEVICE_NAME` -> the GDI adapter name.
    fn source_name(path: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
        let mut req = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            ..Default::default()
        };
        // SAFETY: the header's `size` field describes this exact struct, which
        // is how DisplayConfigGetDeviceInfo knows what it was handed.
        let status = unsafe { DisplayConfigGetDeviceInfo(&mut req.header) };
        (status == ERROR_SUCCESS.0 as i32)
            .then(|| wide_to_string(&req.viewGdiDeviceName))?
            .into()
    }

    /// `DISPLAYCONFIG_TARGET_DEVICE_NAME` -> the monitor device path and name.
    fn target_name(path: &DISPLAYCONFIG_PATH_INFO) -> Option<(String, Option<String>)> {
        let mut req = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                size: std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            ..Default::default()
        };
        // SAFETY: as above.
        let status = unsafe { DisplayConfigGetDeviceInfo(&mut req.header) };
        if status != ERROR_SUCCESS.0 as i32 {
            return None;
        }
        let device_path = wide_to_string(&req.monitorDevicePath);
        if device_path.is_empty() {
            return None;
        }
        let friendly = wide_to_string(&req.monitorFriendlyDeviceName);
        Some((device_path, (!friendly.is_empty()).then_some(friendly)))
    }

    /// The mode the adapter is actually running, plus its DPI scale.
    pub fn current_mode(gdi_name: &str) -> Option<CurrentMode> {
        let name: Vec<u16> = gdi_name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };

        // SAFETY: `name` is NUL-terminated and outlives the call; `dm.dmSize`
        // tells the API the struct version it was handed.
        let ok = unsafe {
            EnumDisplaySettingsExW(
                windows::core::PCWSTR(name.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut dm,
                windows::Win32::Graphics::Gdi::ENUM_DISPLAY_SETTINGS_FLAGS(0),
            )
        };
        if !ok.as_bool() {
            return None;
        }

        // dmPosition lives in a union; it is the meaningful member for a
        // display device, which is what we just asked about.
        // SAFETY: `dmPosition` is the active union member for ENUM_CURRENT_SETTINGS
        // on a display device.
        let pos = unsafe { dm.Anonymous1.Anonymous2.dmPosition };

        let bounds = PixelRect {
            x: pos.x,
            y: pos.y,
            width: dm.dmPelsWidth,
            height: dm.dmPelsHeight,
        };

        Some(CurrentMode {
            resolution: Resolution {
                width: dm.dmPelsWidth,
                height: dm.dmPelsHeight,
            },
            refresh_hz: dm.dmDisplayFrequency,
            bits_per_pixel: dm.dmBitsPerPel,
            bounds,
            dpi_scale: dpi_scale_at(pos.x, pos.y),
        })
    }

    /// Effective DPI of the monitor containing a point, as a scale factor.
    ///
    /// Meaningful only because the process is Per-Monitor V2 aware. Under any
    /// lesser awareness this reports the system DPI for every monitor and the
    /// mixed-DPI case silently produces wrong rectangles.
    fn dpi_scale_at(x: i32, y: i32) -> f64 {
        // A point one pixel inside the monitor, so a rectangle's exact corner
        // does not land on the neighbour.
        let point = POINT { x: x + 1, y: y + 1 };

        // SAFETY: MonitorFromPoint always returns a monitor handle with
        // MONITOR_DEFAULTTONEAREST, and GetDpiForMonitor's out-params are valid.
        unsafe {
            let hmon = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            match GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) {
                Ok(()) => dpi_x as f64 / 96.0,
                Err(e) => {
                    tracing::warn!(error = %e, "GetDpiForMonitor failed; assuming 100%");
                    1.0
                }
            }
        }
    }

    /// A fixed-size WCHAR array, NUL-terminated or not.
    fn wide_to_string(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end]).trim().to_string()
    }
}
