//! Tauri commands — the entire surface the frontend can reach.
//!
//! The frontend never talks to Win32. It talks to these, and every type they
//! exchange lives in `tp-model`, which is exported to TypeScript so the
//! contract cannot drift silently. `scripts/check-ipc.mjs` additionally asserts
//! that every command here has a wrapper in `src/ipc.ts`, because ts-rs
//! generates payload types but knows nothing about command *names*.

use tauri::State;
use tp_model::{
    AccentPreset, AppInfo, CurveResult, DesktopLayoutInfo, DetectedDevice, LengthUnit,
    LoadedPreferences, MonitorInfo, MonitorPitch, ParsedLength, Preferences,
};

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
