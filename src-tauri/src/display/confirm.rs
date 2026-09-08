//! Confirm-or-revert.
//!
//! Windows' own display applet does this and everyone understands it: change
//! the screen, count down, and put it back unless the user says to keep it.
//! Team Principal does the same for one reason — **if the change made the
//! screen unreadable, the user cannot click Keep.** Silence has to mean revert.
//!
//! That single sentence decides the design:
//!
//! * **The countdown runs in Rust, on its own thread.** A timer in the frontend
//!   is a timer on a screen that may have just gone black, and a WebView that
//!   is being resized and re-composited is exactly the thing that stops
//!   painting. The UI shows the count; it does not own it.
//! * **Reverting does not need the UI at all.** The snapshot and the thread are
//!   already in memory. The revert would still happen if the WebView had
//!   crashed outright.
//! * **The revert path is the apply path.** Putting the desktop back is
//!   applying the captured snapshot, so the code that rescues the user is the
//!   same code that got exercised on the way in.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
pub use tp_model::ConfirmOutcome;
use tp_model::{ConfirmState, TopologySnapshot};

use crate::error::{AppError, AppResult};

/// Emitted every second while a change is pending, and once when it settles.
pub const EVENT: &str = "display://confirm";

/// How long the user has. Windows uses fifteen seconds and it is the right
/// number: long enough to find the mouse on a rearranged desktop, short enough
/// that a black screen is not frightening.
const COUNTDOWN: Duration = Duration::from_secs(15);

/// A change waiting to be confirmed.
pub struct Pending {
    before: TopologySnapshot,
    settled: Arc<AtomicBool>,
    keep: Arc<AtomicBool>,
}

/// Managed state: one pending change at a time.
#[derive(Default)]
pub struct PendingChange(pub Mutex<Option<Pending>>);

impl PendingChange {
    /// The snapshot to go back to, for the panic hotkey.
    pub fn snapshot(&self) -> Option<TopologySnapshot> {
        self.0
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|p| p.before.clone()))
    }
}

/// Start the countdown after a change has been applied.
///
/// `revert` is called with the captured snapshot if the user does not confirm.
/// Taking it as a closure keeps this module free of Win32 and, more usefully,
/// makes the countdown itself testable.
pub fn start(
    app: AppHandle,
    pending: &PendingChange,
    before: TopologySnapshot,
    revert: impl Fn(&TopologySnapshot) -> AppResult<()> + Send + 'static,
) -> AppResult<()> {
    let settled = Arc::new(AtomicBool::new(false));
    let keep = Arc::new(AtomicBool::new(false));

    {
        let mut slot = pending
            .0
            .lock()
            .map_err(|_| AppError::Config("the display state is unreadable".into()))?;
        // A second change while one is pending would leave two snapshots and no
        // way to know which is the real "before".
        if slot
            .as_ref()
            .is_some_and(|p| !p.settled.load(Ordering::Relaxed))
        {
            return Err(AppError::Config(
                "a display change is already waiting to be confirmed".into(),
            ));
        }
        *slot = Some(Pending {
            before: before.clone(),
            settled: settled.clone(),
            keep: keep.clone(),
        });
    }

    std::thread::Builder::new()
        .name("display-confirm".into())
        .spawn(move || countdown(app, before, settled, keep, revert))
        .map_err(|e| AppError::Config(format!("could not start the countdown: {e}")))?;

    Ok(())
}

fn countdown(
    app: AppHandle,
    before: TopologySnapshot,
    settled: Arc<AtomicBool>,
    keep: Arc<AtomicBool>,
    revert: impl Fn(&TopologySnapshot) -> AppResult<()>,
) {
    let deadline = Instant::now() + COUNTDOWN;

    while Instant::now() < deadline {
        if keep.load(Ordering::Relaxed) {
            settled.store(true, Ordering::Relaxed);
            tracing::info!("display change kept");
            publish(&app, 0, Some(ConfirmOutcome::Kept));
            return;
        }
        if settled.load(Ordering::Relaxed) {
            // Someone else — the panic hotkey — already put it back.
            return;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        publish(&app, left.as_secs() as u32 + 1, None);

        // A short tick so a Keep press is honoured immediately rather than up
        // to a second later.
        std::thread::sleep(Duration::from_millis(100));
    }

    settled.store(true, Ordering::Relaxed);
    tracing::warn!("display change not confirmed; putting it back");
    let outcome = match revert(&before) {
        Ok(()) => ConfirmOutcome::RevertedOnTimeout,
        Err(e) => {
            tracing::error!(error = %e, "could not revert the display change");
            ConfirmOutcome::RevertFailed
        }
    };
    publish(&app, 0, Some(outcome));
}

/// Keep the change. Ends the countdown without reverting.
pub fn keep(pending: &PendingChange) -> AppResult<()> {
    let slot = pending
        .0
        .lock()
        .map_err(|_| AppError::Config("the display state is unreadable".into()))?;
    let p = slot
        .as_ref()
        .ok_or_else(|| AppError::Config("no display change is waiting".into()))?;
    if p.settled.load(Ordering::Relaxed) {
        return Err(AppError::Config(
            "that change has already settled one way or the other".into(),
        ));
    }
    p.keep.store(true, Ordering::Relaxed);
    Ok(())
}

/// Put it back now, without waiting for the countdown.
///
/// Used by the Undo button and by the panic hotkey. Marking it settled *before*
/// reverting stops the countdown thread doing the same work a moment later.
pub fn revert_now(
    app: &AppHandle,
    pending: &PendingChange,
    revert: impl Fn(&TopologySnapshot) -> AppResult<()>,
    reason: ConfirmOutcome,
) -> AppResult<()> {
    let before = {
        let slot = pending
            .0
            .lock()
            .map_err(|_| AppError::Config("the display state is unreadable".into()))?;
        let p = slot
            .as_ref()
            .ok_or_else(|| AppError::Config("no display change is waiting".into()))?;
        if p.settled.swap(true, Ordering::Relaxed) {
            return Err(AppError::Config(
                "that change has already settled one way or the other".into(),
            ));
        }
        p.before.clone()
    };

    let result = revert(&before);
    let outcome = match &result {
        Ok(()) => reason,
        Err(e) => {
            tracing::error!(error = %e, "could not revert the display change");
            ConfirmOutcome::RevertFailed
        }
    };
    publish(app, 0, Some(outcome));
    result
}

/// Announce an outcome that settled without a countdown ever starting — a
/// read-back mismatch that was put straight back.
pub fn publish_outcome(app: &AppHandle, outcome: ConfirmOutcome) {
    publish(app, 0, Some(outcome));
}

fn publish(app: &AppHandle, seconds_left: u32, outcome: Option<ConfirmOutcome>) {
    let state = ConfirmState {
        seconds_left,
        outcome,
    };
    if let Err(e) = app.emit(EVENT, &state) {
        tracing::debug!(error = %e, "could not publish the confirm state");
    }
}
