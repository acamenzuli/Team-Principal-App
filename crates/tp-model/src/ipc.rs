//! Types that exist only to cross the IPC boundary.
//!
//! These live in `tp-model` rather than in the Tauri crate on purpose: this
//! crate compiles on any platform, so the TypeScript bindings — and the CI job
//! that checks they have not drifted — do not need a Windows runner or a
//! webview toolchain.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::PixelRect;

/// Everything the UI needs to describe the running app, on the dashboard and
/// in a support ticket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// Which milestone this build represents. Shown in the UI so a bug report
    /// says what was actually built, not what was planned.
    pub milestone: u8,
    /// True when running against fixtures. The UI shows a persistent banner so
    /// a screenshot can never be mistaken for real hardware.
    pub simulated: bool,
    pub dpi_awareness: String,
    pub dpi_awareness_ok: bool,
    pub log_dir: String,
}

/// A length parsed from whatever the user typed, rendered in every unit so the
/// UI can show normalised helper text under the field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLength {
    pub mm: f64,
    pub formatted_mm: String,
    pub formatted_cm: String,
    pub formatted_inch: String,
}

/// A curved panel resolved into what a flat-plane sim can consume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CurveResult {
    pub arc_mm: f64,
    pub chord_mm: f64,
    pub sagitta_mm: f64,
    pub subtended_deg: f64,
    pub chord_plane_distance_mm: f64,
    /// The correct answer: chord width at the chord-plane distance.
    pub h_fov_deg: f64,
    /// What you get by naively treating the arc as a flat width. Returned
    /// alongside the correct value so the UI can show the gap rather than
    /// assert it — on a 49" 1000R panel it is over 15 degrees.
    pub naive_h_fov_deg: f64,
    pub worst_case_error_deg: Option<f64>,
}

/// The wire shape of every error the frontend can see. A raw OS error code
/// never reaches the UI; each variant carries enough to write a sentence a
/// person can act on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub kind: String,
    pub message: String,
    /// What the user can do about it, when there is something. Drives the
    /// inline fix button on a failed preflight row.
    pub remedy: Option<Remedy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Remedy {
    /// The operation was refused by UIPI because the target runs at a higher
    /// integrity level. Specific because it has a specific fix.
    RelaunchElevated,
    Retry,
    OpenSettings {
        section: String,
    },
}

/// The virtual desktop's shape, as the layout editor needs to draw it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLayoutInfo {
    /// The bounding box of every monitor, in physical pixels. Its origin is
    /// negative whenever a monitor sits left of or above the primary.
    pub bounds: PixelRect,
    /// Parts of `bounds` that map to no physical panel. A window placed here is
    /// addressable and invisible.
    pub dead_regions: Vec<PixelRect>,
    /// Areas as f64 rather than u64: they cross into JavaScript, where an
    /// integer beyond 2^53 would silently lose precision. A desktop would have
    /// to be about 95 megapixels square to get near that, but `bigint` in the
    /// binding for no reason is worse.
    pub covered_area: f64,
    pub dead_area: f64,
    pub is_gapless: bool,
    /// Per-monitor pixel pitch, in the same order as `list_monitors`.
    pub pitches: Vec<MonitorPitch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MonitorPitch {
    pub device_path: String,
    /// `None` when the monitor reports no physical size. The UI must ask for a
    /// measurement rather than assume a pitch.
    pub px_per_mm: Option<f64>,
    /// True when this monitor's pitch differs from its left-hand neighbour's by
    /// more than 10%. Where that holds, a bezel gap cannot be expressed in
    /// pixels across the seam at all, and the app says so instead of returning
    /// a number that is quietly wrong.
    pub differs_from_neighbour: bool,
}
