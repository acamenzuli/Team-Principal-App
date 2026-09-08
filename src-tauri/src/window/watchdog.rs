//! Keeping the window where it was put.
//!
//! Sims reset their own window: when the render device initialises, when a
//! session loads, on alt-tab, and sometimes on a timer for no visible reason.
//! A launcher that applies geometry once and reports success has done half a
//! job — the window is correct for about four seconds.
//!
//! The policy is two-part, because the two causes are different. A burst of
//! re-applies immediately after launch handles the render device settling.
//! Ongoing drift correction handles a title that fights back, and stops once
//! the window has held still long enough to call it settled.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tp_model::{PixelRect, RectMeans, WatchdogPolicy};

/// Stops the watchdog when dropped.
pub struct WatchdogHandle {
    stop: Arc<AtomicBool>,
}

impl WatchdogHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for WatchdogHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Managed state: one watched window at a time. Two watchdogs fighting over
/// one window would be worse than none.
#[derive(Default)]
pub struct ActiveWatchdog(pub std::sync::Mutex<Option<WatchdogHandle>>);

pub struct Watchdog;

impl Watchdog {
    /// Start watching a window. Returns immediately.
    pub fn start(
        hwnd: u64,
        rect: PixelRect,
        means: RectMeans,
        borderless: bool,
        policy: WatchdogPolicy,
    ) -> WatchdogHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        std::thread::Builder::new()
            .name("window-watchdog".into())
            .spawn(move || run(hwnd, rect, means, borderless, policy, &thread_stop))
            .expect("could not start the window watchdog");

        WatchdogHandle { stop }
    }
}

fn run(
    hwnd: u64,
    rect: PixelRect,
    means: RectMeans,
    borderless: bool,
    policy: WatchdogPolicy,
    stop: &AtomicBool,
) {
    let started = Instant::now();
    let mut applied = 0u32;
    let mut settled_since: Option<Instant> = None;

    // The opening burst: re-apply on a fixed cadence while the render device
    // is still deciding what it wants the window to be.
    let burst_interval = if policy.reapply_count > 0 {
        Duration::from_millis(policy.reapply_window_ms / policy.reapply_count.max(1) as u64)
    } else {
        Duration::from_millis(500)
    };

    while !stop.load(Ordering::Relaxed) {
        let in_burst = applied < policy.reapply_count
            && started.elapsed().as_millis() < policy.reapply_window_ms as u128;

        let drift_check = policy.drift_check_interval_ms.map(Duration::from_millis);
        if !in_burst && drift_check.is_none() {
            break; // Nothing left to do.
        }

        // Read first. Re-applying geometry that is already correct would fight
        // a game that is merely repainting, and would hide real drift in a
        // stream of no-op writes.
        let current = match super::read_geometry(hwnd) {
            Ok(g) => g,
            Err(_) => {
                // The window is gone: the game closed, which is the normal way
                // this ends.
                tracing::info!(hwnd, "watched window disappeared; stopping");
                break;
            }
        };

        let actual = match means {
            RectMeans::ClientArea => current.client,
            RectMeans::OuterWindow => current.outer,
        };
        let drifted = tp_model::has_drifted(actual, rect, 2)
            || (borderless && !tp_model::is_borderless(current.style, current.ex_style));

        if drifted || in_burst {
            match super::apply_geometry(hwnd, rect, means, borderless, false) {
                Ok(_) => {
                    if drifted {
                        tracing::info!(hwnd, "the game moved its own window; put it back");
                    }
                    applied += 1;
                    settled_since = None;
                }
                Err(e) => {
                    tracing::warn!(hwnd, error = %e, "could not re-apply window geometry");
                    // A refusal will not start succeeding on the next tick.
                    // Stop rather than logging the same failure forever.
                    break;
                }
            }
        } else {
            // Stable. Once it has held still for long enough, stop watching —
            // a thread polling forever for a window nobody is fighting over is
            // just background work with no purpose.
            let since = settled_since.get_or_insert_with(Instant::now);
            if let Some(stop_after) = policy.stop_after_stable_ms {
                if since.elapsed().as_millis() >= stop_after as u128 {
                    tracing::info!(hwnd, "window settled; watchdog stopping");
                    break;
                }
            }
        }

        let wait = if in_burst {
            burst_interval
        } else {
            drift_check.unwrap_or(Duration::from_millis(2000))
        };
        std::thread::sleep(wait);
    }
}
