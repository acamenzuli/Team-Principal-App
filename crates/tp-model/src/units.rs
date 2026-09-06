//! Units. Everything internal is millimetres and degrees, as `f64`.
//!
//! These are newtypes rather than bare `f64` so that a millimetre can never be
//! passed where a degree is expected. The compiler catches the class of bug
//! that is otherwise found by staring at a wrong FOV number for an hour.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A length in millimetres. The only length unit that exists in the model.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[ts(type = "number")]
#[serde(transparent)]
pub struct Mm(pub f64);

/// An angle in degrees. Stored in degrees because that is what the user types
/// and what game config files want; converted to radians only inside math.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[ts(type = "number")]
#[serde(transparent)]
pub struct Deg(pub f64);

impl Mm {
    pub const ZERO: Mm = Mm(0.0);
    pub fn inches(self) -> f64 {
        self.0 / 25.4
    }
    pub fn cm(self) -> f64 {
        self.0 / 10.0
    }
    pub fn from_inches(v: f64) -> Mm {
        Mm(v * 25.4)
    }
    pub fn from_cm(v: f64) -> Mm {
        Mm(v * 10.0)
    }
}

impl Deg {
    pub const ZERO: Deg = Deg(0.0);
    pub fn radians(self) -> f64 {
        self.0.to_radians()
    }
    pub fn from_radians(r: f64) -> Deg {
        Deg(r.to_degrees())
    }
}

/// A display preference only. The stored model is always millimetres; this
/// affects the input/display boundary and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Mm,
    Cm,
    Inch,
}

/// Where a measurement came from. Shown in the UI so you can tell an
/// auto-detected value from one you measured yourself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementSource {
    /// Read from the monitor's EDID.
    Edid,
    /// Typed in by the user, overriding whatever was detected.
    Manual,
    /// Computed from other fields (e.g. chord width derived from arc + radius).
    Derived,
}

/// A length plus its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    pub mm: Mm,
    pub source: MeasurementSource,
}

impl Measurement {
    pub fn edid(mm: f64) -> Self {
        Self {
            mm: Mm(mm),
            source: MeasurementSource::Edid,
        }
    }
    pub fn manual(mm: f64) -> Self {
        Self {
            mm: Mm(mm),
            source: MeasurementSource::Manual,
        }
    }
    pub fn derived(mm: f64) -> Self {
        Self {
            mm: Mm(mm),
            source: MeasurementSource::Derived,
        }
    }
}

/// A size in physical pixels. Never DIPs — see docs/design/0001-stack.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// A rectangle in virtual-desktop physical pixels. `x`/`y` are signed because
/// monitors left of or above the primary have negative coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }
}
