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
