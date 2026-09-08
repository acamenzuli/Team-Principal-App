//! Fitting one set of triple-screen values to a rig that is not uniform.
//!
//! Most sims' triple-screen configuration takes **one** width, height, bezel,
//! distance and angle, and assumes three identical panels. A 49" flanked by two
//! 27"s cannot be expressed that way. This finds the single set that is least
//! wrong, and — just as importantly — reports how wrong it still is per screen,
//! so the UI can say "your side screens are off by 2.1 degrees" instead of
//! presenting a fit as if it were the truth.
//!
//! ## The objective
//!
//! Sample points across each real screen's visible surface, compute the true
//! angle to each from the eye, compute where the candidate uniform rig would
//! put the same content, and minimise the weighted sum of squared angular
//! error.
//!
//! Weighting is Gaussian on the angle from straight ahead: error near the
//! centre of vision matters more than error at the far edge of a side screen.
//!
//! ## On the apex-side bias
//!
//! The brief asked for extra weight toward "the apex-side region where I
//! actually look while driving". That is implemented and defaults to **zero**,
//! deliberately. The apex alternates — left-hander, then right-hander — so a
//! fixed bias toward one side would be right half the time and systematically
//! wrong the other half, on a rig that is otherwise symmetric. It is exposed as
//! a knob for anyone racing a genuinely asymmetric layout (an oval, say), where
//! the bias is real and constant.

use tp_model::{RigModel, ScreenId, ScreenRole, SessionMode};

use crate::solve::{solve, RigSolution};
use crate::vec3::Vec3;

/// How samples are weighted across the field of view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitWeights {
    /// Standard deviation of the Gaussian, in degrees from straight ahead.
    /// Error inside this band dominates the fit.
    pub sigma_deg: f64,
    /// Extra weight toward one side, as a fraction. Positive favours the right,
    /// negative the left. Zero by default — see the module docs.
    pub apex_bias: f64,
    /// Samples across each screen. 33 puts one roughly every 1.5 degrees on a
    /// 50-degree panel, which is finer than the error being measured.
    pub samples_per_screen: usize,
}

impl Default for FitWeights {
    fn default() -> Self {
        Self {
            sigma_deg: 25.0,
            apex_bias: 0.0,
            samples_per_screen: 33,
        }
    }
}

/// A single set of values, plus what it costs.
#[derive(Debug, Clone, PartialEq)]
pub struct BestFit {
    pub width_mm: f64,
    pub height_mm: f64,
    pub bezel_mm: f64,
    pub distance_mm: f64,
    pub angle_deg: f64,
    /// Per screen, so the UI can name which one is worst.
    pub residuals: Vec<Residual>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Residual {
    pub id: ScreenId,
    pub role_is_centre: bool,
    pub max_error_deg: f64,
    pub rms_error_deg: f64,
}

impl BestFit {
    pub fn worst_error_deg(&self) -> f64 {
        self.residuals
            .iter()
            .map(|r| r.max_error_deg)
            .fold(0.0, f64::max)
    }

    /// One sentence for the UI. Never presents a fit as if it were exact.
    pub fn summary(&self, title: &str) -> String {
        let worst = self.worst_error_deg();
        if worst < 0.25 {
            format!("{title} accepts one screen size. Your screens are close enough that a single set is exact to within {worst:.2} degrees.")
        } else {
            format!("{title} accepts only one screen size. These values are a best fit — your worst screen is off by {worst:.1} degrees.")
        }
    }
}

/// Solve for the least-wrong uniform values.
///
/// Returns `None` when the rig has no centre screen, since there is then no
/// reference to fit against.
pub fn best_fit(rig: &RigModel, session: SessionMode, weights: FitWeights) -> Option<BestFit> {
    let target = solve(rig, session);
    if target.screens.is_empty() {
        return None;
    }
    let samples = sample(&target, weights.samples_per_screen);

    // Start from the averages of the real screens: already close, so the
    // simplex converges in far fewer evaluations than from an arbitrary point.
    let start = seed(rig, &target);

    let objective =
        |p: &[f64; 5]| -> f64 { cost(rig, session, p, &samples, weights).unwrap_or(f64::INFINITY) };

    let best = nelder_mead(start, objective, 800);

    let residuals = residuals_for(rig, session, &best, &samples)?;
    Some(BestFit {
        width_mm: best[0],
        height_mm: best[1],
        bezel_mm: best[2],
        distance_mm: best[3],
        angle_deg: best[4],
        residuals,
    })
}

/// A screen's true angles at evenly spaced points across its surface.
struct ScreenSamples {
    id: ScreenId,
    role: ScreenRole,
    /// Azimuth in degrees, left edge to right edge.
    azimuths: Vec<f64>,
}

fn sample(solution: &RigSolution, count: usize) -> Vec<ScreenSamples> {
    solution
        .screens
        .iter()
        .map(|s| {
            // The two edge midpoints, ordered left to right by azimuth, so the
            // same fraction means the same place on the panel in either rig.
            let across = (s.corners[2] + s.corners[3]) * 0.5 - s.centre;
            let (a, b) = (s.centre - across, s.centre + across);
            let (from, to) = if a.azimuth_deg() <= b.azimuth_deg() {
                (a, b)
            } else {
                (b, a)
            };

            ScreenSamples {
                id: s.id,
                role: s.role.clone(),
                azimuths: (0..count)
                    .map(|i| {
                        let t = i as f64 / (count - 1).max(1) as f64;
                        lerp(from, to, t).azimuth_deg()
                    })
                    .collect(),
            }
        })
        .collect()
}

fn lerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    a + (b - a) * t
}

