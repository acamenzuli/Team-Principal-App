//! Tauri commands — the entire surface the frontend can reach.
//!
//! The frontend never talks to Win32. It talks to these, and every type they
//! exchange lives in `tp-model`, which is exported to TypeScript so the
//! contract cannot drift silently. `scripts/check-ipc.mjs` additionally asserts
//! that every command here has a wrapper in `src/ipc.ts`, because ts-rs
//! generates payload types but knows nothing about command *names*.

use tauri::State;
use tp_model::{
    AccentPreset, AppInfo, BestFitInfo, CurveResult, DesktopLayoutInfo, DetectedDevice, GapInfo,
    LengthUnit, LoadedPreferences, MonitorInfo, MonitorPitch, ParsedLength, Preferences,
    ResidualInfo, RigModel, RigSolutionInfo, RigWarningInfo, ScreenSolutionInfo, SessionMode,
    SpanInfo,
};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::providers::Providers;

#[tauri::command]
pub fn app_info(providers: State<'_, Providers>) -> AppInfo {
    let awareness = crate::dpi_awareness();
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        milestone: crate::MILESTONE,
        simulated: providers.simulated,
        dpi_awareness: format!("{awareness:?}"),
        dpi_awareness_ok: awareness.is_acceptable(),
        log_dir: crate::logging::app_data_dir()
            .join("logs")
            .display()
            .to_string(),
    }
}

#[tauri::command]
pub fn list_monitors(providers: State<'_, Providers>) -> AppResult<Vec<MonitorInfo>> {
    providers.display.enumerate()
}

/// The current device list.
///
/// Read from the watch thread's cache rather than rescanned: the watcher is
/// already the authority, it has run the statuses through their debouncers, and
/// a second enumeration path would be a second answer to the same question.
///
/// Against fixtures there is no watcher, so the mock provider answers instead.
#[tauri::command]
pub fn list_devices(
    providers: State<'_, Providers>,
    watch: State<'_, crate::peripherals::watch::PeripheralWatch>,
) -> AppResult<Vec<DetectedDevice>> {
    match &watch.0 {
        Some(w) => Ok(w.latest()),
        None => providers.peripherals.enumerate(),
    }
}

/// Ask for an immediate rescan. The result arrives as a `peripherals://changed`
/// event like any other, so there is one path into the UI rather than two.
#[tauri::command]
pub fn refresh_devices(watch: State<'_, crate::peripherals::watch::PeripheralWatch>) {
    if let Some(w) = &watch.0 {
        w.poke();
    }
}

/// The virtual desktop's bounding box and its dead regions.
///
/// Separate from `list_monitors` because it is derived rather than detected:
/// the trait computes it from the enumerated rectangles, so the real and mock
/// providers cannot disagree about it.
#[tauri::command]
pub fn desktop_layout(providers: State<'_, Providers>) -> AppResult<Option<DesktopLayoutInfo>> {
    let monitors = providers.display.enumerate()?;
    let Some(layout) = providers.display.desktop_layout()? else {
        return Ok(None);
    };

    // Pitch per monitor, left to right, each compared with its left neighbour.
    let mut pitches: Vec<MonitorPitch> = Vec::with_capacity(monitors.len());
    let mut previous: Option<f64> = None;
    for m in &monitors {
        let px_per_mm = m
            .physical_size
            .and_then(|p| tp_geometry::pixel_pitch(m.native_resolution.width, p.width.0));
        let differs = match (previous, px_per_mm) {
            (Some(prev), Some(now)) => tp_geometry::pitch_mismatch(prev, now) > 0.10,
            _ => false,
        };
        if px_per_mm.is_some() {
            previous = px_per_mm;
        }
        pitches.push(MonitorPitch {
            device_path: m.device_path.clone(),
            px_per_mm,
            differs_from_neighbour: differs,
        });
    }

    Ok(Some(DesktopLayoutInfo {
        bounds: layout.bounds,
        dead_area: layout.dead_area() as f64,
        covered_area: layout.covered_area as f64,
        is_gapless: layout.is_gapless(),
        dead_regions: layout.dead_regions,
        pitches,
    }))
}

/// Parse a length the way the user typed it — `47.5in`, `1200mm`, `47 1/2"`.
///
/// Lives in Rust rather than TypeScript so there is exactly one parser in the
/// product and it is the one covered by tests.
#[tauri::command]
pub fn parse_length(input: String, unit: LengthUnit) -> AppResult<ParsedLength> {
    let mm =
        tp_geometry::parse_length(&input, unit).map_err(|e| AppError::Config(e.to_string()))?;
    Ok(ParsedLength {
        mm: mm.0,
        formatted_mm: tp_geometry::format_length(mm, LengthUnit::Mm),
        formatted_cm: tp_geometry::format_length(mm, LengthUnit::Cm),
        formatted_inch: tp_geometry::format_length(mm, LengthUnit::Inch),
    })
}

