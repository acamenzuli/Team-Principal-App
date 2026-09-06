# 0003 — The rig model

Status: **proposed**, awaiting sign-off. This is the schema everything else
hangs off, so it needs your agreement before any code is written against it.

---

## 1. Units and invariants

- Every length is `Mm(f64)`. Every angle is `Deg(f64)`. Newtypes, not bare
  floats, so a millimetre can never be passed where a degree is expected.
- The stored value is whatever you typed, in millimetres. `LengthUnit` is a UI
  preference stored on the rig and used **only** at the input/display boundary.
  Round-tripping mm → in → mm re-renders the same stored f64; it never
  re-quantises the model. Unit tested.
- Inline suffixes are parsed: `47.5in`, `1200mm`, `120cm`, `1.2m`, `47 1/2"`.
  Helper text under each field shows the normalised value.

## 2. Coordinate system

Fixed once, written down, never re-litigated:

- Right-handed. **Origin at the eye point.**
- `+X` right, `+Y` up, `+Z` forward (toward the screens).
- Angles are degrees; `+` yaw rotates a screen's outer edge toward the driver.

Putting the origin at the eye rather than at the centre screen makes every
frustum computation direct — a screen corner's position *is* its direction
vector — and it means lateral/vertical seating offset is applied once, when
poses are built, instead of being smeared through every formula.

## 3. Authored model

The rig is authored in human terms (*"the left panel is angled 55° in"*) and
**solved** into 3D poses. Two distinct types; do not conflate them.

```rust
pub struct RigModel {
    pub schema_version: u32,
    pub id: Uuid,
    pub revision: u64,              // bumped on every save; profiles record it
    pub name: String,
    pub updated_at: DateTime<Utc>,
    pub units_preference: LengthUnit,       // Mm | Cm | Inch — UI only
    pub seating: Seating,
    pub screens: Vec<ScreenSpec>,           // ordered left -> right
}

pub struct Seating {
    pub eye_to_center_mm: f64,      // eye -> nearest point of centre screen's visible surface
    pub eye_height_offset_mm: f64,  // + = eye above the centre screen's vertical centre
    pub lateral_offset_mm: f64,     // + = eye right of the centre screen's horizontal centre
}

pub struct ScreenSpec {
    pub id: ScreenId,
    pub role: ScreenRole,           // Center | Left | Right | Auxiliary { label: String }
    pub binding: MonitorBinding,
    pub panel: PanelSpec,
    pub mounting: MountingSpec,
}

pub struct PanelSpec {
    pub native_res: Resolution,                 // px
    pub visible_width: Measurement,             // mm + provenance
    pub visible_height: Measurement,
    pub width_measure: WidthMeasure,            // Arc | Chord — only meaningful when curved
    pub curvature: Curvature,                   // Flat | Radius { mm: f64 }
    pub bezel: Bezel { left, right, top, bottom }, // mm, per edge
}

pub struct Measurement { pub mm: f64, pub source: MeasurementSource } // Edid | Manual | Derived

pub struct MountingSpec {
    pub angle_deg: f64,                  // yaw vs the centre screen plane, + = inward
    pub gap_mm: f64,                     // mount gap to the inboard neighbour, BEYOND both bezels
    pub vertical_offset_mm: f64,         // + = this screen's centre above the centre screen's
    pub distance_override_mm: Option<f64>, // None = derived from geometry
    pub pitch_deg: f64,                  // reserved, defaults 0.0
    pub roll_deg: f64,                   // reserved, defaults 0.0
}
```

**On `pitch_deg` / `roll_deg`:** they are in the schema from day one, defaulted
to zero and not exposed in the v1 UI. Adding a field to a persisted schema later
costs a migration; reserving two f64s now costs nothing. The solver handles them
correctly regardless, because building a full pose is no harder than building a
yaw-only one.

**On `gap_mm`:** defined as space *in addition to* both adjacent bezels, so
changing a bezel measurement does not silently change the mount gap. The UI
shows the total edge-to-edge dark band (`bezel_right + gap + bezel_left`) as
derived helper text, since that is the number you can actually measure with a
ruler.

## 4. Monitor binding — one correction to the brief

You asked for binding by device path "so it survives cable swaps". It does not.
The CCD device path (`DISPLAYCONFIG_TARGET_DEVICE_NAME.monitorDevicePath`)
encodes the adapter and output — move a cable from DP-1 to DP-2 and the path
changes, and your left screen becomes your centre screen.