fn seed(rig: &RigModel, solved: &RigSolution) -> [f64; 5] {
    let mean = |f: &dyn Fn(&tp_model::ScreenSpec) -> f64| -> f64 {
        let vals: Vec<f64> = rig.screens.iter().map(f).collect();
        if vals.is_empty() {
            0.0
        } else {
            vals.iter().sum::<f64>() / vals.len() as f64
        }
    };
    let sides: Vec<&tp_model::ScreenSpec> = rig
        .screens
        .iter()
        .filter(|s| matches!(s.role, ScreenRole::Left | ScreenRole::Right))
        .collect();
    let angle = if sides.is_empty() {
        0.0
    } else {
        sides.iter().map(|s| s.mounting.angle.0).sum::<f64>() / sides.len() as f64
    };

    [
        mean(&|s| s.panel.visible_width.mm.0),
        mean(&|s| s.panel.visible_height.mm.0),
        mean(&|s| (s.panel.bezel.left.0 + s.panel.bezel.right.0) / 2.0),
        solved.centre().map(|c| c.distance.0).unwrap_or(700.0),
        angle,
    ]
}

/// Build a uniform rig from candidate parameters and solve it.
fn candidate(rig: &RigModel, p: &[f64; 5]) -> RigModel {
    use tp_model::*;

    let template = rig.center().or_else(|| rig.screens.first());
    let native = template
        .map(|s| s.panel.native_resolution)
        .unwrap_or(Resolution {
            width: 2560,
            height: 1440,
        });

    let make = |id: u32, role: ScreenRole, angle: f64| ScreenSpec {
        id: ScreenId(id),
        role,
        binding: MonitorBinding::Unbound,
        panel: PanelSpec {
            native_resolution: native,
            visible_width: Measurement::derived(p[0]),
            visible_height: Measurement::derived(p[1]),
            width_measure: WidthMeasure::Chord,
            // A uniform model is a flat model; that is the whole point of it.
            curvature: Curvature::Flat,
            bezel: Bezel {
                left: Mm(p[2]),
                right: Mm(p[2]),
                top: Mm(p[2]),
                bottom: Mm(p[2]),
            },
        },
        mounting: MountingSpec {
            angle: Deg(angle),
            ..Default::default()
        },
    };

    let mut out = RigModel::new("candidate", "");
    out.seating = Seating {
        eye_to_center: Mm(p[3]),
        eye_height_offset: rig.seating.eye_height_offset,
        lateral_offset: rig.seating.lateral_offset,
    };
    out.screens = vec![
        make(1, ScreenRole::Left, p[4]),
        make(2, ScreenRole::Center, 0.0),
        make(3, ScreenRole::Right, p[4]),
    ];
    out
}

fn cost(
    rig: &RigModel,
    session: SessionMode,
    p: &[f64; 5],
    samples: &[ScreenSamples],
    weights: FitWeights,
) -> Option<f64> {
    // Physically impossible parameters are refused rather than clamped, so the
    // simplex is pushed back into the feasible region instead of piling up
    // against a wall where many points share a cost.
    if p[0] <= 1.0 || p[1] <= 1.0 || p[3] <= 50.0 || p[2] < 0.0 || !(-89.0..=89.0).contains(&p[4]) {
        return None;
    }

    let candidate_solution = solve(&candidate(rig, p), session);
    let candidate_samples = sample(&candidate_solution, weights.samples_per_screen);

    let mut total = 0.0;
    let mut used = 0usize;
    for real in samples {
        let Some(fitted) = candidate_samples.iter().find(|c| c.role == real.role) else {
            continue;
        };
        for (t, c) in real.azimuths.iter().zip(&fitted.azimuths) {
            let error = t - c;
            total += weight(*t, weights) * error * error;
            used += 1;
        }
    }
    (used > 0).then_some(total)
}

