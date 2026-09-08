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
use crate::launcher::run::Request;
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

// ----------------------------------------------------------- window control

/// Every top-level window, so the test panel can show what the matcher sees.
#[tauri::command]
pub fn list_windows() -> Vec<tp_model::OpenWindow> {
    crate::window::enumerate_windows()
        .into_iter()
        .map(|c| tp_model::OpenWindow {
            plausible: c.is_plausible_game_window((640, 480)),
            candidate: c,
        })
        .collect()
}

/// Place a window and keep it there.
///
/// The result is read back from the window after the change rather than
/// inferred from a return value: UIPI refuses these calls silently when the
/// target runs at a higher integrity level, and no return value distinguishes
/// that from success.
#[tauri::command]
pub fn place_window(
    state: State<'_, crate::window::watchdog::ActiveWatchdog>,
    hwnd: String,
    rect: tp_model::PixelRect,
    means: tp_model::RectMeans,
    borderless: bool,
    watch: bool,
) -> AppResult<tp_model::WindowResult> {
    let handle: u64 = hwnd
        .parse()
        .map_err(|_| AppError::Config(format!("{hwnd:?} is not a window handle")))?;

    let applied = crate::window::apply_geometry(handle, rect, means, borderless, false)?;

    // Replacing the previous watchdog stops it: only one window is being
    // managed at a time, and two threads fighting over one window would be
    // worse than none.
    let watching = if watch {
        let policy = tp_model::WatchdogPolicy::default();
        if let Ok(mut slot) = state.0.lock() {
            *slot = Some(crate::window::Watchdog::start(
                handle, rect, means, borderless, policy,
            ));
        }
        true
    } else {
        if let Ok(mut slot) = state.0.lock() {
            *slot = None;
        }
        false
    };

    let title = crate::window::enumerate_windows()
        .into_iter()
        .find(|c| c.hwnd == handle)
        .map(|c| c.title)
        .unwrap_or_default();

    Ok(tp_model::WindowResult {
        hwnd,
        title,
        matched_by: Vec::new(),
        requested: applied.requested,
        actual_outer: applied.actual.outer,
        actual_client: applied.actual.client,
        borderless: applied.borderless,
        watching,
    })
}

/// Stop putting a window back when the game moves it.
#[tauri::command]
pub fn stop_watching_window(state: State<'_, crate::window::watchdog::ActiveWatchdog>) {
    if let Ok(mut slot) = state.0.lock() {
        *slot = None;
    }
}

// ------------------------------------------------------------- game discovery

/// Every game the launchers say is installed.
///
/// Read from Steam's own library index and Epic's manifests, so nobody has to
/// type an install path. A manifest can outlive the files it describes, so a
/// game whose folder is gone is not listed — a launch that fails for no visible
/// reason is worse than an absent row.
#[tauri::command]
pub fn discover_games() -> Vec<tp_model::InstalledGameInfo> {
    crate::launcher::discover()
        .into_iter()
        .map(|g| tp_model::InstalledGameInfo {
            name: g.name,
            install_path: g.install_path.display().to_string(),
            launcher: match &g.source {
                crate::launcher::GameSource::Steam { .. } => "steam".into(),
                crate::launcher::GameSource::Epic { .. } => "epic".into(),
            },
            launch_uri: crate::launcher::launch_uri(&g.source),
            // Adapters arrive in milestone 10. Claiming otherwise would be the
            // exact kind of aspirational UI this project avoids.
            has_adapter: false,
        })
        .collect()
}

// ------------------------------------------------------------- launch runs

