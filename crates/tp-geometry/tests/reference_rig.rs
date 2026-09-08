//! The solver against hand-computed reference values.
//!
//! The symmetric fixture — three 27" 16:9 1440p panels at 60 degrees, eye
//! 700 mm, zero bezel and gap — is documented in
//! docs/design/0003-rig-model.md with every expected number. Zero bezel makes
//! it cross-checkable against published FOV calculators. If a number here
//! changes, that document is wrong and has to change with it.

use tp_geometry::solve::{screens_are_uniform, Warning};
use tp_geometry::{solve, Vec3};
use tp_model::*;

/// 27" 16:9 -> 597.7 x 336.2 mm.
const W27: f64 = 597.7;
const H27: f64 = 336.2;

fn panel(width: f64, height: f64, res: (u32, u32), curvature: Curvature) -> PanelSpec {
    PanelSpec {
        native_resolution: Resolution {
            width: res.0,
            height: res.1,
        },
        visible_width: Measurement::manual(width),
        visible_height: Measurement::manual(height),
        width_measure: WidthMeasure::Arc,
        curvature,
        bezel: Bezel::default(),
    }
}

fn screen(id: u32, role: ScreenRole, panel: PanelSpec, angle: f64) -> ScreenSpec {
    ScreenSpec {
        id: ScreenId(id),
        role,
        binding: MonitorBinding::Unbound,
        panel,
        mounting: MountingSpec {
            angle: Deg(angle),
            ..Default::default()
        },
    }
}

/// Three matched 27" 1440p panels, sides at 60 degrees, eye 700 mm, no bezels.
fn symmetric_triple() -> RigModel {
    let mut rig = RigModel::new("symmetric triple", "2026-01-01T00:00:00Z");
    rig.seating = Seating {
        eye_to_center: Mm(700.0),
        eye_height_offset: Mm::ZERO,
        lateral_offset: Mm::ZERO,
    };
    let p = || panel(W27, H27, (2560, 1440), Curvature::Flat);
    rig.screens = vec![
        screen(1, ScreenRole::Left, p(), 60.0),
        screen(2, ScreenRole::Center, p(), 0.0),
        screen(3, ScreenRole::Right, p(), 60.0),
    ];
    rig
}

fn near(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: got {actual:.3}, expected {expected:.3}"
    );
}

#[test]
fn centre_screen_matches_the_documented_reference() {
    let s = solve(&symmetric_triple(), SessionMode::CenterOnly);
    let c = s.centre().expect("centre screen");

    near(c.h_fov.0, 46.24, 0.02, "centre hFOV");
    near(c.v_fov.0, 27.01, 0.02, "centre vFOV");
    near(c.distance.0, 700.0, 0.01, "eye to centre");

    // Dead ahead, so the span must be symmetric about zero.
    near(c.span.asymmetry(), 0.0, 1e-9, "centre asymmetry");
    near(c.px_per_deg_h, 55.4, 0.1, "centre px/deg");
}

#[test]
fn side_screens_match_the_documented_reference() {
    let s = solve(
        &symmetric_triple(),
        SessionMode::FullSpan {
            fit: SpanFit::Letterbox,
        },
    );
    assert_eq!(s.screens.len(), 3);

    let right = s
        .screens
        .iter()
        .find(|x| x.role == ScreenRole::Right)
        .unwrap();
    near(right.span.left.0, 23.12, 0.02, "side inner edge");
    near(right.span.right.0, 73.03, 0.02, "side outer edge");
    near(right.span.horizontal(), 49.91, 0.02, "side span");
    near(right.distance.0, 629.0, 0.5, "eye to side centre");
    near(right.px_per_deg_h, 51.3, 0.1, "side px/deg");

    near(s.total_coverage.0, 146.07, 0.05, "total coverage");
    near(
        s.visible_coverage.0,
        146.07,
        0.05,
        "visible coverage (no gaps)",
    );
}

