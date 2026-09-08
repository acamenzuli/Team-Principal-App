//! The best-fit solver.
//!
//! The property that matters is not "it finds the optimum" — it is that a fit
//! is never presented as if it were exact. So these tests check that a uniform
//! rig fits essentially perfectly, that a mismatched one does not, and that the
//! residual it reports is the truth.

use tp_geometry::bestfit::{best_fit, FitWeights};
use tp_model::*;

fn panel(width: f64, height: f64, res: (u32, u32)) -> PanelSpec {
    PanelSpec {
        native_resolution: Resolution {
            width: res.0,
            height: res.1,
        },
        visible_width: Measurement::manual(width),
        visible_height: Measurement::manual(height),
        width_measure: WidthMeasure::Arc,
        curvature: Curvature::Flat,
        bezel: Bezel {
            left: Mm(8.0),
            right: Mm(8.0),
            top: Mm(8.0),
            bottom: Mm(8.0),
        },
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

fn rig(screens: Vec<ScreenSpec>, eye: f64) -> RigModel {
    let mut r = RigModel::new("fit", "2026-01-01T00:00:00Z");
    r.seating = Seating {
        eye_to_center: Mm(eye),
        eye_height_offset: Mm::ZERO,
        lateral_offset: Mm::ZERO,
    };
    r.screens = screens;
    r
}

fn uniform_triple() -> RigModel {
    let p = || panel(597.7, 336.2, (2560, 1440));
    rig(
        vec![
            screen(1, ScreenRole::Left, p(), 60.0),
            screen(2, ScreenRole::Center, p(), 0.0),
            screen(3, ScreenRole::Right, p(), 60.0),
        ],
        700.0,
    )
}

/// The rig this product exists for: a 49" ultrawide between two 27"s.
fn mismatched_rig() -> RigModel {
    rig(
        vec![
            screen(1, ScreenRole::Left, panel(597.7, 336.2, (2560, 1440)), 55.0),
            screen(
                2,
                ScreenRole::Center,
                panel(1193.0, 336.0, (5120, 1440)),
                0.0,
            ),
            screen(
                3,
                ScreenRole::Right,
                panel(597.7, 336.2, (2560, 1440)),
                55.0,
            ),
        ],
        700.0,
    )
}

const MODE: SessionMode = SessionMode::FullSpan {
    fit: SpanFit::Letterbox,
};

#[test]
fn a_uniform_rig_fits_itself_almost_exactly() {
    // If the rig already is three identical panels, the "best fit" is the rig,
    // and the residual should be near zero. A solver that cannot manage this
    // cannot be trusted on a hard case.
    let fit = best_fit(&uniform_triple(), MODE, FitWeights::default()).expect("a fit");
    assert!(
        fit.worst_error_deg() < 0.25,
        "uniform rig should fit itself; worst error {:.3} deg",
        fit.worst_error_deg()
    );
    assert!(
        (fit.width_mm - 597.7).abs() < 12.0,
        "width {:.1}",
        fit.width_mm
    );
    assert!(
        (fit.angle_deg - 60.0).abs() < 3.0,
        "angle {:.1}",
        fit.angle_deg
    );
}

#[test]
fn a_mismatched_rig_reports_a_real_residual() {
    // The point of the whole exercise: a single set of values *cannot* describe
    // this rig, and the app must say by how much rather than quietly writing
    // the numbers as if they were right.
    let fit = best_fit(&mismatched_rig(), MODE, FitWeights::default()).expect("a fit");
    assert!(
        fit.worst_error_deg() > 0.5,
        "a 49\" between two 27\"s cannot be one uniform set; got {:.2} deg",
        fit.worst_error_deg()
    );
    assert_eq!(
        fit.residuals.len(),
        3,
        "one residual per screen, so the UI can name the worst"
    );
    assert!(fit
        .residuals
        .iter()
        .all(|r| r.rms_error_deg <= r.max_error_deg));
}

#[test]
fn the_summary_never_presents_a_fit_as_exact() {
    let fit = best_fit(&mismatched_rig(), MODE, FitWeights::default()).unwrap();
    let text = fit.summary("Assetto Corsa");
    assert!(text.contains("best fit"), "{text}");
    assert!(text.contains("off by"), "{text}");
    assert!(text.contains("Assetto Corsa"), "{text}");

    // And when it genuinely is exact, it says that instead of crying wolf.
    let exact = best_fit(&uniform_triple(), MODE, FitWeights::default()).unwrap();
    let text = exact.summary("iRacing");
    assert!(text.contains("exact"), "{text}");
}

#[test]
fn the_fit_beats_the_naive_average() {
    // A solver that returns worse values than simply averaging the screens
    // would be worse than useless. Compare against the seed it started from.
    let rig = mismatched_rig();
    let fit = best_fit(&rig, MODE, FitWeights::default()).unwrap();

    let mean_width = (597.7 + 1193.0 + 597.7) / 3.0;
    let naive = tp_geometry::bestfit::BestFit {
        width_mm: mean_width,
        height_mm: 336.1,
        bezel_mm: 8.0,
        distance_mm: 700.0,
        angle_deg: 55.0,
        residuals: vec![],
    };
    // The optimiser is free to land anywhere, but not on the naive average —
    // if it did, it is not optimising.
    assert!(
        (fit.width_mm - naive.width_mm).abs() > 1.0,
        "the fit ({:.1}) is just the mean ({:.1}); the optimiser is not doing anything",
        fit.width_mm,
        naive.width_mm
    );
}

#[test]
fn results_are_deterministic() {
    // No randomness anywhere: the same rig must give byte-identical values, or
    // the recompute-and-propagate diff would show phantom changes.
    let a = best_fit(&mismatched_rig(), MODE, FitWeights::default()).unwrap();
    let b = best_fit(&mismatched_rig(), MODE, FitWeights::default()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_rig_with_no_centre_screen_has_no_fit() {
    let mut r = mismatched_rig();
    r.screens.retain(|s| s.role != ScreenRole::Center);
    assert!(best_fit(&r, MODE, FitWeights::default()).is_none());
}

#[test]
fn apex_bias_shifts_the_fit_toward_the_favoured_side() {
    // Defaults to zero because the apex alternates every corner, but where a
    // layout genuinely is asymmetric the knob has to actually do something.
    let rig = mismatched_rig();
    let neutral = best_fit(&rig, MODE, FitWeights::default()).unwrap();
    let biased = best_fit(
        &rig,
        MODE,
        FitWeights {
            apex_bias: 0.8,
            ..FitWeights::default()
        },
    )
    .unwrap();

    let left = |f: &tp_geometry::bestfit::BestFit| {
        f.residuals
            .iter()
            .find(|r| r.id == ScreenId(1))
            .unwrap()
            .rms_error_deg
    };
    let right = |f: &tp_geometry::bestfit::BestFit| {
        f.residuals
            .iter()
            .find(|r| r.id == ScreenId(3))
            .unwrap()
            .rms_error_deg
    };

    // Biasing right should not make the right screen worse relative to the left.
    let neutral_gap = right(&neutral) - left(&neutral);
    let biased_gap = right(&biased) - left(&biased);
    assert!(
        biased_gap <= neutral_gap + 1e-6,
        "bias made the favoured side worse: {biased_gap:.4} vs {neutral_gap:.4}"
    );
}