/// Start a preflight run for a saved profile.
///
/// The steps come back as `launch://step` events rather than in the return
/// value: the checklist is a view over a stream, which is what lets a
/// self-healing check turn green on its own by the same path its first result
/// took.
#[tauri::command]
pub fn start_preflight(
    app: tauri::AppHandle,
    active: State<'_, crate::launcher::run::ActiveRun>,
    watch: State<'_, crate::peripherals::watch::PeripheralWatch>,
    profile_id: Uuid,
) -> AppResult<Vec<tp_model::StepView>> {
    let mut profile = crate::profiles::load(profile_id)?;
    profile.steps = tp_model::build_steps(&profile);

    // Device status comes from the watch thread, which has already debounced
    // it. Re-enumerating here would be a second answer to the same question.
    let devices: crate::launcher::run::DeviceSource = match &watch.0 {
        Some(w) => {
            let w = w.clone();
            std::sync::Arc::new(move || w.latest())
        }
        None => std::sync::Arc::new(Vec::new),
    };

    let run = crate::launcher::run::start(app, profile, devices)?;
    let views = run.views();

    // Replacing the previous run cancels it: two preflights racing over the
    // same utilities would be worse than one.
    if let Ok(mut slot) = active.0.lock() {
        if let Some(previous) = slot.take() {
            previous.cancel();
        }
        *slot = Some(run);
    }
    Ok(views)
}

/// Cancel a run and tear down whatever it started.
#[tauri::command]
pub fn cancel_preflight(active: State<'_, crate::launcher::run::ActiveRun>) {
    if let Ok(mut slot) = active.0.lock() {
        if let Some(run) = slot.take() {
            run.cancel();
        }
    }
}

/// Queue a request for the running preflight.
///
/// Queued rather than applied: the driver thread owns the scheduler, and
/// mutating it from here would race the loop being steered.
fn ask(active: &State<'_, crate::launcher::run::ActiveRun>, request: Request) -> AppResult<()> {
    let slot = active
        .0
        .lock()
        .map_err(|_| AppError::Config("the launch state is unreadable".into()))?;
    let run = slot
        .as_ref()
        .ok_or_else(|| AppError::Config("nothing is running to act on".into()))?;
    run.request(request);
    Ok(())
}

/// Re-run one step and everything downstream of it.
///
/// Unlike self-healing, this re-runs the step's *action* — that is what the
/// user asked for by pressing the button. Downstream steps go with it, so a
/// utility that depends on the retried one is not left holding a stale result.
#[tauri::command]
pub fn retry_step(active: State<'_, crate::launcher::run::ActiveRun>, id: u32) -> AppResult<()> {
    ask(&active, Request::Retry(tp_model::StepId(id)))
}

/// Stop one step blocking the gate.
///
/// The row becomes Skipped, never Passed: the checklist keeps saying this was
/// overridden rather than satisfied.
#[tauri::command]
pub fn skip_step(active: State<'_, crate::launcher::run::ActiveRun>, id: u32) -> AppResult<()> {
    ask(&active, Request::Skip(tp_model::StepId(id)))
}

/// The second click. Open the gate and run the launch phase.
///
/// `force` is "race anyway": it skips whatever is still failing so the gate
/// opens. Without it the scheduler's phase gate holds the launch, which is why
/// this cannot start a game behind a blocked preflight even if asked.
#[tauri::command]
pub fn launch_game(
    active: State<'_, crate::launcher::run::ActiveRun>,
    force: bool,
) -> AppResult<()> {
    ask(&active, Request::Launch { force })
}

// ----------------------------------------------------------------- profiles

#[tauri::command]
pub fn list_profiles() -> Vec<tp_model::Profile> {
    crate::profiles::list()
}

#[tauri::command]
pub fn save_profile(profile: tp_model::Profile) -> AppResult<tp_model::Profile> {
    crate::profiles::save(&profile)
}

#[tauri::command]
pub fn delete_profile(id: Uuid) -> AppResult<()> {
    crate::profiles::delete(id)
}

/// A profile for a game that has none yet, saved and returned.
///
/// Deliberately empty of utilities and peripherals rather than guessing at
/// them: a preflight that checks things nobody asked for is one people learn to
/// ignore.
#[tauri::command]
pub fn create_profile(
    name: String,
    launch_uri: String,
    install_path: Option<String>,
) -> AppResult<tp_model::Profile> {
    let launch = if launch_uri.is_empty() {
        tp_model::LaunchMethod::Executable {
            path: install_path.clone().unwrap_or_default(),
            args: Vec::new(),
            working_dir: None,
        }
    } else {
        tp_model::LaunchMethod::Uri { uri: launch_uri }
    };
    crate::profiles::save(&crate::profiles::starter(&name, launch, install_path))
}

