//! Team Principal — sim racing launcher and display manager.
//!
//! Milestone 1: skeleton. Tauri v2 + React, DPI manifest, structured logging,
//! typed IPC, provider seams with fixture-driven mocks, CI.
//!
//! Architecture note: the frontend never touches Win32. It calls the commands
//! in [`ipc`], which go through the traits in [`providers`], which have exactly
//! two implementations — the real Win32 one and a mock driven by JSON fixtures.

pub mod adapters;
pub mod art;
pub mod backup;
pub mod diagnostics;
pub mod display;
pub mod error;
pub mod ipc;
pub mod launcher;
pub mod licence;
pub mod logging;
pub mod peripherals;
pub mod profiles;
pub mod providers;
pub mod rig;
pub mod session;
pub mod settings;
pub mod snapshots;
pub mod startup;
pub mod window;

/// Which milestone this build represents. Shown in the UI and in diagnostics so
/// a bug report says what was actually built, not what was planned.
pub const MILESTONE: u8 = 10;

#[cfg(windows)]
pub use providers::win::dpi::Awareness;

#[cfg(not(windows))]
pub use non_windows_dpi::Awareness;

#[cfg(not(windows))]
mod non_windows_dpi {
    /// Mirror of the Windows type so the rest of the app compiles anywhere.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Awareness {
        Unknown,
    }
    impl Awareness {
        pub fn is_acceptable(self) -> bool {
            false
        }
    }
}

pub fn dpi_awareness() -> Awareness {
    #[cfg(windows)]
    {
        providers::win::dpi::current()
    }
    #[cfg(not(windows))]
    {
        Awareness::Unknown
    }
}

/// ISO-8601 UTC, the timestamp format used everywhere in the model.
pub fn now_iso8601() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

/// How this instance was started. Read by `ready`, which decides whether the
/// main window is shown or comes up minimised.
pub struct Startup {
    pub minimised: bool,
}

/// Command-line options. Deliberately tiny.
#[derive(Debug, Clone, Default)]
pub struct Cli {
    /// `--mock <path>`: run against a fixture instead of real hardware.
    pub mock_fixture: Option<std::path::PathBuf>,
    /// `--simulate`: run a profile against mocks and print the plan, no UI.
    pub simulate: bool,
    /// `--check-dpi`: print this process's DPI awareness and exit. Exists so an
    /// integration test can interrogate the *real* executable, which is the
    /// only binary the manifest is embedded into.
    pub check_dpi: bool,
    /// `--minimised`: come up out of the way. What the Windows startup entry
    /// passes, so a boot start does not take the screen.
    pub minimised: bool,
}

impl Cli {
    pub fn from_env() -> Cli {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut cli = Cli::default();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--mock" => {
                    cli.mock_fixture = args.get(i + 1).map(std::path::PathBuf::from);
                    i += 1;
                }
                "--simulate" => cli.simulate = true,
                "--check-dpi" => cli.check_dpi = true,
                // Both spellings: the registry entry writes one and people
                // typing it by hand will use the other.
                "--minimised" | "--minimized" => cli.minimised = true,
                _ => {}
            }
            i += 1;
        }
        cli
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cli = Cli::from_env();
    let start_minimised = cli.minimised;

    // Answered before anything else starts, so the check costs nothing and
    // cannot be perturbed by logging or provider setup.
    if cli.check_dpi {
        let awareness = dpi_awareness();
        println!("{awareness:?}");
        std::process::exit(if awareness.is_acceptable() { 0 } else { 1 });
    }

    let _log_guard = logging::init(logging::app_data_dir().join("logs"));

    // Checked before anything reads a monitor rectangle. See the module docs
    // for why a wrong answer here poisons every geometry value in the app.
    #[cfg(windows)]
    providers::win::dpi::verify_and_log();

    let providers = match providers::Providers::select(cli.mock_fixture.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "could not initialise providers");
            eprintln!("Team Principal could not start: {e}");
            std::process::exit(1);
        }
    };

    let simulated = providers.simulated;

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(providers)
        .setup(move |app| {
            use tauri::Manager;
            // The peripheral watch owns the device list and publishes changes.
            // Started here rather than lazily so the first render already has a
            // scan behind it. Against fixtures there is nothing to watch, and
            // the mock provider answers instead.
            let watcher = (!simulated).then(|| peripherals::watch::start(app.handle().clone()));
            app.manage(peripherals::watch::PeripheralWatch(watcher));
            app.manage(peripherals::monitor::ActiveMonitor::default());
            app.manage(window::watchdog::ActiveWatchdog::default());
            app.manage(launcher::run::ActiveRun::default());
            app.manage(display::confirm::PendingChange::default());

            // The panic hotkey owns its own thread and message loop. Registered
            // last so everything it might need already exists, and kept alive
            // for the life of the process.
            let hotkey = display::hotkey::start(app.handle().clone(), |app| {
                use tauri::Manager;
                let pending = app.state::<display::confirm::PendingChange>();
                let providers = app.state::<providers::Providers>();
                let Ok(monitors) = providers.display.enumerate() else {
                    tracing::error!("panic hotkey pressed but the displays could not be read");
                    return;
                };
                let resolver = ipc::owned_gdi_resolver(&monitors);
                if let Err(e) = display::confirm::revert_now(
                    app,
                    &pending,
                    move |snapshot| display::apply::apply(snapshot, &resolver),
                    display::confirm::ConfirmOutcome::RevertedOnRequest,
                ) {
                    tracing::warn!(error = %e, "panic hotkey pressed with nothing to undo");
                }
            });
            app.manage(hotkey);
            app.manage(Startup {
                minimised: start_minimised,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::app_info,
            ipc::ready,
            ipc::startup_state,
            ipc::set_run_at_startup,
            ipc::list_monitors,
            ipc::list_devices,
            ipc::refresh_devices,
            ipc::start_input_monitor,
            ipc::stop_input_monitor,
            ipc::start_preflight,
            ipc::cancel_preflight,
            ipc::pending_session,
            ipc::recover_session,
            ipc::dismiss_pending_session,
            ipc::retry_step,
            ipc::skip_step,
            ipc::launch_game,
            ipc::save_profile,
            ipc::delete_profile,
            ipc::add_game,
            ipc::game_library,
            ipc::remember_window,
            ipc::capture_window,
            ipc::set_auto_apply,
            ipc::list_windows,
            ipc::place_window,
            ipc::stop_watching_window,
            ipc::desktop_layout,
            ipc::available_modes,
            ipc::current_topology,
            ipc::preview_topology,
            ipc::apply_topology,
            ipc::keep_topology,
            ipc::revert_topology,
            ipc::panic_hotkey,
            ipc::list_snapshots,
            ipc::restore_snapshot,
            ipc::list_adapters,
            ipc::preview_adapter,
            ipc::inspect_adapter,
            ipc::apply_adapter,
            ipc::list_backups,
            ipc::restore_backup,
            ipc::get_preferences,
            ipc::save_preferences,
            ipc::accent_presets,
            ipc::create_diagnostics,
            ipc::reveal_file,
            ipc::licence_state,
            ipc::list_rigs,
            ipc::current_rig,
            ipc::save_rig,
            ipc::delete_rig,
            ipc::detect_rig,
            ipc::solve_rig,
            ipc::fit_rig,
            ipc::parse_length,
            ipc::solve_curvature,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Team Principal");
}
