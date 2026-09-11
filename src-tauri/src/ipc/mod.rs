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

/// Give a device your own name, or take it back.
///
/// The name is stored against the best identity the device has — its serial if
/// it reports one, the port it is in if it does not — so it survives a replug
/// and a restart. It is not stored on a profile: what you call a pedal set is
/// a fact about the rig, and having it change between games would be absurd.
///
/// An empty name clears the alias rather than storing an empty string, so the
/// catalog name comes back instead of a blank row.
#[tauri::command]
pub fn set_device_alias(
    key: String,
    name: Option<String>,
    watch: State<'_, crate::peripherals::watch::PeripheralWatch>,
) -> AppResult<tp_model::Preferences> {
    let (mut prefs, _) = crate::settings::load();

    match name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => prefs.device_aliases.insert(key, name.to_string()),
        None => prefs.device_aliases.remove(&key),
    };

    let saved = crate::settings::save(&prefs)?;

    // The list every surface reads comes from the watch thread, so ask it to
    // rescan. The new name then arrives on `peripherals://changed` like any
    // other change, rather than by this command returning a second answer to
    // the same question.
    if let Some(w) = &watch.0 {
        w.poke();
    }

    Ok(saved)
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

pub fn to_wire(s: tp_geometry::RigSolution) -> RigSolutionInfo {
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

/// Stop watching one device.
///
/// Scoped to a device on purpose. A panel that is being replaced issues its
/// stop and the next panel issues its start, and those are two independent
/// messages: an unscoped stop that arrived late would kill the monitor that had
/// just been started, and the new panel would sit there reporting nothing
/// forever with no error to show for it. Matching on the path makes a late stop
/// a no-op instead of a silent failure.
#[tauri::command]
pub fn stop_input_monitor(
    active: State<'_, crate::peripherals::monitor::ActiveMonitor>,
    instance_path: String,
) {
    if let Ok(mut slot) = active.0.lock() {
        match slot.as_ref() {
            Some(m) if m.path == instance_path => *slot = None,
            Some(m) => tracing::debug!(
                stopping = %instance_path,
                active = %m.path,
                "a stale stop for a device that is no longer being monitored"
            ),
            None => {}
        }
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

    // Fill in the adapter, if one handles this game and the profile has not
    // recorded it yet. Matching is exact — see tp_model::adapter::for_game for
    // why a fuzzy match here would write one game's settings into another.
    if profile.game.adapter_id.is_empty() {
        if let Some(adapter) = tp_model::adapter_for_game(&profile.name) {
            profile.game.adapter_id = adapter.id;
            let _ = crate::profiles::save(&profile);
        }
    }

    profile.steps = tp_model::build_steps(&profile);

    // The crash marker, before anything is changed. If the app dies from here
    // on, the next start knows a session was in flight and offers to undo it.
    crate::session::begin(profile.id, &profile.name)?;

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
pub fn save_profile(profile: tp_model::Profile) -> AppResult<tp_model::Profile> {
    crate::profiles::save(&profile)
}

#[tauri::command]
pub fn delete_profile(id: Uuid) -> AppResult<()> {
    crate::profiles::delete(id)
}

/// Add a game the launchers do not know about.
///
/// Steam and Epic are found automatically; everything else is not. Plenty of
/// sims install outside both — iRacing has its own updater, rFactor 2 predates
/// half of this, and a title bought direct has no manifest anywhere. Without
/// this they are simply absent, which is the kind of gap that makes people
/// stop using an app rather than report it.
///
/// The adapter is matched by name, so a hand-added Assetto Corsa still gets its
/// settings written.
#[tauri::command]
pub fn add_game(name: String, exe_path: String) -> AppResult<tp_model::Profile> {
    let exe = std::path::Path::new(&exe_path);
    if !exe.is_file() {
        return Err(AppError::Config(format!(
            "{exe_path} is not a file. Point this at the game's own .exe."
        )));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config("Give it a name.".into()));
    }

    let mut profile = crate::profiles::starter(
        name,
        tp_model::LaunchMethod::Executable {
            path: exe_path.clone(),
            args: Vec::new(),
            working_dir: None,
        },
        exe.parent().map(|p| p.display().to_string()),
        tp_model::Platform::Other,
        None,
    );
    // Knowing the executable up front is what lets the launcher watch for the
    // game to appear and to exit, and what lets the window matcher find it.
    // A hand-added game is the one case where this is free.
    profile.window_plan.target.exe_name =
        exe.file_name().and_then(|n| n.to_str()).map(str::to_string);
    if let Some(adapter) = tp_model::adapter_for_game(name) {
        profile.game.adapter_id = adapter.id;
    }
    crate::profiles::save(&profile)
}

/// Every profile, with what is true of it on this machine right now.
///
/// This is the Games tab. It scans for installed games, makes a profile for any
/// that does not have one, and returns the lot — installed and not.
///
/// **A profile outlives its install.** Uninstalling a game does not delete what
/// you configured for it: the card stays, marked not installed, and everything
/// in it is still there when the game comes back. Losing a tuned profile
/// because a drive was unplugged would be indefensible.
#[tauri::command]
pub fn game_library() -> AppResult<Vec<tp_model::ProfileCard>> {
    let found = crate::launcher::discover();
    let mut profiles = crate::profiles::list();

    for game in &found {
        let already = profiles.iter().any(|p| matches_game(p, game));
        if already {
            // Refresh what can change under a profile: the game can move
            // between drives, and Steam's art cache fills in later.
            if let Some(existing) = profiles.iter_mut().find(|p| matches_game(p, game)) {
                let path = game.install_path.display().to_string();
                let art = crate::art::find(&game.source).map(|p| p.display().to_string());
                if existing.game.install_path.as_deref() != Some(path.as_str())
                    || (existing.game.art_path.is_none() && art.is_some())
                {
                    existing.game.install_path = Some(path);
                    if art.is_some() {
                        existing.game.art_path = art;
                    }
                    let _ = crate::profiles::save(existing);
                }
            }
            continue;
        }

        // No profile yet: make one, so every installed game is ready to
        // configure without a separate "create" step.
        let (launch, platform) = match &game.source {
            crate::launcher::GameSource::Steam { app_id } => (
                tp_model::LaunchMethod::Steam {
                    app_id: app_id.clone(),
                },
                tp_model::Platform::Steam,
            ),
            crate::launcher::GameSource::Epic { app_name } => (
                tp_model::LaunchMethod::Epic {
                    app_name: app_name.clone(),
                },
                tp_model::Platform::Epic,
            ),
        };
        let profile = crate::profiles::starter(
            &game.name,
            launch,
            Some(game.install_path.display().to_string()),
            platform,
            crate::art::find(&game.source).map(|p| p.display().to_string()),
        );
        match crate::profiles::save(&profile) {
            Ok(saved) => profiles.push(saved),
            Err(e) => tracing::warn!(game = %game.name, error = %e, "could not create a profile"),
        }
    }

    Ok(profiles.into_iter().map(|p| card(p, &found)).collect())
}

/// Match a saved profile to a discovered game.
///
/// By launch identity first — a Steam app id is exact and survives a rename or
/// a move to another drive. By name only as a fallback, for profiles made
/// before the id was known.
fn matches_game(profile: &tp_model::Profile, game: &crate::launcher::InstalledGame) -> bool {
    match (&profile.game.launch, &game.source) {
        (
            tp_model::LaunchMethod::Steam { app_id: a },
            crate::launcher::GameSource::Steam { app_id: b },
        ) => a == b,
        (
            tp_model::LaunchMethod::Epic { app_name: a },
            crate::launcher::GameSource::Epic { app_name: b },
        ) => a == b,
        _ => profile.name.eq_ignore_ascii_case(&game.name),
    }
}

/// Use a picture of your own for a game, or go back to the detected one.
///
/// Art is found in Steam's local cache, which means a game from anywhere else
/// has none — Epic caches nothing stable, and a game added by hand was never
/// in a store. Rather than guess at a picture or fetch one over the network,
/// the answer is to let you point at one.
///
/// `None` clears yours and lets the next scan put back whatever it can find.
#[tauri::command]
pub fn set_game_art(
    profile_id: String,
    data_uri: Option<String>,
) -> AppResult<tp_model::ProfileCard> {
    let id = uuid::Uuid::parse_str(&profile_id)
        .map_err(|_| AppError::Config("that is not a profile id".into()))?;
    let mut profile = crate::profiles::load(id)?;

    profile.game.art_path = match data_uri {
        Some(uri) => Some(
            crate::art::save_chosen(&profile_id, &uri)?
                .display()
                .to_string(),
        ),
        None => {
            crate::art::clear_chosen(&profile_id);
            // Back to whatever a scan can find, which for a Steam game is its
            // cached cover and for anything else is nothing at all.
            crate::launcher::discover()
                .iter()
                .find(|g| matches_game(&profile, g))
                .and_then(|g| crate::art::find(&g.source))
                .map(|p| p.display().to_string())
        }
    };

    crate::profiles::save(&profile)?;
    Ok(card(profile, &crate::launcher::discover()))
}

fn card(
    profile: tp_model::Profile,
    found: &[crate::launcher::InstalledGame],
) -> tp_model::ProfileCard {
    let installed = found.iter().any(|g| matches_game(&profile, g));
    let art = profile
        .game
        .art_path
        .as_deref()
        .map(std::path::Path::new)
        .filter(|p| p.is_file())
        .and_then(crate::art::as_data_uri);

    tp_model::ProfileCard {
        installed,
        art,
        // Only claim a folder that is actually there. A path shown for a game
        // that has been uninstalled reads as "it is still here", which is the
        // one thing the card must not say.
        install_path: profile
            .game
            .install_path
            .clone()
            .filter(|p| std::path::Path::new(p).is_dir()),
        platform: profile.game.platform.label().to_string(),
        profile,
    }
}

/// Remember a window rectangle on a profile, so it can be re-applied.
///
/// This is the half of "set it once" that makes the toggle meaningful: the
/// geometry saved here is a rectangle that has been *seen working*, read back
/// off a real window, rather than one computed and hoped for.
#[tauri::command]
pub fn remember_window(
    id: Uuid,
    rect: tp_model::PixelRect,
    means: tp_model::RectMeans,
    exe_name: Option<String>,
) -> AppResult<tp_model::Profile> {
    let mut profile = crate::profiles::load(id)?;
    profile.window_plan.rect = tp_model::RectSource::Explicit { rect };
    profile.window_plan.rect_means = means;
    if exe_name.is_some() {
        profile.window_plan.target.exe_name = exe_name;
    }
    crate::profiles::save(&profile)
}

/// Copy a running game's screen setup onto its profile.
///
/// The point of this is that a great many rigs are *already* set up — with
/// SRWE, Resize Raccoon, or by hand — and that setup took real effort. Asking
/// somebody to describe a window they can already see, in numbers, is asking
/// them to do the work twice.
///
/// So this reads the window: its rectangle, whether it is frameless, whether it
/// is always on top, and — the part that matters most — **the executable it
/// belongs to**. A Steam or Epic profile names no executable, because a
/// protocol launch hands off to the launcher and the game arrives as a
/// grandchild. Learning it here is what lets every future launch find that
/// window at all.
///
/// Automatic placement is switched on by the same call, because the rectangle
/// being saved is one that is on screen and working at this moment. That is a
/// stronger warrant than any rectangle the app could compute.
#[tauri::command]
pub fn capture_window(
    providers: State<'_, Providers>,
    id: Uuid,
    hwnd: Option<String>,
) -> AppResult<tp_model::CaptureResult> {
    let mut profile = crate::profiles::load(id)?;
    let candidates = crate::window::enumerate_windows();
    let min = profile.window_plan.target.min_size;

    // An explicit choice from the picker wins over any matching.
    let chosen = match &hwnd {
        Some(handle) => {
            let handle: u64 = handle
                .parse()
                .map_err(|_| AppError::Config(format!("{handle:?} is not a window handle")))?;
            match candidates.iter().find(|c| c.hwnd == handle) {
                Some(found) => found.clone(),
                None => {
                    return Err(AppError::Config(
                        "that window has closed since the list was drawn".into(),
                    ))
                }
            }
        }
        None => {
            let hint = profile
                .window_plan
                .target
                .exe_name
                .clone()
                .or_else(|| tp_model::expected_exe(&profile));

            match tp_model::pick_capture(&candidates, hint.as_deref(), min) {
                tp_model::CaptureChoice::One(found) => found.clone(),
                tp_model::CaptureChoice::Several(list) => {
                    return Ok(tp_model::CaptureResult::Choose {
                        windows: list
                            .into_iter()
                            .map(|c| tp_model::OpenWindow {
                                plausible: true,
                                candidate: c.clone(),
                            })
                            .collect(),
                    })
                }
                tp_model::CaptureChoice::NotRunning(exe) => {
                    return Ok(tp_model::CaptureResult::NotRunning { exe })
                }
                tp_model::CaptureChoice::Nothing => {
                    return Ok(tp_model::CaptureResult::NothingFound)
                }
            }
        }
    };

    let screens: Vec<(String, tp_model::PixelRect)> = providers
        .display
        .enumerate()?
        .into_iter()
        .map(|m| (m.friendly_name, m.bounds))
        .collect();
    let layout = tp_model::capture(&chosen, &screens);

    profile.window_plan.rect = tp_model::RectSource::Explicit { rect: layout.rect };
    // The captured rectangle is the *outer* window, because that is what an
    // enumeration reports and what SRWE moved. Recording it as the client area
    // would be wrong by exactly the frame width.
    profile.window_plan.rect_means = tp_model::RectMeans::OuterWindow;
    profile.window_plan.borderless = layout.borderless;
    profile.window_plan.always_on_top = layout.always_on_top;
    if layout.exe_name.is_some() {
        profile.window_plan.target.exe_name = layout.exe_name.clone();
    }
    if !layout.class_name.is_empty() {
        profile.window_plan.target.window_class = Some(layout.class_name.clone());
    }
    // On, because this rectangle is working on screen right now. Nothing the
    // app could compute has that warrant.
    profile.window_plan.auto_apply = true;

    let profile = crate::profiles::save(&profile)?;
    tracing::info!(
        game = %profile.name,
        rect = ?layout.rect,
        exe = ?layout.exe_name,
        "copied a running game's screen setup"
    );
    Ok(tp_model::CaptureResult::Captured {
        layout,
        profile: Box::new(profile),
    })
}

/// Turn automatic placement on or off for a profile.
///
/// Kept as its own command rather than a whole-profile save, so the toggle
/// cannot carry along whatever else the editor happened to have in memory.
#[tauri::command]
pub fn set_auto_apply(id: Uuid, enabled: bool) -> AppResult<tp_model::Profile> {
    let mut profile = crate::profiles::load(id)?;
    if enabled && matches!(profile.window_plan.rect, tp_model::RectSource::FromGeometry) {
        return Err(AppError::Config(
            "Place the window once first. Automatic placement re-applies a \
             rectangle you have already seen work — there is nothing saved to \
             re-apply yet."
                .into(),
        ));
    }
    profile.window_plan.auto_apply = enabled;
    crate::profiles::save(&profile)
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

// ------------------------------------------------------------------ adapters

/// The adapters that exist, and how well each one's key names are known.
///
/// Confidence is part of the wire type on purpose: "read out of the shipped
/// file" and "corroborated across forum posts" are different claims and the UI
/// must not make them look alike.
#[tauri::command]
pub fn list_adapters() -> Vec<tp_model::AdapterInfo> {
    tp_model::adapter_catalog()
}

/// What an adapter would write for a rig, and what is already correct.
///
/// Reads only. The preview half of preview-before-apply, produced by the same
/// code that performs the write so the two cannot drift.
#[tauri::command]
pub fn preview_adapter(
    adapter_id: String,
    rig: tp_model::RigModel,
    session: tp_model::SessionMode,
) -> AppResult<tp_model::AdapterPreview> {
    let solution = solve_rig(rig.clone(), session);
    let plan = tp_model::adapter_plan(&adapter_id, &rig, &solution, session)
        .ok_or_else(|| AppError::Config(format!("there is no {adapter_id} adapter")))?;
    Ok(tp_model::AdapterPreview {
        files: crate::adapters::preview(&plan),
        warnings: plan.warnings,
    })
}

/// What a title's config files actually contain.
///
/// For the games the catalog recognises but nobody has confirmed the layout
/// of. Names only, never values: what is needed to write an adapter is what the
/// settings are *called*, and a listing with no numbers in it is one a user can
/// read and send without having to judge what is in it.
#[tauri::command]
pub fn inspect_adapter(adapter_id: String) -> Vec<tp_model::ConfigInspection> {
    crate::adapters::inspect(&adapter_id)
}

/// Back up, then write.
///
/// Returns the backup id, so the UI can offer to undo exactly this change
/// rather than the whole history.
#[tauri::command]
pub fn apply_adapter(
    adapter_id: String,
    rig: tp_model::RigModel,
    session: tp_model::SessionMode,
) -> AppResult<tp_model::AppliedAdapterInfo> {
    let solution = solve_rig(rig.clone(), session);
    let plan = tp_model::adapter_plan(&adapter_id, &rig, &solution, session)
        .ok_or_else(|| AppError::Config(format!("there is no {adapter_id} adapter")))?;
    let applied = crate::adapters::apply(&plan)?;
    Ok(tp_model::AppliedAdapterInfo {
        backup: applied.backup,
        written: applied.written,
    })
}

/// Every backup, newest first. The one-click "put it back" list.
#[tauri::command]
pub fn list_backups() -> Vec<tp_model::BackupInfo> {
    crate::backup::list()
        .into_iter()
        .map(|m| tp_model::BackupInfo {
            id: m.taken_at.clone(),
            taken_at: m.taken_at,
            reason: m.reason,
            files: m.files.into_iter().map(|f| f.original_path).collect(),
        })
        .collect()
}

/// Put one operation's files back exactly as they were.
#[tauri::command]
pub fn restore_backup(id: String) -> AppResult<Vec<String>> {
    crate::backup::restore(&id)
}

// --------------------------------------------------------------- diagnostics

/// Build a diagnostics bundle and return where it landed.
///
/// One file containing what is needed to work out what happened on a machine
/// nobody debugging it can see. It carries a README listing everything in it
/// and what was redacted, because asking somebody to email a black box about
/// their own machine is not reasonable — and a bundle people are afraid of is
/// one nobody sends.
#[tauri::command]
pub fn create_diagnostics(providers: State<'_, Providers>) -> AppResult<String> {
    Ok(crate::diagnostics::build(&providers)?.display().to_string())
}

/// Show a file in Explorer, selected.
///
/// The bundle is only useful once the user can find it, and reading a path off
/// a screen and typing it into Explorer is a step too many at the exact moment
/// somebody is already annoyed.
#[tauri::command]
pub fn reveal_file(app: tauri::AppHandle, path: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let target = std::path::PathBuf::from(&path);
    // Only ever inside the app's own data directory. This takes a path from the
    // frontend, and a shell-open of an arbitrary string is a way to run
    // anything at all.
    if !target.starts_with(crate::logging::app_data_dir()) {
        return Err(AppError::Config(
            "that file is not one of Team Principal's own".into(),
        ));
    }

    app.opener()
        .reveal_item_in_dir(&target)
        .map_err(|e| AppError::Config(format!("could not open the folder: {e}")))
}

// ----------------------------------------------------------------- licensing

/// The current entitlement, and the whole of what the UI is told.
#[tauri::command]
pub fn licence_state() -> tp_model::LicenceState {
    crate::licence::provider().state()
}

// ------------------------------------------------------------------ recovery

/// The session that did not finish, if there is one.
///
/// A session changes things outside this app. If it is killed mid-flight —
/// crash, power cut, Task Manager — nothing is left running to put them back,
/// so a marker on disk is what lets a later start notice and offer.
#[tauri::command]
pub fn pending_session() -> Option<tp_model::PendingSession> {
    crate::session::pending().map(|s| tp_model::PendingSession {
        profile_name: s.profile_name,
        started_at: s.started_at,
        has_config_backup: s.config_backup.is_some(),
    })
}

/// Undo what the unfinished session changed.
#[tauri::command]
pub fn recover_session() -> AppResult<Vec<String>> {
    let Some(marker) = crate::session::pending() else {
        return Err(AppError::Config("nothing is outstanding".into()));
    };

    let mut done = Vec::new();
    if let Some(backup) = &marker.config_backup {
        match crate::backup::restore(backup) {
            Ok(files) => done.push(format!(
                "put back {} game config {}",
                files.len(),
                if files.len() == 1 { "file" } else { "files" }
            )),
            // Reported rather than aborting: the marker still has to be
            // cleared, or the app offers the same failing recovery forever.
            Err(e) => done.push(format!("could not restore the config files: {e}")),
        }
    }

    crate::session::clear();
    Ok(done)
}

/// Leave the unfinished session alone and stop asking.
#[tauri::command]
pub fn dismiss_pending_session() {
    crate::session::clear();
}

// ------------------------------------------------------------------- startup

/// The frontend has something to show. Close the splash and reveal the app.
///
/// Called once, from the first render that has real data behind it. The main
/// window starts hidden precisely so this can decide *when* it appears —
/// showing it earlier means a white rectangle the size of the window, which is
/// the thing a splash screen exists to prevent.
///
/// Minimised when the machine started the app rather than the user, or when
/// they have asked for it always. Minimised rather than hidden: there is no
/// tray icon, and an app with no way back is not a feature.
#[tauri::command]
pub fn ready(app: tauri::AppHandle, startup: State<'_, crate::Startup>) {
    use tauri::Manager;

    let minimised = startup.minimised || crate::settings::load().0.start_minimised;

    if let Some(main) = app.get_webview_window("main") {
        if minimised {
            let _ = main.minimize();
        }
        // Shown either way. A minimised window still has to exist on the
        // taskbar, or there is no way back to it.
        let _ = main.show();
        if !minimised {
            let _ = main.set_focus();
        }
    }

    // Last, so the splash never disappears before the app is up — a gap of
    // empty desktop reads as a crash.
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }
}

/// Whether the app is registered to start with Windows, read from the registry.
///
/// Read back rather than remembered: Windows disables startup entries through
/// Task Manager and through its own heuristics without telling the app, and a
/// switch showing On over an entry Windows turned off is a lie the user finds
/// out about on the morning it matters.
#[tauri::command]
pub fn startup_state() -> tp_model::StartupState {
    tp_model::StartupState {
        enabled: crate::startup::is_enabled(),
        stale: crate::startup::is_stale(),
    }
}

/// Register or unregister the app to start with Windows.
#[tauri::command]
pub fn set_run_at_startup(enabled: bool) -> AppResult<tp_model::StartupState> {
    crate::startup::set(enabled)?;
    Ok(startup_state())
}

// ------------------------------------------------------------------- updates

/// Ask whether a newer version exists. Downloads nothing.
///
/// A build with no signing key configured reports that, rather than checking
/// nothing and saying "up to date" — which would be a claim nothing made.
#[tauri::command]
pub async fn check_for_update(app: tauri::AppHandle) -> AppResult<tp_model::UpdateInfo> {
    crate::updates::check(&app).await
}

/// Download, verify, install and restart.
///
/// The signature is checked against the public key compiled into this binary
/// before a byte of the new version runs. Refused outright while a session is
/// in flight.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> AppResult<()> {
    crate::updates::install(&app).await
}
