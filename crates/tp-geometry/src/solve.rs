//! The rig solver: an authored [`RigModel`] becomes solved geometry.
//!
//! Everything a game adapter needs comes from here, and adapters never do their
//! own trigonometry. See docs/design/0003-rig-model.md for the model and the
//! coordinate system.
//!
//! ## How a rig is laid out
//!
//! Screens are placed by walking outward from the centre panel. Each screen's
//! inboard edge starts where the previous screen's outboard edge ended, plus
//! the two bezels and the mount gap between them, and then the panel extends
//! outward along its own yawed direction. That is how a real rig is built, so
//! it is how the model builds it — rather than asking the user for coordinates
//! nobody measures.

use tp_model::{
    Curvature, Deg, Mm, RigModel, ScreenId, ScreenRole, ScreenSpec, SessionMode, WidthMeasure,
};

use crate::curvature::{self, CurveSolution};
use crate::vec3::Vec3;

/// A screen's placement and everything derived from it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenSolution {
    pub id: ScreenId,
    pub role: ScreenRole,
    /// Centre of the visible surface, eye at the origin.
    pub centre: Vec3,
    /// The four visible corners: inboard-bottom, inboard-top, outboard-bottom,
    /// outboard-top. In the eye frame, so each *is* a direction.
    pub corners: [Vec3; 4],
    /// Eye to the centre of this screen's surface.
    pub distance: Mm,
    /// What a flat-plane sim should be told. For a curved panel this is the
    /// chord width paired with the chord-plane distance — see `curvature`.
    pub flat: FlatEquivalent,
    /// The honest asymmetric truth. Left is negative, right positive.
    pub span: AngularSpan,
    /// `|left| + |right|`. What titles that accept a single number want.
    pub h_fov: Deg,
    pub v_fov: Deg,
    /// Off-axis frustum at near = 1. What titles with real triple support want.
    pub frustum: Frustum,
    pub px_per_deg_h: f64,
    pub px_per_deg_v: f64,
    /// Physical dark band between this screen and its inboard neighbour.
    pub inner_gap: Option<BezelGap>,
    /// Max angular error from treating this panel's curve as flat.
    pub curvature_error: Option<Deg>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatEquivalent {
    pub width: Mm,
    pub height: Mm,
    pub distance: Mm,
}

/// Signed angles to the screen's edges. Left and right are rarely symmetric —
/// any yawed side screen, or any lateral seating offset, breaks the symmetry —
/// so both are reported rather than one half-angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngularSpan {
    pub left: Deg,
    pub right: Deg,
    pub bottom: Deg,
    pub top: Deg,
}

impl AngularSpan {
    pub fn horizontal(&self) -> f64 {
        self.right.0 - self.left.0
    }
    pub fn vertical(&self) -> f64 {
        self.top.0 - self.bottom.0
    }
    /// How far off-centre the screen sits, in degrees. Above about 1 degree the
    /// UI shows the asymmetry rather than only the symmetric equivalent.
    pub fn asymmetry(&self) -> f64 {
        (self.right.0 + self.left.0).abs()
    }
}

/// Off-axis projection at near = 1. Multiply by a title's near plane to use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BezelGap {
    /// The physical dark band: this screen's inner bezel, the mount gap, and
    /// the neighbour's outer bezel.
    pub mm: Mm,
    /// The same gap in pixels. `None` when the panels either side have
    /// different pixel pitch, where the conversion is meaningless — better to
    /// say so than to return a number that is quietly wrong.
    pub px: Option<f64>,
    /// What the gap costs in field of view at this seating distance.
    pub deg: Deg,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RigSolution {
    pub screens: Vec<ScreenSolution>,
    /// Outer edge to outer edge, gaps included.
    pub total_coverage: Deg,
    /// Sum of the per-screen spans, gaps excluded.
    pub visible_coverage: Deg,
    pub warnings: Vec<Warning>,
}

impl RigSolution {
    pub fn screen(&self, id: ScreenId) -> Option<&ScreenSolution> {
        self.screens.iter().find(|s| s.id == id)
    }
    pub fn centre(&self) -> Option<&ScreenSolution> {
        self.screens.iter().find(|s| s.role == ScreenRole::Center)
    }
}