/// Resolve a curved panel into the numbers a flat-plane sim can consume.
///
/// Exposed this early because it is the most load-bearing piece of math in the
/// product, and Screen Setup will need it live as the user types.
#[tauri::command]
pub fn solve_curvature(
    arc_or_chord_mm: f64,
    radius_mm: Option<f64>,
    measured_as_chord: bool,
    eye_distance_mm: f64,
) -> CurveResult {
    use tp_model::{Curvature, Mm, WidthMeasure};

    let curvature = match radius_mm {
        Some(r) if r > 0.0 => Curvature::Radius { radius: Mm(r) },
        _ => Curvature::Flat,
    };
    let measure = if measured_as_chord {
        WidthMeasure::Chord
    } else {
        WidthMeasure::Arc
    };
    let s = tp_geometry::solve_curvature(Mm(arc_or_chord_mm), curvature, measure);
    let eye = Mm(eye_distance_mm);

    CurveResult {
        arc_mm: s.arc.0,
        chord_mm: s.chord.0,
        sagitta_mm: s.sagitta.0,
        subtended_deg: s.subtended_deg,
        chord_plane_distance_mm: s.chord_plane_distance(eye).0,
        h_fov_deg: s.true_h_fov_deg(eye),
        naive_h_fov_deg: 2.0 * ((s.arc.0 / 2.0) / eye.0).atan().to_degrees(),
        worst_case_error_deg: radius_mm
            .filter(|r| *r > 0.0)
            .map(|r| s.worst_case_error_deg(eye, Mm(r))),
    }
}

#[tauri::command]
pub fn get_preferences() -> LoadedPreferences {
    let (preferences, problem) = crate::settings::load();
    LoadedPreferences {
        accent_foreground: tp_model::accent_foreground(&preferences.appearance.accent).to_string(),
        preferences,
        problem,
    }
}

#[tauri::command]
pub fn save_preferences(preferences: Preferences) -> AppResult<LoadedPreferences> {
    let saved = crate::settings::save(&preferences)?;
    Ok(LoadedPreferences {
        accent_foreground: tp_model::accent_foreground(&saved.appearance.accent).to_string(),
        preferences: saved,
        problem: None,
    })
}

/// The named accents offered in settings. A custom hex is always allowed.
#[tauri::command]
pub fn accent_presets() -> Vec<AccentPreset> {
    tp_model::accent_presets()
        .into_iter()
        .map(|(name, hex)| AccentPreset {
            name: name.to_string(),
            hex: hex.to_string(),
            foreground: tp_model::accent_foreground(hex).to_string(),
        })
        .collect()
}

// ------------------------------------------------------------------- the rig

/// Every saved rig, newest first.
#[tauri::command]
pub fn list_rigs() -> Vec<RigModel> {
    crate::rig::list()
}

/// The rig to work on: the most recently updated, or a fresh one built from the
/// monitors currently plugged in.
///
/// Never returns nothing. A first run should land on Screen Setup with the
/// detectable half already filled in, not on an empty form.
#[tauri::command]
pub fn current_rig(providers: State<'_, Providers>) -> RigModel {
    if let Some(rig) = crate::rig::list().into_iter().next() {
        return rig;
    }
    let monitors = providers.display.enumerate().unwrap_or_default();
    crate::rig::detect::from_monitors(&monitors, None)
}

#[tauri::command]
pub fn save_rig(rig: RigModel) -> AppResult<RigModel> {
    crate::rig::save(&rig)
}

#[tauri::command]
pub fn delete_rig(id: Uuid) -> AppResult<()> {
    crate::rig::delete(id)
}

/// Re-read the monitors and fold them into a rig, keeping every measurement.
///
/// Screens are matched by EDID identity, so a cable swap or a rearranged
/// desktop cannot lose the bezel and angle numbers someone measured by hand.
#[tauri::command]
pub fn detect_rig(
    providers: State<'_, Providers>,
    existing: Option<RigModel>,
) -> AppResult<RigModel> {
    let monitors = providers.display.enumerate()?;
    Ok(crate::rig::detect::from_monitors(
        &monitors,
        existing.as_ref(),
    ))
}

/// Solve a rig without saving it, so the UI can show live numbers as fields
/// change. The geometry crate is the only place this maths exists.
#[tauri::command]
pub fn solve_rig(rig: RigModel, session: SessionMode) -> RigSolutionInfo {
    to_wire(tp_geometry::solve(&rig, session))
}

