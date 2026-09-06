//! The rig model — the single source of truth for the physical setup.
//!
//! See docs/design/0003-rig-model.md. Two distinct ideas live here and must not
//! be conflated:
//!
//! * [`RigModel`] is what the user *authors* — "the left panel is angled 55 deg
//!   inward, there is a 12 mm bezel". Human terms.
//! * The solved output (poses, frusta, FOV) lives in `tp-geometry` and is
//!   *derived*. It is never stored in the rig.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::units::{Deg, LengthUnit, Measurement, Mm, PixelRect, Resolution};

/// serde `default` for the reserved pose fields.
fn deg_zero() -> Deg {
    Deg::ZERO
}

/// Bumped whenever the persisted shape changes. Loading a file with a higher
/// version is refused outright rather than partially parsed.
pub const RIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RigModel {
    pub schema_version: u32,
    pub id: Uuid,
    /// Incremented on every save. Profiles record the revision they were last
    /// computed against, which is what drives recompute-and-propagate.
    pub revision: u64,
    pub name: String,
    /// ISO-8601. A string rather than a timestamp type so the TypeScript
    /// binding is unambiguous and no date library is implied.
    pub updated_at: String,
    /// UI preference only. Never the source of truth for any stored value.
    pub units_preference: LengthUnit,
    pub seating: Seating,
    /// Ordered left to right as physically arranged.
    pub screens: Vec<ScreenSpec>,
}

impl RigModel {
    pub fn new(name: impl Into<String>, updated_at: impl Into<String>) -> Self {
        Self {
            schema_version: RIG_SCHEMA_VERSION,
            id: Uuid::new_v4(),
            revision: 1,
            name: name.into(),
            updated_at: updated_at.into(),
            units_preference: LengthUnit::default(),
            seating: Seating::default(),
            screens: Vec::new(),
        }
    }

    pub fn screen(&self, id: ScreenId) -> Option<&ScreenSpec> {
        self.screens.iter().find(|s| s.id == id)
    }

    pub fn center(&self) -> Option<&ScreenSpec> {
        self.screens.iter().find(|s| s.role == ScreenRole::Center)
    }
}

/// Where the driver's eyes are. The origin of the whole coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Seating {
    /// Eye to the *nearest point* of the centre screen's visible surface. On a
    /// curved panel that is the middle of the curve; the chord-plane distance
    /// is derived from it, not measured.
    pub eye_to_center: Mm,
    /// Positive = eye above the centre screen's vertical centre.
    pub eye_height_offset: Mm,
    /// Positive = eye to the right of the centre screen's horizontal centre.
    pub lateral_offset: Mm,
}