/// Warnings never block. A rig the model dislikes is still a rig someone owns.
#[derive(Debug, Clone, PartialEq)]
pub enum Warning {
    NoCentreScreen,
    FovTooWide { id: ScreenId, deg: f64 },
    FovTooNarrow { id: ScreenId, deg: f64 },
    SeatingTooClose { mm: f64 },
    ScreenEdgeBehindEye { id: ScreenId },
    PitchMismatch { id: ScreenId, fraction: f64 },
    CurvatureErrorHigh { id: ScreenId, deg: f64 },
    MissingPhysicalSize { id: ScreenId },
}

impl Warning {
    /// One sentence saying what is wrong and what it means. The UI renders
    /// these directly, so they are written for a person, not a log.
    pub fn message(&self) -> String {
        match self {
            Warning::NoCentreScreen => {
                "No screen is marked as the centre. Every angle and distance is measured from it, \
                 so one screen has to be."
                    .into()
            }
            Warning::FovTooWide { deg, .. } => format!(
                "Horizontal field of view is {deg:.1} degrees. Above about 120 the edges distort \
                 badly in most sims — check the seating distance and screen width."
            ),
            Warning::FovTooNarrow { deg, .. } => format!(
                "Horizontal field of view is only {deg:.1} degrees. Below about 30 you lose the \
                 peripheral cues that make a car feel placed."
            ),
            Warning::SeatingTooClose { mm } => format!(
                "Eye to screen is {mm:.0} mm. Under 300 mm is closer than a monitor is designed \
                 to be viewed; check the measurement."
            ),
            Warning::ScreenEdgeBehindEye { .. } => {
                "A screen edge sits behind your eye point. That angle cannot be projected — \
                 reduce the screen angle or move the seat back."
                    .into()
            }
            Warning::PitchMismatch { fraction, .. } => format!(
                "This panel's pixel pitch differs from its neighbour's by {:.0}%. Bezel \
                 compensation cannot be expressed in pixels across that seam.",
                fraction * 100.0
            ),
            Warning::CurvatureErrorHigh { deg, .. } => format!(
                "Treating this curved panel as flat is off by up to {deg:.1} degrees at the \
                 edges. Sims project onto flat planes, so some error is unavoidable."
            ),
            Warning::MissingPhysicalSize { .. } => {
                "This screen has no physical size. Measure the visible image width and height — \
                 every angle depends on it."
                    .into()
            }
        }
    }
}

/// Solve a rig.
///
/// `session` selects which screens participate: centre-only ignores the sides
/// entirely, rather than solving them and leaving the caller to filter.
pub fn solve(rig: &RigModel, session: SessionMode) -> RigSolution {
    let mut warnings = Vec::new();

    if rig.center().is_none() {
        warnings.push(Warning::NoCentreScreen);
    }
    if rig.seating.eye_to_center.0 < 300.0 {
        warnings.push(Warning::SeatingTooClose {
            mm: rig.seating.eye_to_center.0,
        });
    }

    let participating: Vec<&ScreenSpec> = match session {
        SessionMode::CenterOnly => rig
            .screens
            .iter()
            .filter(|s| s.role == ScreenRole::Center)
            .collect(),
        _ => rig.screens.iter().collect(),
    };

    let placements = place(rig, &participating);
    let mut screens: Vec<ScreenSolution> = Vec::with_capacity(placements.len());

    for (index, placement) in placements.iter().enumerate() {
        let spec = placement.spec;
        let solution = derive(placement, &mut warnings);

        // Pixel pitch either side of the inboard seam. This decides both
        // whether the bezel gap can be stated in pixels and whether the rig
        // gets a mismatch warning, so it is looked up once.
        let seam_mismatch = neighbour_of(&placements, index)
            .and_then(|n| Some(crate::layout::pitch_mismatch(pitch(placement)?, pitch(n)?)));

        if let Some(mismatch) = seam_mismatch {
            if mismatch > 0.10 {
                warnings.push(Warning::PitchMismatch {
                    id: spec.id,
                    fraction: mismatch,
                });
            }
        }

        let inner_gap = placement.inner_gap_mm.map(|gap_mm| BezelGap {
            mm: Mm(gap_mm),
            // Only meaningful when both panels share a pitch; otherwise there
            // is no single pixel size for the gap to have.
            px: match (seam_mismatch, pitch(placement)) {
                (Some(m), Some(px_per_mm)) if m <= 0.10 => Some(gap_mm * px_per_mm),
                _ => None,
            },
            // What the gap costs in angle at this screen's distance.
            deg: Deg(2.0 * ((gap_mm / 2.0) / solution.distance.0).atan().to_degrees()),
        });

        screens.push(ScreenSolution {
            inner_gap,
            ..solution
        });
    }

    let (total, visible) = coverage(&screens);
    RigSolution {
        screens,
        total_coverage: Deg(total),
        visible_coverage: Deg(visible),
        warnings,
    }
}