// ----------------------------------------------------------- display control

/// Every mode each attached output can run.
///
/// Enumerated per output rather than once, because two panels on the same
/// adapter do not share a mode list and offering the union of them is how a
/// picker ends up suggesting a mode that blacks out one screen.
#[tauri::command]
pub fn available_modes(
    providers: State<'_, Providers>,
) -> AppResult<Vec<tp_model::AvailableModes>> {
    Ok(providers
        .display
        .enumerate()?
        .into_iter()
        .map(|m| tp_model::AvailableModes {
            modes: crate::display::apply::available_modes(&m.gdi_name),
            device_path: m.device_path,
        })
        .collect())
}

/// The desktop exactly as it is now, in the shape a plan takes.
///
/// The starting point for the editor, and the thing a plan is diffed against.
#[tauri::command]
pub fn current_topology(providers: State<'_, Providers>) -> AppResult<tp_model::TopologySnapshot> {
    let monitors = providers.display.enumerate()?;
    crate::display::capture_snapshot(&monitors)
}

/// What a plan would do, and what is wrong with it. Nothing is written.
///
/// The preview-before-apply rule, applied to the one thing in this app that can
/// leave a machine unusable.
#[tauri::command]
pub fn preview_topology(
    providers: State<'_, Providers>,
    plan: tp_model::TopologySnapshot,
) -> AppResult<tp_model::TopologyPreview> {
    let monitors = providers.display.enumerate()?;
    let current = crate::display::capture_snapshot(&monitors)?;
    let names = friendly_names(&monitors);

    let mut preview = tp_model::preview_topology(&current, &plan, &names);
    // The mode check needs the driver's list, which only the platform layer
    // has, so it is folded in here rather than left to the caller to remember.
    let available: Vec<tp_model::AvailableModes> = monitors
        .iter()
        .map(|m| tp_model::AvailableModes {
            device_path: m.device_path.clone(),
            modes: crate::display::apply::available_modes(&m.gdi_name),
        })
        .collect();
    preview
        .problems
        .extend(tp_model::validate_modes(&plan, &available, &names));
    Ok(preview)
}

/// Apply a plan, then start the countdown.
///
/// The order matters and is not negotiable: capture, validate, apply, read
/// back, and only then ask. If the read-back does not match the plan, it is put
/// back immediately without asking — the answer to "do you want to keep this"
/// is already no when what happened is not what was requested.
#[tauri::command]
pub fn apply_topology(
    app: tauri::AppHandle,
    providers: State<'_, Providers>,
    pending: State<'_, crate::display::confirm::PendingChange>,
    plan: tp_model::TopologySnapshot,
) -> AppResult<Vec<String>> {
    let monitors = providers.display.enumerate()?;
    let names = friendly_names(&monitors);
    let before = crate::display::capture_snapshot(&monitors)?;

    let available: Vec<tp_model::AvailableModes> = monitors
        .iter()
        .map(|m| tp_model::AvailableModes {
            device_path: m.device_path.clone(),
            modes: crate::display::apply::available_modes(&m.gdi_name),
        })
        .collect();

    let mut problems = tp_model::validate_topology(&plan, &names);
    problems.extend(tp_model::validate_modes(&plan, &available, &names));
    if let Some(blocker) = problems
        .iter()
        .find(|p| p.severity == tp_model::ProblemSeverity::Blocking)
    {
        return Err(AppError::Config(blocker.message.clone()));
    }

    // On disk before anything changes, so a crash mid-change still leaves
    // something to put the desktop back with.
    crate::snapshots::save(&before)?;
    crate::snapshots::prune(20);

    let resolver = gdi_resolver(&monitors);
    crate::display::apply::apply(&plan, &resolver)?;

    // Read-back. DISP_CHANGE_SUCCESSFUL means accepted, not achieved.
    let actual = crate::display::capture_snapshot(&providers.display.enumerate()?)?;
    let differences = crate::display::apply::matches(&plan, &actual);
    if !differences.is_empty() {
        tracing::warn!(
            ?differences,
            "the desktop did not match the plan; putting it back"
        );
        crate::display::apply::apply(&before, &resolver)?;
        crate::display::confirm::publish_outcome(
            &app,
            crate::display::confirm::ConfirmOutcome::RevertedOnMismatch,
        );
        return Ok(differences);
    }

    let revert_resolver = owned_gdi_resolver(&monitors);
    crate::display::confirm::start(app, &pending, before, move |snapshot| {
        crate::display::apply::apply(snapshot, &revert_resolver)
    })?;
    Ok(Vec::new())
}

