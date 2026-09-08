//! What the display layer reports upward. Platform-neutral on purpose: the
//! Win32 implementation and the mock both produce these.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::rig::EdidIdentity;
use crate::units::{Mm, PixelRect, Resolution};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    /// CCD target device path. Stable while the cable stays in the same port.
    pub device_path: String,
    /// GDI name, e.g. `\\.\DISPLAY1`.
    pub gdi_name: String,
    pub friendly_name: String,
    pub identity: EdidIdentity,
    pub native_resolution: Resolution,
    pub current_mode: DisplayMode,
    /// Position and size in virtual-desktop physical pixels. Signed: monitors
    /// left of or above the primary have negative origins.
    pub bounds: PixelRect,
    pub is_primary: bool,
    /// Windows scaling as a factor: 1.0 for 100%, 1.5 for 150%.
    pub dpi_scale: f64,
    /// Physical visible area from EDID. `None` when the panel reports nothing
    /// usable, which happens — the UI then asks for a manual measurement rather
    /// than inventing a number.
    pub physical_size: Option<PhysicalSize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalSize {
    pub width: Mm,
    pub height: Mm,
    /// True when parsed from the EDID detailed timing descriptor (millimetre
    /// precision) rather than EDID bytes 0x15/0x16 (whole centimetres, so a
    /// 1193 mm panel reports 119 and quantises to +/-5 mm).
    pub millimetre_precision: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DisplayMode {
    pub resolution: Resolution,
    pub refresh_hz: u32,
    pub bits_per_pixel: u32,
}

/// A complete topology snapshot: enough to put the desktop back exactly as it
/// was. Captured before any change and restorable by the panic hotkey.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TopologySnapshot {
    pub id: uuid::Uuid,
    pub captured_at: String,
    pub monitors: Vec<SnapshotEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    pub device_path: String,
    pub active: bool,
    pub mode: DisplayMode,
    pub position: (i32, i32),
    pub is_primary: bool,
}

/// A region of virtual-desktop space that maps to no physical panel. Real on
/// any rig with mismatched panel heights, and the layout editor draws them so
/// you can see where a window would vanish.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DeadRegion {
    pub rect: PixelRect,
}

/// Whether the panic hotkey is armed, and what it is.
///
/// `registered` is read back from Windows rather than assumed: another program
/// may already own the combination, and a safety net the user believes in but
/// that does nothing is worse than none at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyInfo {
    pub combination: String,
    pub registered: bool,
}

/// Where a display change stands while the user decides.
///
/// The countdown is owned by the backend, never by the UI: if the change made
/// the screen unreadable there is nobody to run a timer in the frontend, and
/// silence has to mean revert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmState {
    /// Seconds left before the change is undone. Zero once it has settled.
    pub seconds_left: u32,
    pub outcome: Option<ConfirmOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmOutcome {
    /// The user said keep it.
    Kept,
    /// The countdown ran out. The default, and deliberately so.
    RevertedOnTimeout,
    /// The user asked for it back, or pressed the panic hotkey.
    RevertedOnRequest,
    /// The desktop did not end up matching the plan, so it was put back without
    /// waiting. Nothing was asked of the user, because the answer was already
    /// no.
    RevertedOnMismatch,
    /// Putting it back failed. The worst outcome there is, and it is reported
    /// rather than swallowed.
    RevertFailed,
}