// ------------------------------------------------------------------ placement

struct Placement<'a> {
    spec: &'a ScreenSpec,
    curve: CurveSolution,
    /// Centre of the visible surface, eye at the origin.
    centre: Vec3,
    /// Unit vector along the surface, pointing outward from the rig centre.
    along: Vec3,
    /// Unit vector up the surface.
    up: Vec3,
    /// Effective flat width and height for projection.
    flat_width: f64,
    flat_height: f64,
    /// Distance to the plane the flat model uses.
    flat_distance: f64,
    inner_gap_mm: Option<f64>,
    /// Signed order: negative left of centre, 0 centre, positive right.
    order: i32,
}

fn place<'a>(rig: &RigModel, screens: &[&'a ScreenSpec]) -> Vec<Placement<'a>> {
    let mut out = Vec::with_capacity(screens.len());

    let Some(centre_spec) = screens
        .iter()
        .copied()
        .find(|s| s.role == ScreenRole::Center)
    else {
        // Without a centre there is no reference plane. Solve nothing rather
        // than invent one; the warning already says so.
        return out;
    };

    // The eye sits at the origin, so a lateral or vertical seating offset moves
    // the *screens*, not the eye. Doing it here means no formula downstream has
    // to carry the offset around.
    let eye_offset = Vec3::new(
        -rig.seating.lateral_offset.0,
        -rig.seating.eye_height_offset.0,
        0.0,
    );

    let centre_curve = curve_of(centre_spec);
    let centre_distance = rig.seating.eye_to_center.0 + centre_curve.sagitta.0;
    let centre_pos = Vec3::new(0.0, 0.0, centre_distance) + eye_offset;

    out.push(Placement {
        spec: centre_spec,
        curve: centre_curve,
        centre: centre_pos,
        along: Vec3::new(1.0, 0.0, 0.0),
        up: Vec3::new(0.0, 1.0, 0.0),
        flat_width: centre_curve.chord.0,
        flat_height: centre_spec.panel.visible_height.mm.0,
        flat_distance: centre_distance,
        inner_gap_mm: None,
        order: 0,
    });

    // Walk outward in each direction from the centre, in the order the screens
    // are listed. `direction` is -1 for the left chain, +1 for the right.
    for direction in [-1.0f64, 1.0] {
        let chain: Vec<&ScreenSpec> = if direction < 0.0 {
            screens
                .iter()
                .copied()
                .filter(|s| s.role == ScreenRole::Left)
                .rev()
                .collect()
        } else {
            screens
                .iter()
                .copied()
                .filter(|s| s.role == ScreenRole::Right)
                .collect()
        };

        // The outboard edge of the previous panel, and the direction it ran.
        let mut edge = centre_pos + Vec3::new(direction * centre_curve.chord.0 / 2.0, 0.0, 0.0);
        let mut previous_bezel = if direction < 0.0 {
            centre_spec.panel.bezel.left.0
        } else {
            centre_spec.panel.bezel.right.0
        };
        let mut cumulative_angle = 0.0f64;
        let mut order = 0i32;

        for spec in chain {
            order += 1;
            let curve = curve_of(spec);
            cumulative_angle += spec.mounting.angle.radians();

            // Direction this panel runs, outward from the rig centre.
            // Positive angle means angled *inward*: the outboard edge wraps
            // toward the driver, i.e. toward -Z. For the right chain that is a
            // positive yaw of the +X direction; for the left chain a negative
            // yaw of -X. Both are `yaw(direction * angle)` — inverting this
            // sign folds the panels away behind the rig, which is what the
            // reference test caught.
            let along = Vec3::new(direction, 0.0, 0.0).yaw(direction * cumulative_angle);

            let inner_bezel = if direction < 0.0 {
                spec.panel.bezel.right.0
            } else {
                spec.panel.bezel.left.0
            };
            let gap = previous_bezel + spec.mounting.gap.0 + inner_bezel;

            // The visible surface starts a gap's width along, then extends by
            // the panel's chord.
            let inner_edge = edge + along * gap;
            let centre = inner_edge
                + along * (curve.chord.0 / 2.0)
                + Vec3::new(0.0, spec.mounting.vertical_offset.0, 0.0);

            // No sagitta adjustment here: the walk steps along chord lengths
            // between chord endpoints, so `centre` is already on the chord
            // plane — which is the plane a flat-projecting sim should be told
            // about. Nudging it again would count the curve twice.
            let distance = spec
                .mounting
                .distance_override
                .map(|d| d.0 + curve.sagitta.0)
                .unwrap_or_else(|| centre.length());

            out.push(Placement {
                spec,
                curve,
                centre,
                along,
                up: Vec3::new(0.0, 1.0, 0.0).pitch(spec.mounting.pitch.radians()),
                flat_width: curve.chord.0,
                flat_height: spec.panel.visible_height.mm.0,
                flat_distance: distance,
                inner_gap_mm: Some(gap),
                order: (direction as i32) * order,
            });

            edge = inner_edge + along * curve.chord.0;
            previous_bezel = if direction < 0.0 {
                spec.panel.bezel.left.0
            } else {
                spec.panel.bezel.right.0
            };
        }
    }

    out.sort_by_key(|p| p.order);
    out
}