What actually survives is the monitor's own EDID identity. So:

```rust
pub enum MonitorBinding {
    Edid {
        manufacturer_id: String,    // 3-char PNP ID, EDID bytes 0x08..0x0A
        product_code: u16,          // bytes 0x0A..0x0C
        serial: Option<String>,     // descriptor 0xFF, when present
        serial_number: u32,         // bytes 0x0C..0x10, fallback
        week_year: (u8, u16),       // final tiebreaker for identical un-serialled panels
        cached_device_path: Option<String>,   // fast path only, re-validated on every scan
    },
    Unbound,
}
```

Resolution order: EDID identity first, `cached_device_path` only as a hint to
skip a full rescan. When two panels are genuinely indistinguishable (identical
model, no serial descriptor — common on cheap panels, and you have two 27"s
which may be a matched pair), the app cannot guess. It will say so and offer an
**Identify** button that flashes a full-screen colour on one panel and asks
which one you saw, then pins the answer to the device path until it changes.

## 5. Curvature — the part most tools get wrong

Your centre screen is curved. Sims project onto flat planes. The naive handling
of this is wrong by a *lot*, so here is the exact treatment.

For radius `R` and arc (developed) width `L`:

```
theta   = L / R                       subtended angle, radians
chord   = 2R * sin(theta/2)
sagitta = R * (1 - cos(theta/2))      how far the centre bulges toward you
```

Worked example — a 49" 1000R panel, arc width 1193 mm, eye 700 mm from the
panel's nearest point:

| Quantity | Value |
| --- | --- |
| Subtended angle at the centre of curvature | 68.35° |
| Chord width | 1123.5 mm |
| Sagitta | 172.7 mm |
| **hFOV if you feed the arc width at 700 mm** | **80.87°** |
| **hFOV correctly modelled** | **65.54°** |

A 15.3° error. That is the difference between a rig that feels right and one
where the world slides past too fast and you never work out why.

The reason the naive number is wrong: on a curved panel the *edges* wrap toward
you, so they sit at depth `D + sagitta`, not `D`. Their lateral offset is
`chord/2`, not `arc/2`. Both corrections shrink the angle.

**The rule, which resolves your "chord or arc?" question:** it is not chord *or*
arc, it is *which distance you pair the width with*.

> Use the **chord width** together with the **distance to the chord plane**
> (`eye_to_center + sagitta`). By construction the panel's visible edges lie in
> that plane, so this reproduces the true edge angles exactly.

`65.54° = 2 · atan(561.7 / 872.7)`. Pairing chord with the *nearest-point*
distance (1123.5 mm at 700 mm → 77.49°) is the common half-right mistake, and it
is what a lot of FOV calculators do.

The residual error is that intermediate points on the arc sit in front of the
chord plane. The app quantifies it rather than hand-waving: it samples the arc,
computes each sample's true angle from the eye against the angle the flat model
implies, and reports the maximum as `curvature_error_deg`. You get a number and
decide whether it matters at your radius.

`width_measure` records whether you entered arc or chord, defaulting to **Arc**
for curved panels — a tape measure laid on the screen gives arc, and spec sheets
usually quote the developed dimension. The other is derived and shown as helper
text so you can sanity-check against the datasheet.

## 6. Solved output

`tp_geometry::solve(&RigModel, SessionMode) -> RigSolution`. Pure function, no
I/O, no Win32, no adapter knowledge.

```rust
pub struct RigSolution {
    pub screens: Vec<ScreenSolution>,
    pub total_coverage_deg: f64,          // outer edge to outer edge, gaps included
    pub visible_coverage_deg: f64,        // sum of per-screen spans, gaps excluded
    pub span_rect_px: Option<PixelRect>,  // for SessionMode::FullSpan
    pub dead_regions_px: Vec<PixelRect>,  // virtual-desktop space mapping to no panel
    pub warnings: Vec<GeometryWarning>,
}

pub struct ScreenSolution {
    pub id: ScreenId,
    pub pose: ScreenPose,                 // centre, normal, right, up — eye-origin frame
    pub flat_equivalent: FlatEquivalent,  // width/height/distance a flat-plane sim should use
    pub h_fov_deg: f64,                   // symmetric equivalent = |left| + |right|
    pub v_fov_deg: f64,
    pub span: AngularSpan,                // signed left/right/top/bottom — the honest, asymmetric truth
    pub frustum: OffAxisFrustum,          // l,r,b,t at near=1 — for titles with real triple support
    pub px_per_deg_h: f64,
    pub px_per_deg_v: f64,
    pub bezel_comp: BezelCompensation,
    pub curvature_error_deg: Option<f64>,
}

pub struct BezelCompensation {
    pub gap_mm: f64,                      // physical dark band to the inboard neighbour
    pub gap_px: Option<PxWithCaveat>,     // None when PPI differs across the seam, with a reason
    pub gap_deg: f64,                     // angular loss at your seating distance
}
```

