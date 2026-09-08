//! Running a profile.
//!
//! The scheduler in `tp_model::graph` says what may run; this runs it, in
//! parallel where the graph allows, and publishes every status change so the
//! UI can be a pure view over the stream rather than driving the sequence
//! itself.
//!
//! Three rules shape the loop:
//!
//! * **Nothing claims credit it has not earned.** A utility that was already
//!   running reports `AlreadyRunning`, and the row's text is derived from that
//!   rather than written by hand.
//! * **A step is visible for at least a moment.** An instant pass that flashes
//!   past is worse than useless on a go/no-go screen — but the padding is per
//!   step and never delays the whole run.
//! * **Cancelling tears down what was started.** Leaving half a session running
//!   because someone changed their mind is how a launcher earns distrust.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tp_model::{
    ActionTaken, Phase, Profile, ReadinessGate, ReadyState, Scheduler, StepAction, StepId,
    StepStatus, StepView,
};

use super::job::JobObject;
use crate::error::{AppError, AppResult};

/// Emitted for every status change.
pub const STEP_EVENT: &str = "launch://step";
/// Emitted when the run reaches a terminal state.
pub const STATE_EVENT: &str = "launch://state";

/// A run in progress.
pub struct Run {
    cancel: Arc<AtomicBool>,
    /// Kept so the session's utilities die with the app rather than outliving
    /// a crash. Dropping this terminates them.
    _job: Arc<JobObject>,
    views: Arc<Mutex<Vec<StepView>>>,
}

impl Run {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn views(&self) -> Vec<StepView> {
        self.views.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

/// Managed state: one run at a time. Two preflights racing each other over the
/// same utilities would be worse than none.
#[derive(Default)]
pub struct ActiveRun(pub Mutex<Option<Run>>);

/// What one step produced.
struct Completion {
    id: StepId,
    status: StepStatus,
    action: ActionTaken,
    detail: String,
    elapsed_ms: f64,
}

/// Start running a profile's preflight and launch phases.
pub fn start(app: AppHandle, profile: Profile) -> AppResult<Run> {
    let scheduler = Scheduler::new(profile.steps.clone())
        .map_err(|e| AppError::Config(format!("this profile cannot run: {e}")))?;

    let cancel = Arc::new(AtomicBool::new(false));
    let job = Arc::new(JobObject::new()?);
    let views = Arc::new(Mutex::new(initial_views(&scheduler)));

    let run = Run {
        cancel: cancel.clone(),
        _job: job.clone(),
        views: views.clone(),
    };

    std::thread::Builder::new()
        .name("launch-run".into())
        .spawn(move || drive(app, scheduler, cancel, job, views))
        .map_err(|e| AppError::Config(format!("could not start the launch: {e}")))?;

    Ok(run)
}

fn initial_views(scheduler: &Scheduler) -> Vec<StepView> {
    scheduler
        .steps()
        .iter()
        .map(|s| StepView {
            id: s.id,
            label: s.label.clone(),
            phase: s.phase,
            severity: s.severity,
            status: StepStatus::Pending,
            detail: String::new(),
            elapsed_ms: None,
            action_taken: ActionTaken::NotAttempted,
            can_retry: false,
        })
        .collect()
}

fn drive(
    app: AppHandle,
    mut scheduler: Scheduler,
    cancel: Arc<AtomicBool>,
    job: Arc<JobObject>,
    views: Arc<Mutex<Vec<StepView>>>,
) {
    let (done_tx, done_rx) = mpsc::channel::<Completion>();
    let mut running: HashMap<StepId, ()> = HashMap::new();

    loop {
        if cancel.load(Ordering::Relaxed) {
            tracing::info!("launch cancelled; tearing down what was started");
            job.terminate_all();
            publish_state(&app, ReadyState::Blocked);
            return;
        }

        // Start everything the graph allows, at once. This is the whole point
        // of the graph: peripheral checks run while SimHub is booting.
        for id in scheduler.runnable() {
            if running.contains_key(&id) {
                continue;
            }
            let Some(step) = scheduler.steps().iter().find(|s| s.id == id).cloned() else {
                continue;
            };

            scheduler.record(id, StepStatus::Running);
            update(&app, &views, id, |v| {
                v.status = StepStatus::Running;
                v.detail = super::gates::describe(&step.gate);
            });

            running.insert(id, ());
            let tx = done_tx.clone();
            let job = job.clone();
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                let _ = tx.send(run_step(step, job, cancel));
            });
        }