fn curve_of(spec: &ScreenSpec) -> CurveSolution {
    curvature::solve(
        spec.panel.visible_width.mm,
        spec.panel.curvature,
        spec.panel.width_measure,
    )
}

fn neighbour_of<'a, 'b>(
    placements: &'b [Placement<'a>],
    index: usize,
) -> Option<&'b Placement<'a>> {
    // The inboard neighbour is the one nearer the centre in the same direction.
    let here = placements.get(index)?;
    match here.order.cmp(&0) {
        std::cmp::Ordering::Less => placements.iter().find(|p| p.order == here.order + 1),
        std::cmp::Ordering::Greater => placements.iter().find(|p| p.order == here.order - 1),
        std::cmp::Ordering::Equal => None,
    }
}

fn pitch(p: &Placement) -> Option<f64> {
    crate::layout::pixel_pitch(p.spec.panel.native_resolution.width, p.curve.arc.0)
}

// -------------------------------------------------------------------- derive

fn derive(p: &Placement, warnings: &mut Vec<Warning>) -> ScreenSolution {
    let spec = p.spec;
    let half_w = p.flat_width / 2.0;
    let half_h = p.flat_height / 2.0;

    if p.flat_width <= 0.0 || p.flat_height <= 0.0 {
        warnings.push(Warning::MissingPhysicalSize { id: spec.id });
    }

    let across = p.along * half_w;
    let upward = p.up * half_h;
    let corners = [
        p.centre - across - upward,
        p.centre - across + upward,
        p.centre + across - upward,
        p.centre + across + upward,
    ];

    if corners.iter().any(|c| c.z <= 0.0) {
        warnings.push(Warning::ScreenEdgeBehindEye { id: spec.id });
    }

    // Horizontal angles come from the corners, which is safe because azimuth
    // ignores height — a corner and the edge midpoint below it share a bearing.
    let azimuths: Vec<f64> = corners.iter().map(|c| c.azimuth_deg()).collect();

    // Vertical angles do NOT come from the corners. A corner is further from
    // the eye than the middle of the same edge, so its elevation is smaller,
    // and taking the extremes of corner elevations understates vertical FOV —
    // 24.9 degrees rather than 27.0 on the reference rig. Vertical FOV is
    // measured up the screen's centreline, so use the edge midpoints.
    let bottom_mid = p.centre - upward;
    let top_mid = p.centre + upward;

    let span = AngularSpan {
        left: Deg(azimuths.iter().cloned().fold(f64::INFINITY, f64::min)),
        right: Deg(azimuths.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
        bottom: Deg(bottom_mid.elevation_deg()),
        top: Deg(top_mid.elevation_deg()),
    };

    let h_fov = span.horizontal();
    let v_fov = span.vertical();

    if h_fov > 120.0 {
        warnings.push(Warning::FovTooWide {
            id: spec.id,
            deg: h_fov,
        });
    } else if h_fov < 30.0 {
        warnings.push(Warning::FovTooNarrow {
            id: spec.id,
            deg: h_fov,
        });
    }

    let curvature_error = match spec.panel.curvature {
        Curvature::Flat => None,
        Curvature::Radius { radius } => {
            // Distance to the nearest point, which is what the curve model wants.
            let nearest = Mm(p.flat_distance - p.curve.sagitta.0);
            let err = p.curve.worst_case_error_deg(nearest, radius);
            if err > 1.0 {
                warnings.push(Warning::CurvatureErrorHigh {
                    id: spec.id,
                    deg: err,
                });
            }
            Some(Deg(err))
        }
    };

    ScreenSolution {
        id: spec.id,
        role: spec.role.clone(),
        centre: p.centre,
        corners,
        distance: Mm(p.flat_distance),
        flat: FlatEquivalent {
            width: Mm(p.flat_width),
            height: Mm(p.flat_height),
            distance: Mm(p.flat_distance),
        },
        span,
        h_fov: Deg(h_fov),
        v_fov: Deg(v_fov),
        frustum: Frustum {
            left: span.left.radians().tan(),
            right: span.right.radians().tan(),
            bottom: span.bottom.radians().tan(),
            top: span.top.radians().tan(),
        },
        px_per_deg_h: if h_fov > 0.0 {
            spec.panel.native_resolution.width as f64 / h_fov
        } else {
            0.0
        },
        px_per_deg_v: if v_fov > 0.0 {
            spec.panel.native_resolution.height as f64 / v_fov
        } else {
            0.0
        },
        inner_gap: None,
        curvature_error,
    }
}

fn coverage(screens: &[ScreenSolution]) -> (f64, f64) {
    if screens.is_empty() {
        return (0.0, 0.0);
    }
    let leftmost = screens
        .iter()
        .map(|s| s.span.left.0)
        .fold(f64::INFINITY, f64::min);
    let rightmost = screens
        .iter()
        .map(|s| s.span.right.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let visible: f64 = screens.iter().map(|s| s.span.horizontal()).sum();
    (rightmost - leftmost, visible)
}

/// Which strategy a title's capabilities allow for this rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// The title takes per-screen values. Write the truth.
    Exact,
    /// The title takes one set. Solve for the least-wrong single set and say so.
    BestFit,
    /// The title cannot usefully represent the rig at all.
    CentreOnly,
}

/// Are all participating screens close enough to identical for a single set of
/// values to be exact rather than a fit?
pub fn screens_are_uniform(rig: &RigModel) -> bool {
    let sides: Vec<&ScreenSpec> = rig
        .screens
        .iter()
        .filter(|s| matches!(s.role, ScreenRole::Left | ScreenRole::Right))
        .collect();
    let [a, b] = sides.as_slice() else {
        return sides.len() <= 1;
    };

    let close = |x: f64, y: f64, tol: f64| (x - y).abs() <= tol;
    close(a.panel.visible_width.mm.0, b.panel.visible_width.mm.0, 1.0)
        && close(
            a.panel.visible_height.mm.0,
            b.panel.visible_height.mm.0,
            1.0,
        )
        && close(a.mounting.angle.0, b.mounting.angle.0, 0.5)
        && a.panel.native_resolution == b.panel.native_resolution
}

/// Convenience: horizontal FOV of a flat screen of `width` at `distance`.
///
/// The textbook formula, exact only when the eye is on the screen's normal
/// through its centre. Anything else should use [`AngularSpan`].
pub fn flat_h_fov(width: Mm, distance: Mm) -> Deg {
    Deg(2.0 * ((width.0 / 2.0) / distance.0).atan().to_degrees())
}

/// Round-trip helper for adapters that are handed an FOV and need the width it
/// implies at a known distance.
pub fn width_for_fov(fov: Deg, distance: Mm) -> Mm {
    Mm(2.0 * distance.0 * (fov.radians() / 2.0).tan())
}

/// Whether a panel's stated width was entered as arc or chord.
pub fn width_measure_of(spec: &ScreenSpec) -> WidthMeasure {
    spec.panel.width_measure
}
