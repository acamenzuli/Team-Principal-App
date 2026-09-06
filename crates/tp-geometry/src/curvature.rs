//! Curved panels.
//!
//! Sims project onto flat planes. A curved screen is therefore always an
//! approximation, and the naive handling of it is wrong by a lot — feeding a
//! 49" 1000R panel's arc width straight into an FOV formula overstates the
//! result by more than 15 degrees.
//!
//! Two corrections matter, and they compound:
//!
//! 1. The panel's *edges* are separated by the chord, not the arc.
//! 2. Those edges sit further from the eye than the panel's centre does,
//!    because the middle of the panel bulges toward you by the sagitta.
//!
//! Pairing the chord width with the distance to the *chord plane* handles both
//! at once: the visible edges lie exactly in that plane, so the flat model
//! reproduces the true edge angles by construction. See
//! docs/design/0003-rig-model.md for the worked numbers.

use tp_model::{Curvature, Mm};

/// A curved panel resolved into the flat-plane equivalent a sim can consume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSolution {
    /// Developed surface length — a tape measure laid along the screen.
    pub arc: Mm,
    /// Straight-line distance between the visible left and right edges.
    pub chord: Mm,
    /// How far the centre of the panel bulges toward the eye.
    pub sagitta: Mm,
    /// Angle the panel subtends at its own centre of curvature. Not the FOV.
    pub subtended_deg: f64,
}

impl CurveSolution {
    /// The distance a flat-plane model should use, given the eye-to-nearest-
    /// point distance. This is the half of the answer that is usually missed:
    /// chord width paired with the *nearest-point* distance still overstates
    /// the FOV, because the edges are `sagitta` further away than the centre.
    pub fn chord_plane_distance(&self, eye_to_nearest_point: Mm) -> Mm {
        Mm(eye_to_nearest_point.0 + self.sagitta.0)
    }

    /// True horizontal angular subtense from the eye, in degrees.
    pub fn true_h_fov_deg(&self, eye_to_nearest_point: Mm) -> f64 {
        let d = self.chord_plane_distance(eye_to_nearest_point).0;
        2.0 * ((self.chord.0 / 2.0) / d).atan().to_degrees()
    }

    /// Worst-case angular error introduced by treating the arc as flat.
    ///
    /// Defined numerically rather than in closed form: sample points along the
    /// arc, compare each sample's true angle from the eye against the angle the
    /// flat chord-plane model implies for the same image content, and take the
    /// maximum. The app reports this so the user can judge whether the
    /// approximation matters at their radius instead of being told to trust it.
    pub fn worst_case_error_deg(&self, eye_to_nearest_point: Mm, radius: Mm) -> f64 {
        const SAMPLES: usize = 257;
        let r = radius.0;
        let half_theta = (self.subtended_deg.to_radians()) / 2.0;
        let d_near = eye_to_nearest_point.0;
        // Centre of curvature sits behind the panel's nearest point.
        let cz = d_near + r;
        let d_chord_plane = self.chord_plane_distance(eye_to_nearest_point).0;

        let mut worst: f64 = 0.0;
        for i in 0..SAMPLES {
            let t = i as f64 / (SAMPLES - 1) as f64; // 0..1 across the panel
            let phi = -half_theta + t * 2.0 * half_theta;

            // True position of this point on the arc, eye at the origin.
            let x_true = r * phi.sin();
            let z_true = cz - r * phi.cos();
            let ang_true = x_true.atan2(z_true).to_degrees();

            // Where the flat model puts the same image content: the panel's
            // surface is unrolled linearly onto the chord plane.
            let x_flat = (t - 0.5) * self.chord.0;
            let ang_flat = x_flat.atan2(d_chord_plane).to_degrees();

            worst = worst.max((ang_true - ang_flat).abs());
        }
        worst
    }
}