#[test]
fn the_rig_is_mirror_symmetric() {
    // A symmetric rig must solve symmetrically. If it does not, the outward
    // walk has a sign error on one side — the classic way this goes wrong.
    let s = solve(
        &symmetric_triple(),
        SessionMode::FullSpan {
            fit: SpanFit::Letterbox,
        },
    );
    let left = s
        .screens
        .iter()
        .find(|x| x.role == ScreenRole::Left)
        .unwrap();
    let right = s
        .screens
        .iter()
        .find(|x| x.role == ScreenRole::Right)
        .unwrap();

    near(
        -left.span.right.0,
        right.span.left.0,
        1e-6,
        "inner edges mirror",
    );
    near(
        -left.span.left.0,
        right.span.right.0,
        1e-6,
        "outer edges mirror",
    );
    near(left.distance.0, right.distance.0, 1e-6, "distances mirror");
    near(left.centre.x, -right.centre.x, 1e-6, "centres mirror in x");
    near(left.centre.z, right.centre.z, 1e-6, "centres mirror in z");
}

#[test]
fn px_per_degree_differs_between_centre_and_sides() {
    // Identical panels still have different angular density once the sides are
    // angled in, because they subtend more degrees at a shorter distance. 7.4%
    // here, under the 10% threshold that would warn.
    let s = solve(
        &symmetric_triple(),
        SessionMode::FullSpan {
            fit: SpanFit::Letterbox,
        },
    );
    let c = s.centre().unwrap().px_per_deg_h;
    let side = s
        .screens
        .iter()
        .find(|x| x.role == ScreenRole::Right)
        .unwrap()
        .px_per_deg_h;
    let diff = (c - side) / c;
    near(diff, 0.074, 0.002, "px/deg difference");
}

#[test]
fn centre_only_ignores_the_side_screens() {
    let s = solve(&symmetric_triple(), SessionMode::CenterOnly);
    assert_eq!(s.screens.len(), 1);
    near(
        s.total_coverage.0,
        46.24,
        0.02,
        "coverage is just the centre",
    );
}

#[test]
fn bezels_and_gaps_push_the_sides_outward() {
    // Same rig, but with real bezels and a mount gap. The sides must move
    // outward, and the total coverage must grow by the gaps.
    let mut rig = symmetric_triple();
    for s in &mut rig.screens {
        s.panel.bezel = Bezel {
            left: Mm(8.0),
            right: Mm(8.0),
            top: Mm(8.0),
            bottom: Mm(8.0),
        };
        s.mounting.gap = Mm(4.0);
    }
    let bare = solve(
        &symmetric_triple(),
        SessionMode::FullSpan {
            fit: SpanFit::Letterbox,
        },
    );
    let with_bezels = solve(
        &rig,
        SessionMode::FullSpan {
            fit: SpanFit::Letterbox,
        },
    );

    assert!(with_bezels.total_coverage.0 > bare.total_coverage.0);

    let right = with_bezels
        .screens
        .iter()
        .find(|x| x.role == ScreenRole::Right)
        .unwrap();
    let gap = right.inner_gap.expect("side screens have an inboard gap");
    // 8 mm centre bezel + 4 mm mount gap + 8 mm side bezel.
    near(gap.mm.0, 20.0, 1e-9, "physical gap");
    assert!(gap.deg.0 > 0.0, "a gap costs field of view");
    // Matched panels, so the gap is expressible in pixels.
    assert!(gap.px.is_some(), "matched pitch should give a pixel figure");

    // The centre screen has no inboard neighbour.
    assert!(with_bezels.centre().unwrap().inner_gap.is_none());
}

#[test]
fn a_pitch_mismatch_refuses_to_state_the_gap_in_pixels() {
    // 1080p side panels of the same physical size as the 1440p centre: a third
    // of a pitch difference, so there is no single pixel size for a gap to have.
    let mut rig = symmetric_triple();
    for s in &mut rig.screens {
        s.panel.bezel = Bezel {
            left: Mm(8.0),
            right: Mm(8.0),
            top: Mm(8.0),
            bottom: Mm(8.0),
        };
        if s.role != ScreenRole::Center {
            s.panel.native_resolution = Resolution {
                width: 1920,
                height: 1080,
            };
        }
    }
    let s = solve(
        &rig,
        SessionMode::FullSpan {
            fit: SpanFit::Letterbox,
        },
    );
    let right = s
        .screens
        .iter()
        .find(|x| x.role == ScreenRole::Right)
        .unwrap();

    assert_eq!(
        right.inner_gap.unwrap().px,
        None,
        "a mismatched seam has no pixel gap"
    );
    assert!(
        right.inner_gap.unwrap().deg.0 > 0.0,
        "but it still costs degrees"
    );
    assert!(
        s.warnings
            .iter()
            .any(|w| matches!(w, Warning::PitchMismatch { .. })),
        "and the user is told"
    );
}