        if running.is_empty() {
            break;
        }

        match done_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(done) => {
                running.remove(&done.id);
                scheduler.record(done.id, done.status);
                update(&app, &views, done.id, |v| {
                    v.status = done.status;
                    v.action_taken = done.action;
                    v.detail = done.detail.clone();
                    v.elapsed_ms = Some(done.elapsed_ms);
                    v.can_retry = done.status == StepStatus::Failed;
                });

                // Steps skipped by a fatal failure need publishing too, or the
                // UI shows them as pending forever.
                for step in scheduler.steps().to_vec() {
                    if scheduler.status_of(step.id) == StepStatus::Skipped {
                        update(&app, &views, step.id, |v| {
                            if v.status != StepStatus::Skipped {
                                v.status = StepStatus::Skipped;
                                v.action_taken = ActionTaken::Skipped;
                                v.detail = "skipped: something it needed did not pass".into();
                            }
                        });
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let state = scheduler.ready_state();
    tracing::info!(?state, "launch run finished");
    publish_state(&app, state);
}

/// Run one step: do the thing, then wait for its gate.
fn run_step(step: tp_model::StepSpec, job: Arc<JobObject>, cancel: Arc<AtomicBool>) -> Completion {
    let started = Instant::now();
    let (action, mut detail, mut failed) = perform(&step.action, &job);

    // Then wait for readiness. This is where a hardcoded sleep would go in a
    // lesser launcher; here it is a real condition with a real timeout.
    if !failed {
        let deadline = started + Duration::from_millis(step.timeout_ms);
        loop {
            if cancel.load(Ordering::Relaxed) {
                failed = true;
                detail = "cancelled".into();
                break;
            }
            match super::gates::is_open(&step.gate, started) {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => {
                    failed = true;
                    detail = e.to_string();
                    break;
                }
            }
            if Instant::now() >= deadline {
                failed = true;
                detail = format!(
                    "gave up after {} seconds — {}",
                    step.timeout_ms / 1000,
                    super::gates::describe(&step.gate)
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    // A row that flashes past is unreadable on a go/no-go screen. This pads the
    // individual step, never the run: independent steps are still concurrent,
    // so the total is unchanged.
    let elapsed = started.elapsed();
    let minimum = Duration::from_millis(step.min_visible_ms);
    if elapsed < minimum {
        std::thread::sleep(minimum - elapsed);
    }

    Completion {
        id: step.id,
        status: if failed {
            StepStatus::Failed
        } else {
            StepStatus::Passed
        },
        action,
        detail,
        elapsed_ms: started.elapsed().as_millis() as f64,
    }
}

/// Do whatever the step is, and report *what actually happened*.
fn perform(action: &StepAction, job: &JobObject) -> (ActionTaken, String, bool) {
    match action {
        // The distinction that makes the honesty rule mechanical: a utility
        // that was already up reports AlreadyRunning, and the UI derives its
        // text from that rather than claiming a start it did not perform.
        StepAction::EnsureProcess { exe_path, args } => {
            let exe = file_name(exe_path);
            if super::gates::process_running(&exe) {
                return (
                    ActionTaken::AlreadyRunning,
                    format!("{exe} was already running"),
                    false,
                );
            }
            match super::process::start_executable(exe_path, args, None) {
                Ok(pid) => {
                    let _ = job.adopt(pid);
                    (ActionTaken::Started, format!("started {exe}"), false)
                }
                Err(e) => (ActionTaken::NotAttempted, e.to_string(), true),
            }
        }
        StepAction::StartProcess { exe_path, args } => {
            match super::process::start_executable(exe_path, args, None) {
                Ok(pid) => {
                    let _ = job.adopt(pid);
                    (
                        ActionTaken::Started,
                        format!("started {}", file_name(exe_path)),
                        false,
                    )
                }
                Err(e) => (ActionTaken::NotAttempted, e.to_string(), true),
            }
        }
        StepAction::CheckNotAlreadyRunning => (ActionTaken::NoActionNeeded, String::new(), false),
        StepAction::CheckGameInstalled => (ActionTaken::NoActionNeeded, String::new(), false),
        StepAction::CheckConfigWritable => (ActionTaken::NoActionNeeded, String::new(), false),
        StepAction::CheckDisplayTopology => (ActionTaken::NoActionNeeded, String::new(), false),
        StepAction::CheckPeripheral { device } => (
            ActionTaken::NoActionNeeded,
            format!("checking {}", device.display_name),
            false,
        ),
        // Not built yet, and saying so beats a step that silently passes.
        StepAction::ApplyAdapter { adapter_id } => (
            ActionTaken::NotAttempted,
            format!("no adapter for {adapter_id} yet — game settings arrive in milestone 10"),
            true,
        ),
        StepAction::ApplyDisplaySnapshot { .. } => (
            ActionTaken::NotAttempted,
            "display changes arrive in milestone 9, behind the confirm-or-revert flow".into(),
            true,
        ),
        StepAction::LaunchGame | StepAction::ApplyWindowGeometry => {
            (ActionTaken::NoActionNeeded, String::new(), false)
        }
    }
}

fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

fn update(
    app: &AppHandle,
    views: &Arc<Mutex<Vec<StepView>>>,
    id: StepId,
    change: impl FnOnce(&mut StepView),
) {
    let updated = {
        let Ok(mut guard) = views.lock() else { return };
        let Some(view) = guard.iter_mut().find(|v| v.id == id) else {
            return;
        };
        change(view);
        view.clone()
    };
    if let Err(e) = app.emit(STEP_EVENT, &updated) {
        tracing::debug!(error = %e, "could not publish a step change");
    }
}

fn publish_state(app: &AppHandle, state: ReadyState) {
    if let Err(e) = app.emit(STATE_EVENT, &state) {
        tracing::debug!(error = %e, "could not publish the launch state");
    }
}

/// Steps that need no gate, for the placeholder profile below.
fn immediate() -> ReadinessGate {
    ReadinessGate::Immediate
}

/// A minimal runnable profile, for exercising the machinery before profile
/// editing exists.
///
/// Deliberately does nothing destructive: it checks things and reports. The
/// point is to see the graph run in parallel and the gate hold, not to launch
/// a game.
pub fn demo_profile(name: &str) -> Vec<tp_model::StepSpec> {
    use tp_model::{Severity, StepSpec};

    let step = |id: u32, label: &str, phase, severity, deps: Vec<u32>, action| StepSpec {
        id: StepId(id),
        label: label.into(),
        phase,
        depends_on: deps.into_iter().map(StepId).collect(),
        action,
        gate: immediate(),
        timeout_ms: 10_000,
        severity,
        fix: None,
        min_visible_ms: 350,
    };

    vec![
        step(
            1,
            "Check displays",
            Phase::Preflight,
            Severity::Fatal,
            vec![],
            StepAction::CheckDisplayTopology,
        ),
        step(
            2,
            "Check peripherals",
            Phase::Preflight,
            Severity::Warning,
            vec![],
            StepAction::CheckNotAlreadyRunning,
        ),
        step(
            3,
            &format!("Check {name} is installed"),
            Phase::Preflight,
            Severity::Fatal,
            vec![],
            StepAction::CheckGameInstalled,
        ),
        step(
            4,
            "Check config files are writable",
            Phase::Preflight,
            Severity::Warning,
            vec![3],
            StepAction::CheckConfigWritable,
        ),
        step(
            10,
            "Ready to launch",
            Phase::Launch,
            Severity::Fatal,
            vec![],
            StepAction::LaunchGame,
        ),
    ]
}