/// Resolve a panel's curvature into arc, chord and sagitta.
///
/// `width` is interpreted per `measured_as`. A flat panel round-trips
/// unchanged with zero sagitta, so callers need no special case.
pub fn solve(
    width: Mm,
    curvature: Curvature,
    measured_as: tp_model::WidthMeasure,
) -> CurveSolution {
    let Some(radius) = curvature.radius() else {
        return CurveSolution {
            arc: width,
            chord: width,
            sagitta: Mm::ZERO,
            subtended_deg: 0.0,
        };
    };
    let r = radius.0;

    let (arc, chord, theta) = match measured_as {
        tp_model::WidthMeasure::Arc => {
            let theta = width.0 / r;
            (width.0, 2.0 * r * (theta / 2.0).sin(), theta)
        }
        tp_model::WidthMeasure::Chord => {
            // asin is only defined for chord <= 2r; a chord wider than the
            // diameter is a data-entry error, so clamp rather than produce NaN.
            let half = (width.0 / (2.0 * r)).clamp(-1.0, 1.0);
            let theta = 2.0 * half.asin();
            (r * theta, width.0, theta)
        }
    };

    CurveSolution {
        arc: Mm(arc),
        chord: Mm(chord),
        sagitta: Mm(r * (1.0 - (theta / 2.0).cos())),
        subtended_deg: theta.to_degrees(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_model::WidthMeasure;

    /// The worked example from docs/design/0003-rig-model.md. If this test
    /// changes, that document is wrong and must be updated with it.
    #[test]
    fn forty_nine_inch_1000r_reference() {
        let s = solve(
            Mm(1193.0),
            Curvature::Radius { radius: Mm(1000.0) },
            WidthMeasure::Arc,
        );

        assert!(
            (s.subtended_deg - 68.35).abs() < 0.01,
            "subtended {}",
            s.subtended_deg
        );
        assert!((s.chord.0 - 1123.5).abs() < 0.1, "chord {}", s.chord.0);
        assert!((s.sagitta.0 - 172.7).abs() < 0.1, "sagitta {}", s.sagitta.0);

        let eye = Mm(700.0);
        assert!((s.chord_plane_distance(eye).0 - 872.7).abs() < 0.1);

        // The number that matters.
        let correct = s.true_h_fov_deg(eye);
        assert!((correct - 65.54).abs() < 0.02, "correct hFOV {correct}");

        // The two wrong answers, asserted so the difference is documented in
        // code and cannot quietly stop being true.
        let naive_arc_flat = 2.0 * ((1193.0 / 2.0f64) / 700.0).atan().to_degrees();
        assert!(
            (naive_arc_flat - 80.87).abs() < 0.02,
            "naive {naive_arc_flat}"
        );
        let chord_at_near = 2.0 * ((s.chord.0 / 2.0) / 700.0).atan().to_degrees();
        assert!(
            (chord_at_near - 77.49).abs() < 0.02,
            "half-right {chord_at_near}"
        );

        assert!(
            naive_arc_flat - correct > 15.0,
            "the error we exist to prevent"
        );
    }

    #[test]
    fn arc_and_chord_are_inverses() {
        let curve = Curvature::Radius { radius: Mm(1000.0) };
        let from_arc = solve(Mm(1193.0), curve, WidthMeasure::Arc);
        let from_chord = solve(from_arc.chord, curve, WidthMeasure::Chord);
        assert!((from_chord.arc.0 - 1193.0).abs() < 1e-6);
        assert!((from_chord.sagitta.0 - from_arc.sagitta.0).abs() < 1e-6);
    }

    #[test]
    fn flat_panels_pass_straight_through() {
        let s = solve(Mm(597.7), Curvature::Flat, WidthMeasure::Arc);
        assert_eq!(s.arc, s.chord);
        assert_eq!(s.sagitta, Mm::ZERO);
        assert_eq!(s.subtended_deg, 0.0);
        // A flat panel's "true" FOV is just the textbook formula.
        let expected = 2.0 * ((597.7 / 2.0f64) / 700.0).atan().to_degrees();
        assert!((s.true_h_fov_deg(Mm(700.0)) - expected).abs() < 1e-9);
    }

    #[test]
    fn gentler_curves_approximate_better() {
        // 1800R should be measurably more flat-like than 1000R. If this ever
        // inverts, the sagitta sign is wrong.
        let arc = Mm(1193.0);
        let tight = solve(
            arc,
            Curvature::Radius { radius: Mm(1000.0) },
            WidthMeasure::Arc,
        );
        let gentle = solve(
            arc,
            Curvature::Radius { radius: Mm(1800.0) },
            WidthMeasure::Arc,
        );
        assert!(gentle.sagitta.0 < tight.sagitta.0);
        assert!(
            gentle.worst_case_error_deg(Mm(700.0), Mm(1800.0))
                < tight.worst_case_error_deg(Mm(700.0), Mm(1000.0))
        );
    }

    #[test]
    fn a_chord_wider_than_the_diameter_is_clamped_not_nan() {
        let s = solve(
            Mm(5000.0),
            Curvature::Radius { radius: Mm(1000.0) },
            WidthMeasure::Chord,
        );
        assert!(s.arc.0.is_finite() && s.sagitta.0.is_finite());
    }
}
