//! Team Principal — sim racing launcher and display manager.
//!
//! Milestone 1: skeleton. Tauri v2 + React, DPI manifest, structured logging,
//! typed IPC, provider seams with fixture-driven mocks, CI.
//!
//! Architecture note: the frontend never touches Win32. It calls the commands
//! in [`ipc`], which go through the traits in [`providers`], which have exactly
//! two implementations — the real Win32 one and a mock driven by JSON fixtures.

pub mod display;
pub mod error;
pub mod ipc;
pub mod logging;
pub mod peripherals;
pub mod providers;
pub mod rig;
pub mod settings;

/// Which milestone this build represents. Shown in the UI and in diagnostics so
/// a bug report says what was actually built, not what was planned.
pub const MILESTONE: u8 = 1;

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::app_info,
            ipc::list_monitors,
            ipc::list_devices,
            ipc::refresh_devices,
            ipc::start_input_monitor,
            ipc::stop_input_monitor,
            ipc::desktop_layout,
            ipc::get_preferences,
            ipc::save_preferences,
            ipc::accent_presets,
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