/// Keep the pending change. Ends the countdown.
#[tauri::command]
pub fn keep_topology(pending: State<'_, crate::display::confirm::PendingChange>) -> AppResult<()> {
    crate::display::confirm::keep(&pending)
}

/// Put the desktop back now, without waiting for the countdown.
#[tauri::command]
pub fn revert_topology(
    app: tauri::AppHandle,
    providers: State<'_, Providers>,
    pending: State<'_, crate::display::confirm::PendingChange>,
) -> AppResult<()> {
    let resolver = owned_gdi_resolver(&providers.display.enumerate()?);
    crate::display::confirm::revert_now(
        &app,
        &pending,
        move |snapshot| crate::display::apply::apply(snapshot, &resolver),
        crate::display::confirm::ConfirmOutcome::RevertedOnRequest,
    )
}

/// Whether the panic hotkey is actually registered, and what it is.
///
/// Reported rather than assumed: a hotkey the user believes in and that another
/// program already owns is worse than no hotkey at all.
#[tauri::command]
pub fn panic_hotkey(state: State<'_, crate::display::hotkey::HotkeyState>) -> tp_model::HotkeyInfo {
    tp_model::HotkeyInfo {
        combination: crate::display::hotkey::DESCRIPTION.to_string(),
        registered: state.registered(),
    }
}

/// Snapshots on disk, newest first, so a change nothing is left running to undo
/// can still be undone later.
#[tauri::command]
pub fn list_snapshots() -> Vec<tp_model::TopologySnapshot> {
    crate::snapshots::list()
}

/// Apply a stored snapshot as a plan. Goes through the same countdown.
#[tauri::command]
pub fn restore_snapshot(
    app: tauri::AppHandle,
    providers: State<'_, Providers>,
    pending: State<'_, crate::display::confirm::PendingChange>,
    id: Uuid,
) -> AppResult<Vec<String>> {
    let plan = crate::snapshots::load(id)?;
    apply_topology(app, providers, pending, plan)
}

/// Device path to friendly name, for messages a person can read.
fn friendly_names(monitors: &[MonitorInfo]) -> std::collections::BTreeMap<String, String> {
    monitors
        .iter()
        .map(|m| (m.device_path.clone(), m.friendly_name.clone()))
        .collect()
}

/// CCD device path to GDI name. The two namespaces are separate and only the
/// enumeration knows how they line up.
fn gdi_resolver(monitors: &[MonitorInfo]) -> impl Fn(&str) -> Option<String> + '_ {
    move |path: &str| {
        monitors
            .iter()
            .find(|m| m.device_path == path)
            .map(|m| m.gdi_name.clone())
    }
}

/// The same map, owned, for the closures that outlive this call.
pub fn owned_gdi_resolver(
    monitors: &[MonitorInfo],
) -> impl Fn(&str) -> Option<String> + Send + 'static {
    let map: std::collections::BTreeMap<String, String> = monitors
        .iter()
        .map(|m| (m.device_path.clone(), m.gdi_name.clone()))
        .collect();
    move |path: &str| map.get(path).cloned()
}