/// Fit one set of triple-screen values to a rig that is not uniform, for titles
/// that accept only one.
#[tauri::command]
pub fn fit_rig(rig: RigModel, session: SessionMode) -> Option<BestFitInfo> {
    let uniform = tp_geometry::solve::screens_are_uniform(&rig);
    let fit = tp_geometry::best_fit(&rig, session, tp_geometry::FitWeights::default())?;
    Some(BestFitInfo {
        width_mm: fit.width_mm,
        height_mm: fit.height_mm,
        bezel_mm: fit.bezel_mm,
        distance_mm: fit.distance_mm,
        angle_deg: fit.angle_deg,
        worst_error_deg: fit.worst_error_deg(),
        // Uniform screens *and* a negligible residual. Either alone could
        // mislead, and the UI must never present a fit as exact when it is not.
        is_exact: uniform && fit.worst_error_deg() < 0.25,
        residuals: fit
            .residuals
            .iter()
            .map(|r| ResidualInfo {
                id: r.id,
                max_error_deg: r.max_error_deg,
                rms_error_deg: r.rms_error_deg,
            })
            .collect(),
    })
}

fn to_wire(s: tp_geometry::RigSolution) -> RigSolutionInfo {
    RigSolutionInfo {
        total_coverage_deg: s.total_coverage.0,
        visible_coverage_deg: s.visible_coverage.0,
        warnings: s.warnings.iter().map(warning_to_wire).collect(),
        screens: s
            .screens
            .iter()
            .map(|screen| ScreenSolutionInfo {
                id: screen.id,
                role: screen.role.clone(),
                distance_mm: screen.distance.0,
                flat_width_mm: screen.flat.width.0,
                flat_height_mm: screen.flat.height.0,
                h_fov_deg: screen.h_fov.0,
                v_fov_deg: screen.v_fov.0,
                span: SpanInfo {
                    left_deg: screen.span.left.0,
                    right_deg: screen.span.right.0,
                    bottom_deg: screen.span.bottom.0,
                    top_deg: screen.span.top.0,
                    asymmetry_deg: screen.span.asymmetry(),
                },
                px_per_deg_h: screen.px_per_deg_h,
                px_per_deg_v: screen.px_per_deg_v,
                inner_gap: screen.inner_gap.map(|g| GapInfo {
                    mm: g.mm.0,
                    px: g.px,
                    deg: g.deg.0,
                }),
                curvature_error_deg: screen.curvature_error.map(|d| d.0),
                centre: [screen.centre.x, screen.centre.y, screen.centre.z],
                corners: screen.corners.map(|c| [c.x, c.y, c.z]),
            })
            .collect(),
    }
}

fn warning_to_wire(w: &tp_geometry::Warning) -> RigWarningInfo {
    use tp_geometry::Warning::*;
    let (kind, screen_id) = match w {
        NoCentreScreen => ("no_centre_screen", None),
        FovTooWide { id, .. } => ("fov_too_wide", Some(*id)),
        FovTooNarrow { id, .. } => ("fov_too_narrow", Some(*id)),
        SeatingTooClose { .. } => ("seating_too_close", None),
        ScreenEdgeBehindEye { id } => ("screen_edge_behind_eye", Some(*id)),
        PitchMismatch { id, .. } => ("pitch_mismatch", Some(*id)),
        CurvatureErrorHigh { id, .. } => ("curvature_error_high", Some(*id)),
        MissingPhysicalSize { id } => ("missing_physical_size", Some(*id)),
    };
    RigWarningInfo {
        kind: kind.to_string(),
        message: w.message(),
        screen_id,
    }
}

// ------------------------------------------------------------ input monitor

/// Start watching one device's axes and buttons.
///
/// At most one device is monitored at a time: only one is on screen, and
/// reading the rest would be work nobody asked for. Starting a new one replaces
/// the previous.
#[tauri::command]
pub fn start_input_monitor(
    app: tauri::AppHandle,
    active: State<'_, crate::peripherals::monitor::ActiveMonitor>,
    instance_path: String,
) -> AppResult<()> {
    let monitor = crate::peripherals::monitor::start(app, instance_path);
    match active.0.lock() {
        // Dropping the previous monitor stops its thread.
        Ok(mut slot) => {
            *slot = Some(monitor);
            Ok(())
        }
        Err(_) => Err(AppError::Config(
            "the input monitor is in a bad state".into(),
        )),
    }
}

#[tauri::command]
pub fn stop_input_monitor(active: State<'_, crate::peripherals::monitor::ActiveMonitor>) {
    if let Ok(mut slot) = active.0.lock() {
        *slot = None;
    }
}