impl Default for Seating {
    fn default() -> Self {
        Self {
            eye_to_center: Mm(700.0),
            eye_height_offset: Mm::ZERO,
            lateral_offset: Mm::ZERO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
#[ts(type = "number")]
#[serde(transparent)]
pub struct ScreenId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind",
    content = "label"
)]
pub enum ScreenRole {
    Center,
    Left,
    Right,
    /// A fourth panel, a stacked dash screen, a button-box display. Extensible
    /// on purpose — plenty of rigs are not exactly three screens.
    Auxiliary(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSpec {
    pub id: ScreenId,
    pub role: ScreenRole,
    pub binding: MonitorBinding,
    pub panel: PanelSpec,
    pub mounting: MountingSpec,
}

/// How a rig screen is tied to a physically detected monitor.
///
/// Note this binds by EDID identity, *not* by device path. The CCD device path
/// encodes the adapter and output, so moving a cable from DP-1 to DP-2 changes
/// it and would silently turn your left screen into your centre screen. The
/// monitor's own EDID identity is what actually survives a re-cable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum MonitorBinding {
    Edid(EdidIdentity),
    /// Rig screen described but not yet matched to hardware. Legitimate state:
    /// you can build a profile with a monitor unplugged.
    Unbound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct EdidIdentity {
    /// Three-character PNP vendor ID, EDID bytes 0x08..0x0A.
    pub manufacturer_id: String,
    /// EDID bytes 0x0A..0x0C.
    pub product_code: u16,
    /// Descriptor 0xFF when the panel bothers to provide one. Many do not.
    pub serial: Option<String>,
    /// EDID bytes 0x0C..0x10. Weaker than the string serial but usually present.
    pub serial_number: u32,
    /// Manufacture week and year. Last-resort tiebreaker between two otherwise
    /// identical un-serialled panels.
    pub week_year: (u8, u16),
    /// A fast path only, re-validated on every scan. Never trusted alone.
    pub cached_device_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PanelSpec {
    pub native_resolution: Resolution,
    /// The visible *image* area, not the outer chassis.
    pub visible_width: Measurement,
    pub visible_height: Measurement,
    /// Whether `visible_width` was entered as arc (developed) or chord length.
    /// Only meaningful when `curvature` is not `Flat`.
    pub width_measure: WidthMeasure,
    pub curvature: Curvature,
    pub bezel: Bezel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum WidthMeasure {
    /// Developed surface length — what a tape measure laid on the screen gives,
    /// and usually what a datasheet quotes. The default for curved panels.
    #[default]
    Arc,
    /// Straight-line distance between the visible left and right edges.
    Chord,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Curvature {
    Flat,
    /// `radius` is the R number: 1000R is `Mm(1000.0)`.
    Radius {
        radius: Mm,
    },
}

impl Curvature {
    pub fn radius(&self) -> Option<Mm> {
        match self {
            Curvature::Flat => None,
            Curvature::Radius { radius } => Some(*radius),
        }
    }
}

/// Per-edge bezel: the dark border between the visible image and the outside of
/// the chassis. Measured per edge because left/right routinely differ from
/// top/bottom, and the two side panels of a rig are often not a matched pair.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Bezel {
    pub left: Mm,
    pub right: Mm,
    pub top: Mm,
    pub bottom: Mm,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MountingSpec {
    /// Yaw relative to the centre screen plane. Positive = angled inward,
    /// toward the driver. A coplanar screen is 0.
    pub angle: Deg,
    /// Extra physical space between this screen and its inboard neighbour,
    /// *beyond* both adjacent bezels. Defined this way so that correcting a
    /// bezel measurement does not silently change the mount gap.
    pub gap: Mm,
    /// Positive = this screen's centre sits above the centre screen's centre.
    pub vertical_offset: Mm,
    /// Eye to the centre of this screen's surface. `None` derives it from the
    /// centre distance and the geometry, which is right for a normal mount.
    pub distance_override: Option<Mm>,
    /// Reserved. Defaults to 0 and is not exposed in the v1 UI. Present in the
    /// schema from the start because adding a field to a persisted format later
    /// costs a migration, and reserving two f64 now costs nothing.
    #[serde(default = "deg_zero")]
    pub pitch: Deg,
    /// Reserved. See `pitch`.
    #[serde(default = "deg_zero")]
    pub roll: Deg,
}

impl Default for MountingSpec {
    fn default() -> Self {
        Self {
            angle: Deg::ZERO,
            gap: Mm::ZERO,
            vertical_offset: Mm::ZERO,
            distance_override: None,
            pitch: Deg::ZERO,
            roll: Deg::ZERO,
        }
    }
}

/// How a game session uses the rig. Derived per profile, never typed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SessionMode {
    /// Game on the centre panel only; side screens left free for SimHub.
    CenterOnly,
    /// One surface across every screen.
    FullSpan { fit: SpanFit },
    /// An arbitrary rectangle drawn in the layout editor.
    CustomRect { rect: PixelRect },
}

/// How to reconcile unequal panel heights when spanning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum SpanFit {
    Letterbox,
    Crop,
    Stretch,
}
