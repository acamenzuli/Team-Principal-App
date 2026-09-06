//! Tauri commands — the entire surface the frontend can reach.
//!
//! The frontend never talks to Win32. It talks to these, and every type they
//! exchange lives in `tp-model`, which is exported to TypeScript so the
//! contract cannot drift silently. `scripts/check-ipc.mjs` additionally asserts
//! that every command here has a wrapper in `src/ipc.ts`, because ts-rs
//! generates payload types but knows nothing about command *names*.

use tauri::State;
use tp_model::{AppInfo, CurveResult, DetectedDevice, LengthUnit, MonitorInfo, ParsedLength};

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

#[tauri::command]
pub fn list_devices(providers: State<'_, Providers>) -> AppResult<Vec<DetectedDevice>> {
    providers.peripherals.enumerate()
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
