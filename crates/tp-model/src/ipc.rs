//! Types that exist only to cross the IPC boundary.
//!
//! These live in `tp-model` rather than in the Tauri crate on purpose: this
//! crate compiles on any platform, so the TypeScript bindings — and the CI job
//! that checks they have not drifted — do not need a Windows runner or a
//! webview toolchain.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{PixelRect, Preferences, ScreenId, ScreenRole, WindowCandidate};

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

/// Preferences, plus a note if the stored file could not be used.
///
/// The note travels in the payload rather than as an error because a broken
/// preferences file must not stop the app opening. It starts on defaults and
/// says what happened, instead of silently discarding someone's settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LoadedPreferences {
    pub preferences: Preferences,
    pub problem: Option<String>,
    /// Black or white, whichever is legible on the chosen accent. Computed in
    /// Rust and covered by tests, so a custom colour can never produce a button
    /// whose label cannot be read.
    pub accent_foreground: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AccentPreset {
    pub name: String,
    pub hex: String,
    pub foreground: String,
}

// --------------------------------------------------------- solved geometry

/// A solved rig, flattened for the UI.
///
/// `tp-geometry` owns the real types; these are the wire shapes. Keeping them
/// here rather than deriving serde in the geometry crate keeps that crate about
/// mathematics, and keeps every TypeScript binding generated from one place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RigSolutionInfo {
    pub screens: Vec<ScreenSolutionInfo>,
    /// Outer edge to outer edge, bezel gaps included.
    pub total_coverage_deg: f64,
    /// Sum of the per-screen spans, gaps excluded.
    pub visible_coverage_deg: f64,
    pub warnings: Vec<RigWarningInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSolutionInfo {
    pub id: ScreenId,
    pub role: ScreenRole,
    pub distance_mm: f64,
    /// What a flat-projecting sim should be told. For a curved panel this is
    /// the chord width at the chord-plane distance, not the arc.
    pub flat_width_mm: f64,
    pub flat_height_mm: f64,
    /// The symmetric equivalent: `|left| + |right|`.
    pub h_fov_deg: f64,
    pub v_fov_deg: f64,
    /// The asymmetric truth. Any yawed screen or lateral seating offset makes
    /// left and right differ, and a single half-angle would hide it.
    pub span: SpanInfo,
    pub px_per_deg_h: f64,
    pub px_per_deg_v: f64,
    pub inner_gap: Option<GapInfo>,
    pub curvature_error_deg: Option<f64>,
    /// Centre of the visible surface in the eye frame, millimetres.
    /// `[x right, y up, z forward]`. The schematic draws straight from this.
    pub centre: [f64; 3],
    /// Visible corners in the same frame: inboard-bottom, inboard-top,
    /// outboard-bottom, outboard-top.
    pub corners: [[f64; 3]; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SpanInfo {
    pub left_deg: f64,
    pub right_deg: f64,
    pub bottom_deg: f64,
    pub top_deg: f64,
    /// How far off-centre the screen sits. Above about 1 degree the UI shows
    /// the asymmetry rather than only the symmetric equivalent.
    pub asymmetry_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct GapInfo {
    /// The physical dark band to the inboard neighbour: both bezels plus the
    /// mount gap.
    pub mm: f64,
    /// `None` when the panels either side have different pixel pitch, where
    /// there is no single pixel size the gap could have.
    pub px: Option<f64>,
    pub deg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RigWarningInfo {
    /// A stable slug, for styling and for tests. The UI renders `message`.
    pub kind: String,
    /// One sentence saying what is wrong and what it means, written for a
    /// person rather than a log.
    pub message: String,
    pub screen_id: Option<ScreenId>,
}

/// A single set of triple-screen values fitted to a rig that is not uniform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BestFitInfo {
    pub width_mm: f64,
    pub height_mm: f64,
    pub bezel_mm: f64,
    pub distance_mm: f64,
    pub angle_deg: f64,
    pub worst_error_deg: f64,
    /// True when the rig is uniform enough that this is exact rather than a
    /// compromise. The UI must never present a fit as exact when it is not.
    pub is_exact: bool,
    pub residuals: Vec<ResidualInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ResidualInfo {
    pub id: ScreenId,
    pub max_error_deg: f64,
    pub rms_error_deg: f64,
}

// ------------------------------------------------------------ input monitor

/// One frame of live input from a device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct InputFrame {
    /// The device interface path, matching `DeviceRef.instancePath`.
    pub instance_path: String,
    pub axes: Vec<AxisReading>,
    /// One entry per declared button, in report order, so the UI can draw a
    /// stable grid rather than a list that changes length as buttons are held.
    pub buttons: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AxisReading {
    pub name: String,
    /// -1.0 to 1.0.
    pub value: f64,
    /// 0.0 to 1.0. What a pedal wants: at rest is empty, not half full.
    pub unipolar: f64,
}

// ----------------------------------------------------------- window control

/// The outcome of placing a window, after it has been read back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WindowResult {
    pub hwnd: String,
    pub title: String,
    /// What identified this window as the game, so a wrong pick is diagnosable.
    pub matched_by: Vec<String>,
    pub requested: PixelRect,
    /// Read back after the change, never assumed from a return value.
    pub actual_outer: PixelRect,
    pub actual_client: PixelRect,
    pub borderless: bool,
    /// True while the watchdog is putting the window back when the game moves it.
    pub watching: bool,
}

/// A window the user can pick from, for the test panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct OpenWindow {
    pub candidate: WindowCandidate,
    /// True when this one would survive the splash-and-tool-window filters.
    pub plausible: bool,
}

// ------------------------------------------------------------- game discovery

/// A game found on disk, with what it takes to start it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct InstalledGameInfo {
    pub name: String,
    pub install_path: String,
    /// "steam" or "epic". The launcher it belongs to.
    pub launcher: String,
    /// The URI that starts it. Protocol launches return no process handle,
    /// which is why the window matcher works from the executable name instead.
    pub launch_uri: String,
    /// Whether an adapter exists for this title yet. Adapters arrive in
    /// milestone 10, so this is honest rather than aspirational.
    pub has_adapter: bool,
}