#[test]
fn a_lateral_seating_offset_makes_the_centre_screen_asymmetric() {
    // Sitting 100 mm right of centre means the centre screen is no longer
    // symmetric about the line of sight. A single half-angle would hide that;
    // the signed span must not.
    let mut rig = symmetric_triple();
    rig.seating.lateral_offset = Mm(100.0);
    let s = solve(&rig, SessionMode::CenterOnly);
    let c = s.centre().unwrap();

    assert!(
        c.span.asymmetry() > 1.0,
        "asymmetry {} should be visible",
        c.span.asymmetry()
    );
    // Sitting right means more of the screen is to the left.
    assert!(c.span.left.0.abs() > c.span.right.0.abs());
    // The symmetric equivalent still totals the same span.
    near(c.h_fov.0, c.span.horizontal(), 1e-9, "h_fov is the span");
}

#[test]
fn a_curved_centre_screen_uses_the_chord_plane() {
    // 49" 1000R, arc 1193 mm, eye 700 mm to the nearest point. The documented
    // answer is 65.54 degrees, not the 80.87 a flat reading of the arc gives.
    let mut rig = symmetric_triple();
    rig.screens = vec![screen(
        1,
        ScreenRole::Center,
        panel(
            1193.0,
            336.0,
            (5120, 1440),
            Curvature::Radius { radius: Mm(1000.0) },
        ),
        0.0,
    )];
    let s = solve(&rig, SessionMode::CenterOnly);
    let c = s.centre().unwrap();

    near(c.h_fov.0, 65.54, 0.05, "curved hFOV");
    // The solver places the surface at the chord plane: 700 + sagitta.
    near(c.distance.0, 872.7, 0.2, "chord-plane distance");
    near(c.flat.width.0, 1123.5, 0.2, "chord width");
    assert!(
        c.curvature_error.is_some(),
        "a curved panel reports its flat-model error"
    );
}

#[test]
fn warnings_describe_the_problem_in_words() {
    let mut rig = symmetric_triple();
    rig.seating.eye_to_center = Mm(200.0);
    let s = solve(&rig, SessionMode::CenterOnly);

    let seating = s
        .warnings
        .iter()
        .find(|w| matches!(w, Warning::SeatingTooClose { .. }))
        .expect("200 mm is too close");
    let message = seating.message();
    assert!(
        message.contains("200"),
        "the message states the value: {message}"
    );
    assert!(
        !message.contains("Error"),
        "no error codes, no jargon: {message}"
    );
}

#[test]
fn a_rig_with_no_centre_screen_warns_and_solves_nothing() {
    // Rather than picking a screen arbitrarily and producing angles measured
    // from the wrong reference.
    let mut rig = symmetric_triple();
    rig.screens.retain(|s| s.role != ScreenRole::Center);
    let s = solve(
        &rig,
        SessionMode::FullSpan {
            fit: SpanFit::Letterbox,
        },
    );
    assert!(s.warnings.contains(&Warning::NoCentreScreen));
    assert!(s.screens.is_empty());
}

#[test]
fn uniformity_detection_drives_the_exact_versus_best_fit_choice() {
    assert!(
        screens_are_uniform(&symmetric_triple()),
        "matched sides are uniform"
    );

    let mut mixed = symmetric_triple();
    mixed.screens[0].mounting.angle = Deg(45.0);
    assert!(
        !screens_are_uniform(&mixed),
        "different angles are not uniform"
    );

    let mut mixed = symmetric_triple();
    mixed.screens[0].panel.visible_width = Measurement::manual(531.4);
    assert!(
        !screens_are_uniform(&mixed),
        "different widths are not uniform"
    );
}

#[test]
fn corners_are_direction_vectors_from_the_eye() {
    // The whole reason the origin is the eye: a corner's position is its
    // direction, so the span is just atan2 over the corners.
    let s = solve(&symmetric_triple(), SessionMode::CenterOnly);
    let c = s.centre().unwrap();
    for corner in c.corners {
        assert!(corner.z > 0.0, "every corner is in front of the eye");
    }
    let widest = c
        .corners
        .iter()
        .map(|c| c.azimuth_deg())
        .fold(f64::NEG_INFINITY, f64::max);
    near(
        widest,
        c.span.right.0,
        1e-9,
        "span.right is the widest corner",
    );
    assert_ne!(c.centre, Vec3::ZERO);
}
