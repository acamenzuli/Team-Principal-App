//! Structured logging to a rotating file, plus the diagnostics bundle seam.
//!
//! Logs go to `%APPDATA%\Team Principal\logs\`. They are structured (JSON in
//! the file, human-readable on stderr) because the questions this app has to
//! answer after the fact — "my pedals dropped mid-race", "the window moved
//! itself" — are queries over timestamped events, not prose.

use std::path::PathBuf;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Keep the guard alive for the process lifetime or the last lines are lost.
pub struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

pub fn init(log_dir: PathBuf) -> LogGuard {
    std::fs::create_dir_all(&log_dir).ok();

    let file = tracing_appender::rolling::daily(&log_dir, "team-principal.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file);

    let filter = EnvFilter::try_from_env("TP_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,team_principal_lib=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            // Machine-readable, for the diagnostics bundle.
            tracing_subscriber::fmt::layer()
                .json()
                .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
                .with_writer(file_writer),
        )
        .with(
            // Human-readable, for development.
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr),
        )
        .init();

    tracing::info!(dir = %log_dir.display(), "logging started");
    LogGuard(guard)
}

/// `%APPDATA%\Team Principal` on Windows; a sensible local equivalent
/// elsewhere so the app can be developed on any machine.
pub fn app_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Team Principal")
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("team-principal")
    }
}