`h_fov_deg = 2·atan(W / 2D)` holds only when the eye is on the screen's normal
through its centre. With a lateral seating offset, or on any angled side screen,
it is not — so `span` carries the signed asymmetric values and `h_fov_deg` is
the symmetric equivalent for titles that only accept one number. Both are
reported, always, and the UI shows the asymmetry when it exceeds ~1°.

`gap_px` is `Option` on purpose. Converting a physical gap to pixels requires
`gap_mm × px_per_mm`, which is only meaningful when the two panels across the
seam have the same pixel pitch. Your 49" and your 27"s almost certainly do not.
Returning `None` with a stated reason is more useful than returning a number
that is quietly wrong.

## 7. Handling mismatched screens

Three strategies, chosen per title from `Capabilities`, and **always named in
the UI**:

- **Exact** — the title supports per-screen values. Write the truth.
- **Best fit** — the title accepts one width/height/bezel/distance/angle. Solve
  for the single set minimising weighted angular error:
  - sample ~33 points across each screen's visible surface;
  - error = |true angle from eye − angle the single-set model implies|;
  - weight `w(θ) = exp(-(θ/σ)²)` with `σ = 25°`, times an apex-side multiplier
    (default 1.5) for the region you actually look at while driving. Both
    constants live in one documented place and are tunable;
  - minimise `Σ w·err²` with a hand-rolled Nelder–Mead (~80 lines, no new
    dependency, deterministic, unit-testable);
  - report max and RMS residual **per screen**.
- **Center only** — the title's projection model cannot represent the rig.
  Say so plainly and recommend the centre screen alone.

The rule that matters: a best-fit result is never presented as correct. The UI
line is literally of the form *"Assetto Corsa accepts one screen size. These are
a best fit — your side screens are off by 2.1°."*

## 8. Session modes

Derived, never typed:

- **Center only** — game on the centre panel, sides free for SimHub.
- **Full span** — one surface across all screens. Unequal panel heights are
  reconciled by `SpanFit::{Letterbox, Crop, Stretch}`, chosen per profile.
- **Custom rect** — drawn in the layout editor; geometry recomputed for whatever
  rectangle you draw, including partial-panel rectangles.

## 9. Warnings (warn, never block)

`FovTooWide(>120°)`, `FovTooNarrow(<30°)`, `SeatingTooClose(<300 mm)`,
`EdidSizeMismatch(>3%)`, `ScreenEdgeBehindEye`, `PpiMismatch(>10%)`,
`CurvatureErrorHigh`, `AngleAsymmetry`, `MonitorBindingAmbiguous`.

## 10. Reference fixture

`fixtures/geometry/symmetric-triple-27-1440p.json` — three 27" 16:9 1440p panels
at 60°, eye 700 mm, zero bezel and gap. Hand-computed expectations committed in
the test:

| Quantity | Expected |
| --- | --- |
| Panel visible size | 597.7 × 336.2 mm |
| Centre hFOV | 46.24° |
| Centre vFOV | 27.01° |
| Side screen angular span | 23.12° → 73.03° (49.91° wide) |
| Total coverage | 146.07° |
| Derived eye distance to a side screen centre | 629.0 mm |
| px/deg, centre vs side | 55.4 vs 51.3 (7.4% — under the 10% warn threshold) |

Zero bezel makes it cross-checkable against published FOV calculators. A second
fixture adds real bezels and gaps so the bezel path is covered too.

## 11. Recompute and propagate

`RigModel.revision` increments on every save. Each profile stores the revision
it was computed against **plus a snapshot of the derived values it actually
consumed**. On a rig change the app re-solves every profile, diffs against the
stored snapshot, and shows one summary screen: which profiles changed, which
fields, old → new. Apply all, or tick individually. Nothing is written to any
game config until you press apply.

Storing the consumed snapshot (rather than just the revision) is what makes the
diff exact and available offline, with no need to reconstruct the old rig.