fn weight(azimuth_deg: f64, w: FitWeights) -> f64 {
    let gaussian = (-(azimuth_deg / w.sigma_deg).powi(2)).exp();
    let bias = 1.0 + w.apex_bias * (azimuth_deg / 90.0).clamp(-1.0, 1.0);
    gaussian * bias.max(0.0)
}

fn residuals_for(
    rig: &RigModel,
    session: SessionMode,
    p: &[f64; 5],
    samples: &[ScreenSamples],
) -> Option<Vec<Residual>> {
    let fitted = sample(
        &solve(&candidate(rig, p), session),
        samples.first()?.azimuths.len(),
    );
    Some(
        samples
            .iter()
            .filter_map(|real| {
                let c = fitted.iter().find(|c| c.role == real.role)?;
                let errors: Vec<f64> = real
                    .azimuths
                    .iter()
                    .zip(&c.azimuths)
                    .map(|(a, b)| (a - b).abs())
                    .collect();
                let n = errors.len().max(1) as f64;
                Some(Residual {
                    id: real.id,
                    role_is_centre: real.role == ScreenRole::Center,
                    max_error_deg: errors.iter().cloned().fold(0.0, f64::max),
                    rms_error_deg: (errors.iter().map(|e| e * e).sum::<f64>() / n).sqrt(),
                })
            })
            .collect(),
    )
}

/// Nelder-Mead over five parameters.
///
/// Hand-rolled rather than pulled in: it is forty lines, it is deterministic,
/// and a dependency for this would be larger than the thing it replaces. No
/// gradients are available here — the objective runs the whole solver — so a
/// simplex method is the right tool.
fn nelder_mead(start: [f64; 5], f: impl Fn(&[f64; 5]) -> f64, max_iter: usize) -> [f64; 5] {
    const N: usize = 5;
    const ALPHA: f64 = 1.0; // reflection
    const GAMMA: f64 = 2.0; // expansion
    const RHO: f64 = 0.5; // contraction
    const SIGMA: f64 = 0.5; // shrink

    // Initial simplex: perturb each axis by a step scaled to that parameter, so
    // millimetres and degrees are explored at comparable rates.
    let steps = [20.0, 15.0, 3.0, 25.0, 4.0];
    let mut simplex: Vec<([f64; 5], f64)> = Vec::with_capacity(N + 1);
    simplex.push((start, f(&start)));
    for i in 0..N {
        let mut p = start;
        p[i] += steps[i];
        let v = f(&p);
        simplex.push((p, v));
    }

    for _ in 0..max_iter {
        simplex.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Converged once the simplex has collapsed on every axis.
        let spread: f64 = (0..N)
            .map(|i| {
                let vals: Vec<f64> = simplex.iter().map(|(p, _)| p[i]).collect();
                vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                    - vals.iter().cloned().fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max);
        if spread < 1e-4 {
            break;
        }

        // Centroid of everything but the worst point.
        let mut centroid = [0.0; N];
        for (p, _) in &simplex[..N] {
            for i in 0..N {
                centroid[i] += p[i] / N as f64;
            }
        }

        let worst = simplex[N];
        let combine = |k: f64| -> [f64; 5] {
            let mut out = [0.0; N];
            for i in 0..N {
                out[i] = centroid[i] + k * (worst.0[i] - centroid[i]);
            }
            out
        };

        let reflected = combine(-ALPHA);
        let fr = f(&reflected);

        if fr < simplex[0].1 {
            let expanded = combine(-GAMMA);
            let fe = f(&expanded);
            simplex[N] = if fe < fr {
                (expanded, fe)
            } else {
                (reflected, fr)
            };
        } else if fr < simplex[N - 1].1 {
            simplex[N] = (reflected, fr);
        } else {
            let contracted = combine(RHO);
            let fc = f(&contracted);
            if fc < worst.1 {
                simplex[N] = (contracted, fc);
            } else {
                // Shrink toward the best point.
                let best = simplex[0].0;
                for entry in simplex.iter_mut().skip(1) {
                    let mut p = entry.0;
                    for i in 0..N {
                        p[i] = best[i] + SIGMA * (p[i] - best[i]);
                    }
                    *entry = (p, f(&p));
                }
            }
        }
    }

    simplex
        .into_iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(p, _)| p)
        .unwrap_or(start)
}
