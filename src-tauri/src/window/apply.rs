//! Applying geometry, and proving it worked.
//!
//! The important part of this module is not `SetWindowPos`. It is everything
//! after it.

use tp_model::{FrameInsets, PixelRect, RectMeans};

use crate::error::{AppError, AppResult};

/// What a window looks like right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    pub outer: PixelRect,
    pub client: PixelRect,
    pub style: u32,
    pub ex_style: u32,
}

impl WindowGeometry {
    /// The frame this window currently carries.
    pub fn insets(&self) -> FrameInsets {
        FrameInsets {
            left: self.client.x - self.outer.x,
            top: self.client.y - self.outer.y,
            right: self.outer.right() - self.client.right(),
            bottom: self.outer.bottom() - self.client.bottom(),
        }
    }
}

/// The result of an apply, after verification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppliedWindow {
    pub requested: PixelRect,
    pub actual: WindowGeometry,
    pub borderless: bool,
}

#[cfg(windows)]
pub fn read_geometry(hwnd: u64) -> AppResult<WindowGeometry> {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::*;

    let handle = HWND(hwnd as *mut core::ffi::c_void);

    // SAFETY: `handle` is a window handle supplied by enumeration; every call
    // below is a read.
    unsafe {
        let mut outer = RECT::default();
        GetWindowRect(handle, &mut outer).map_err(|e| win32("GetWindowRect", &e))?;

        let mut client = RECT::default();
        GetClientRect(handle, &mut client).map_err(|e| win32("GetClientRect", &e))?;

        // GetClientRect is window-relative; move it into screen space so the
        // frame insets can be computed by subtraction.
        let mut origin = POINT {
            x: client.left,
            y: client.top,
        };
        let _ = ClientToScreen(handle, &mut origin);

        Ok(WindowGeometry {
            outer: rect_to_pixels(outer),
            client: PixelRect {
                x: origin.x,
                y: origin.y,
                width: (client.right - client.left).max(0) as u32,
                height: (client.bottom - client.top).max(0) as u32,
            },
            style: GetWindowLongPtrW(handle, GWL_STYLE) as u32,
            ex_style: GetWindowLongPtrW(handle, GWL_EXSTYLE) as u32,
        })
    }
}

/// Strip the frame if asked, move and size the window, then check.
///
/// `rect` is interpreted per `means`. That distinction is not cosmetic: asking
/// for a 5120x1440 client area and setting it as the window rectangle leaves
/// the client short by the frame, which on a triple rig puts the horizon seam
/// in the wrong place on every screen.
#[cfg(windows)]
pub fn apply_geometry(
    hwnd: u64,
    rect: PixelRect,
    means: RectMeans,
    borderless: bool,
    always_on_top: bool,
) -> AppResult<AppliedWindow> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::*;

    let handle = HWND(hwnd as *mut core::ffi::c_void);
    let before = read_geometry(hwnd)?;

    // SAFETY: every call takes a valid HWND. Return values are deliberately
    // ignored — see the verification below for why they cannot be trusted.
    unsafe {
        if borderless {
            let style = tp_model::borderless_style(before.style);
            let ex_style = tp_model::borderless_ex_style(before.ex_style);
            // The return values are not checked, and deliberately so:
            // SetWindowLongPtrW returns the *previous* style on success and
            // zero on failure, and zero is a legitimate previous style. Only
            // the read-back below can tell the difference.
            SetWindowLongPtrW(handle, GWL_STYLE, style as isize);
            SetWindowLongPtrW(handle, GWL_EXSTYLE, ex_style as isize);
        }

        // Once the style has changed the frame is a different size, so the
        // outer rectangle a client request implies has to be recomputed from
        // what the window will be, not from what it was.
        let insets = if borderless {
            FrameInsets::default()
        } else {
            before.insets()
        };
        let outer = match means {
            RectMeans::ClientArea => tp_model::outer_from_client(rect, insets),
            RectMeans::OuterWindow => rect,
        };

        let z = if always_on_top {
            HWND_TOPMOST
        } else {
            HWND_TOP
        };
        let mut flags = SWP_FRAMECHANGED | SWP_NOACTIVATE;
        if !always_on_top {
            flags |= SWP_NOZORDER;
        }

        let _ = SetWindowPos(
            handle,
            Some(z),
            outer.x,
            outer.y,
            outer.width as i32,
            outer.height as i32,
            flags,
        );
    }

    verify(hwnd, rect, means, borderless)
}

/// The whole point of this module.
///
/// UIPI refuses these calls silently when the target runs at a higher integrity
/// level, and no return value distinguishes that from success. So the only way
/// to know is to look.
#[cfg(windows)]
fn verify(
    hwnd: u64,
    requested: PixelRect,
    means: RectMeans,
    borderless: bool,
) -> AppResult<AppliedWindow> {
    let after = read_geometry(hwnd)?;
    let actual = match means {
        RectMeans::ClientArea => after.client,
        RectMeans::OuterWindow => after.outer,
    };

    // A pixel of disagreement is the OS, not a failure. Anything more means the
    // change did not take.
    let placed = !tp_model::has_drifted(actual, requested, 2);
    let styled = !borderless || tp_model::is_borderless(after.style, after.ex_style);

    if !placed || !styled {
        // Distinguish the case with a remedy from the case without one. A game
        // running elevated is the overwhelmingly common cause, and "relaunch
        // Team Principal as administrator" is something the user can act on.
        return Err(AppError::ElevationRequired {
            operation: format!(
                "positioning the game window (asked for {}x{} at {},{}, got {}x{} at {},{}{})",
                requested.width,
                requested.height,
                requested.x,
                requested.y,
                actual.width,
                actual.height,
                actual.x,
                actual.y,
                if styled {
                    ""
                } else {
                    "; the frame is still present"
                }
            ),
        });
    }

    Ok(AppliedWindow {
        requested,
        actual: after,
        borderless: tp_model::is_borderless(after.style, after.ex_style),
    })
}

#[cfg(windows)]
fn rect_to_pixels(r: windows::Win32::Foundation::RECT) -> PixelRect {
    PixelRect {
        x: r.left,
        y: r.top,
        width: (r.right - r.left).max(0) as u32,
        height: (r.bottom - r.top).max(0) as u32,
    }
}

#[cfg(windows)]
fn win32(operation: &str, e: &windows::core::Error) -> AppError {
    AppError::Win32 {
        operation: operation.to_string(),
        code: e.code().0 as u32,
        message: e.message(),
    }
}

#[cfg(not(windows))]
pub fn read_geometry(_hwnd: u64) -> AppResult<WindowGeometry> {
    Err(AppError::Config("window control requires Windows".into()))
}

#[cfg(not(windows))]
pub fn apply_geometry(
    _hwnd: u64,
    _rect: PixelRect,
    _means: RectMeans,
    _borderless: bool,
    _always_on_top: bool,
) -> AppResult<AppliedWindow> {
    Err(AppError::Config("window control requires Windows".into()))
}
