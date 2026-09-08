//! Running a profile.
//!
//! The scheduler in `tp_model::graph` says what may run; this runs it, in
//! parallel where the graph allows, and publishes every status change so the
//! UI can be a pure view over the stream rather than driving the sequence
//! itself.
//!
//! Four rules shape the loop:
//!
//! * **Nothing claims credit it has not earned.** A utility that was already
//!   running reports `AlreadyRunning`, and the row's text is derived from that
//!   rather than written by hand.
//! * **A step is visible for at least a moment.** An instant pass that flashes
//!   past is worse than useless on a go/no-go screen — but the padding is per
//!   step and never delays the whole run.
//! * **Launching is a second, deliberate press.** Preflight runs on its own;
//!   the launch phase waits for a request that only the user makes. The
//!   scheduler's phase gate stops it running early even if this code asked.
//! * **Cancelling tears down what was started.** Leaving half a session running
//!   because someone changed their mind is how a launcher earns distrust.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tp_model::{
    ActionTaken, LaunchMethod, Phase, Profile, ReadinessGate, ReadyState, Scheduler, Severity,
    StepAction, StepId, StepStatus, StepView,
};

use super::job::JobObject;
use super::process::Started;
use crate::error::{AppError, AppResult};

/// Emitted for every status change.
pub const STEP_EVENT: &str = "launch://step";
/// Emitted when the run reaches a terminal state.
pub const STATE_EVENT: &str = "launch://state";

/// Something the user asked for while a run was in progress.
///
/// Queued rather than applied directly, because the driver thread owns the
/// scheduler. A command handler that mutated it from the UI thread would be
/// racing the very loop it is trying to steer.
#[derive(Debug, Clone, Copy)]
pub enum Request {
    /// Re-run this step and everything downstream of it.
    Retry(StepId),
    /// Stop this step blocking the gate. The per-row form of "race anyway".
    Skip(StepId),
    /// Open the gate and run the launch phase. `force` skips whatever is still
    /// failing first — the whole-run form of the same override.
    Launch { force: bool },
}

/// A run in progress.
pub struct Run {
    cancel: Arc<AtomicBool>,
    /// Kept so the session's utilities die with the app rather than outliving
    /// a crash. Dropping this terminates them.
    _job: Arc<JobObject>,
    views: Arc<Mutex<Vec<StepView>>>,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Run {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn views(&self) -> Vec<StepView> {
        self.views.lock().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn request(&self, request: Request) {
        if let Ok(mut queue) = self.requests.lock() {
            queue.push(request);
        }
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

/// A snapshot of connected devices, for peripheral gates.
///
/// Taken from the watch thread rather than re-enumerated: it has already run
/// every status through its debouncer, and a second enumeration path would be a
/// second answer to the same question.
pub type DeviceSource = Arc<dyn Fn() -> Vec<tp_model::DetectedDevice> + Send + Sync>;

/// Everything the driver thread needs, in one place rather than eight
/// parameters threaded through five functions.
struct Context {
    app: AppHandle,
    profile: Arc<Profile>,
    cancel: Arc<AtomicBool>,
    job: Arc<JobObject>,
    views: Arc<Mutex<Vec<StepView>>>,
    devices: DeviceSource,
    requests: Arc<Mutex<Vec<Request>>>,
}

/// Start running a profile's preflight phase.
///
/// The launch phase is *not* started here. It waits for `Request::Launch`.
pub fn start(app: AppHandle, profile: Profile, devices: DeviceSource) -> AppResult<Run> {
    let scheduler = Scheduler::new(profile.steps.clone())
        .map_err(|e| AppError::Config(format!("this profile cannot run: {e}")))?;

    let cancel = Arc::new(AtomicBool::new(false));
    let job = Arc::new(JobObject::new()?);
    let views = Arc::new(Mutex::new(initial_views(&scheduler)));
    let requests = Arc::new(Mutex::new(Vec::new()));

    let run = Run {
        cancel: cancel.clone(),
        _job: job.clone(),
        views: views.clone(),
        requests: requests.clone(),
    };

    let context = Context {
        app,
        profile: Arc::new(profile),
        cancel,
        job,
        views,
        devices,
        requests,
    };

    std::thread::Builder::new()
        .name("launch-run".into())
        .spawn(move || drive(context, scheduler))
        .map_err(|e| AppError::Config(format!("could not start the launch: {e}")))?;

    Ok(run)
}

fn initial_views(scheduler: &Scheduler) -> Vec<StepView> {
    scheduler.steps().iter().map(pending_view).collect()
}

fn pending_view(step: &tp_model::StepSpec) -> StepView {
    StepView {
        id: step.id,
        label: step.label.clone(),
        phase: step.phase,
        severity: step.severity,
        status: StepStatus::Pending,
        detail: String::new(),
        elapsed_ms: None,
        action_taken: ActionTaken::NotAttempted,
        can_retry: false,
        fix: step.fix.clone(),
    }
}

fn drive(context: Context, mut scheduler: Scheduler) {
    run_phase(&context, &mut scheduler, Phase::Preflight);
    if context.cancel.load(Ordering::Relaxed) {
        return teardown(&context);
    }

    let state = scheduler.ready_state();
    tracing::info!(?state, "preflight settled");
    publish_state(&context.app, state);

    supervise(context, scheduler);
}

/// Run every step of one phase, starting each as soon as the graph allows.
///
/// Returns when nothing in the phase can start and nothing is still running.
fn run_phase(context: &Context, scheduler: &mut Scheduler, phase: Phase) {
    let (done_tx, done_rx) = mpsc::channel::<Completion>();
    let mut running: HashMap<StepId, ()> = HashMap::new();

    loop {
        if context.cancel.load(Ordering::Relaxed) {
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
            // The phase gate is the scheduler's, but a driver that pulled a
            // launch step off the runnable list while the user is still looking
            // at the checklist would start the game without being asked.
            if step.phase != phase {
                continue;
            }

            scheduler.record(id, StepStatus::Running);
            update(context, id, |v| {
                v.status = StepStatus::Running;
                v.detail = super::gates::describe(&step.gate);
                v.can_retry = false;
            });

            running.insert(id, ());
            let tx = done_tx.clone();
            let job = context.job.clone();
            let cancel = context.cancel.clone();
            let devices = context.devices.clone();
            let profile = context.profile.clone();
            std::thread::spawn(move || {
                let _ = tx.send(run_step(step, profile, job, cancel, devices));
            });
        }

        if running.is_empty() {
            return;
        }

        match done_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(done) => {
                running.remove(&done.id);
                scheduler.record(done.id, done.status);
                update(context, done.id, |v| {
                    v.status = done.status;
                    v.action_taken = done.action;
                    v.detail = done.detail.clone();
                    v.elapsed_ms = Some(done.elapsed_ms);
                    v.can_retry = done.status == StepStatus::Failed;
                });
                publish_skipped(context, scheduler);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Steps the scheduler skipped need publishing too, or the UI shows them as
/// pending forever.
fn publish_skipped(context: &Context, scheduler: &Scheduler) {
    for step in scheduler.steps() {
        if scheduler.status_of(step.id) != StepStatus::Skipped {
            continue;
        }
        let id = step.id;
        update(context, id, |v| {
            if v.status != StepStatus::Skipped {
                v.status = StepStatus::Skipped;
                v.action_taken = ActionTaken::Skipped;
                v.detail = "skipped: something it needed did not pass".into();
                v.can_retry = true;
            }
        });
    }
}

/// After preflight settles: heal what recovers on its own, and act on what the
/// user asks for.
///
/// The healing is the most important behaviour on this screen. If the pedals
/// are unplugged the row goes red; when they are plugged back in it turns green
/// by itself. Nobody should have to hunt for a Retry button for something they
/// just fixed — manual retry exists as a fallback, not as the primary path.
///
/// Only failed steps are re-checked, and only their gate: re-running a step's
/// *action* would restart SimHub because a pedal check failed, which is exactly
/// the behaviour that makes a preflight useless. A retry the user asked for
/// does re-run the action, because that is what they asked for.
fn supervise(context: Context, mut scheduler: Scheduler) {
    let started = Instant::now();
    let mut tick: u32 = 0;
    let mut launched = false;

    loop {
        if context.cancel.load(Ordering::Relaxed) {
            return teardown(&context);
        }
        std::thread::sleep(Duration::from_millis(150));
        tick = tick.wrapping_add(1);

        let queued: Vec<Request> = match context.requests.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        };

        for request in queued {
            match request {
                Request::Retry(id) => {
                    if launched {
                        continue;
                    }
                    retry(&context, &mut scheduler, id);
                }
                Request::Skip(id) => {
                    if launched {
                        continue;
                    }
                    skip(&context, &mut scheduler, id);
                }
                Request::Launch { force } => {
                    if launched {
                        continue;
                    }
                    launched = true;
                    if force {
                        skip_remaining_failures(&context, &mut scheduler);
                    }
                    publish_state(&context.app, ReadyState::Launching);
                    run_phase(&context, &mut scheduler, Phase::Launch);
                    if context.cancel.load(Ordering::Relaxed) {
                        return teardown(&context);
                    }
                    let state = if scheduler.has_fatal_failure(Phase::Launch) {
                        ReadyState::LaunchFailed
                    } else {
                        ReadyState::Racing
                    };
                    tracing::info!(?state, "launch phase settled");
                    publish_state(&context.app, state);
                }
            }
        }

        // Healing stops once the game is up: a row flipping green behind a
        // running sim is noise, and re-checking costs a process enumeration.
        if launched || tick % 4 != 0 {
            continue;
        }
        if heal_once(&context, &mut scheduler, started) {
            publish_state(&context.app, scheduler.ready_state());
        }
    }
}

/// Re-check failed steps' gates. Returns whether anything recovered.
///
/// Two exclusions, and both matter:
///
/// * **Only `Failed`, never `Skipped`.** A skipped step never ran — either
///   something it needed failed, or the user overrode it. Passing it because
///   its gate happens to be open would claim a result nothing produced.
/// * **Only steps with a real gate.** A step gated on `Immediate` failed in its
///   *action*, and `Immediate` is open by definition — re-checking it would
///   turn every failed action green a moment later while nothing had changed.
///   "The game is installed" going green over a folder that is still missing is
///   exactly the lie this whole app is built not to tell.
fn heal_once(context: &Context, scheduler: &mut Scheduler, started: Instant) -> bool {
    let recovered: Vec<StepId> = scheduler
        .steps()
        .iter()
        .filter(|s| s.phase == Phase::Preflight)
        .filter(|s| !matches!(s.gate, ReadinessGate::Immediate))
        .filter(|s| scheduler.status_of(s.id) == StepStatus::Failed)
        .filter(|s| gate_open(&s.gate, started, &context.devices))
        .map(|s| s.id)
        .collect();

    if recovered.is_empty() {
        return false;
    }

    for id in recovered {
        tracing::info!(?id, "condition recovered on its own");
        scheduler.record(id, StepStatus::Passed);
        update(context, id, |v| {
            v.status = StepStatus::Passed;
            v.action_taken = ActionTaken::NoActionNeeded;
            v.detail = "fixed".into();
            v.can_retry = false;
        });
    }
    true
}

/// Re-run one step and everything downstream of it.
fn retry(context: &Context, scheduler: &mut Scheduler, id: StepId) {
    scheduler.reset_from(id);
    for step in scheduler.steps().to_vec() {
        if scheduler.status_of(step.id) == StepStatus::Pending {
            update(context, step.id, |v| {
                *v = pending_view(&step);
            });
        }
    }
    publish_state(&context.app, ReadyState::Running);
    run_phase(context, scheduler, Phase::Preflight);

    // Retrying a step whose own dependency is still failing resets it to
    // Pending and then cannot run it, because the graph will not let it. Left
    // alone that is a checklist stuck on "Checking" forever, so anything the
    // retry could not reach goes back to Skipped and the gate settles.
    settle_unreachable(context, scheduler);
    publish_state(&context.app, scheduler.ready_state());
}

/// Anything still Pending after a phase has run cannot run at all.
fn settle_unreachable(context: &Context, scheduler: &mut Scheduler) {
    let stranded: Vec<StepId> = scheduler
        .steps()
        .iter()
        .filter(|s| s.phase == Phase::Preflight)
        .filter(|s| scheduler.status_of(s.id) == StepStatus::Pending)
        .map(|s| s.id)
        .collect();
    for id in stranded {
        scheduler.record(id, StepStatus::Skipped);
        update(context, id, |v| {
            v.status = StepStatus::Skipped;
            v.action_taken = ActionTaken::Skipped;
            v.detail = "skipped: something it needed still has not passed".into();
            v.can_retry = true;
        });
    }
}

/// Stop a step blocking the gate, without pretending it passed.
///
/// Skipped is its own status with its own glyph and word, so the checklist
/// still shows that this was overridden rather than satisfied.
fn skip(context: &Context, scheduler: &mut Scheduler, id: StepId) {
    scheduler.record(id, StepStatus::Skipped);
    update(context, id, |v| {
        v.status = StepStatus::Skipped;
        v.action_taken = ActionTaken::Skipped;
        v.detail = "skipped on purpose".into();
        v.can_retry = true;
    });
    publish_state(&context.app, scheduler.ready_state());
}

/// "Race anyway": skip everything still failing so the gate opens.
fn skip_remaining_failures(context: &Context, scheduler: &mut Scheduler) {
    let failed: Vec<StepId> = scheduler
        .steps()
        .iter()
        .filter(|s| s.phase == Phase::Preflight && s.severity == Severity::Fatal)
        .filter(|s| scheduler.status_of(s.id) == StepStatus::Failed)
        .map(|s| s.id)
        .collect();
    for id in failed {
        tracing::warn!(?id, "overridden by the user; launching anyway");
        skip(context, scheduler, id);
    }
}

/// Run one step: do the thing, then wait for its gate.
fn run_step(
    step: tp_model::StepSpec,
    profile: Arc<Profile>,
    job: Arc<JobObject>,
    cancel: Arc<AtomicBool>,
    devices: DeviceSource,
) -> Completion {
    let started = Instant::now();
    let (action, mut detail, mut failed) = perform(&step.action, &profile, &job);

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
            if gate_open(&step.gate, started, &devices) {
                break;
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
fn perform(action: &StepAction, profile: &Profile, job: &JobObject) -> (ActionTaken, String, bool) {
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

        // The gate does the checking for this one — ProcessAbsent. Reporting
        // NoActionNeeded here and letting the gate decide keeps one answer to
        // the question rather than two that can disagree.
        StepAction::CheckNotAlreadyRunning => (ActionTaken::NoActionNeeded, String::new(), false),

        StepAction::CheckGameInstalled => match &profile.game.install_path {
            Some(path) if std::path::Path::new(path).is_dir() => {
                (ActionTaken::NoActionNeeded, path.clone(), false)
            }
            Some(path) => (
                ActionTaken::NoActionNeeded,
                format!("{path} is not there — has it moved or been uninstalled?"),
                true,
            ),
            // A profile made from a launcher address alone never knew a folder.
            // Saying so beats a green row that checked nothing.
            None => (
                ActionTaken::NoActionNeeded,
                "no install folder recorded; taking the launcher's word for it".into(),
                false,
            ),
        },

        StepAction::CheckConfigWritable => (ActionTaken::NoActionNeeded, String::new(), false),
        StepAction::CheckDisplayTopology => (ActionTaken::NoActionNeeded, String::new(), false),
        StepAction::CheckPeripheral { device } => (
            ActionTaken::NoActionNeeded,
            format!("checking {}", device.display_name),
            false,
        ),

        StepAction::LaunchGame => match launch(&profile.game.launch) {
            // A direct launch is the only case where we hold the process id,
            // and the only one where "started it" is literally true.
            Ok(Started::Pid(pid)) => (
                ActionTaken::Started,
                format!("started, process {pid}"),
                false,
            ),
            // A protocol launch hands off to Steam or Epic and gets nothing
            // back. The game appears later as a grandchild we never started, so
            // this claims the handoff and nothing more.
            Ok(Started::Detached) => (
                ActionTaken::Started,
                "asked the launcher to start it".into(),
                false,
            ),
            Err(e) => (ActionTaken::NotAttempted, e.to_string(), true),
        },

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
        StepAction::ApplyWindowGeometry => (ActionTaken::NoActionNeeded, String::new(), false),
    }
}

/// Start the game.
///
/// Deliberately *not* adopted into the Job Object. The utilities are the app's
/// to clean up; the game is the user's, and killing their session because Team
/// Principal exited would be indefensible.
fn launch(method: &LaunchMethod) -> AppResult<Started> {
    match method {
        LaunchMethod::Executable {
            path,
            args,
            working_dir,
        } => super::process::start_executable(path, args, working_dir.as_deref()).map(Started::Pid),
        LaunchMethod::Steam { app_id } => {
            super::process::start_uri(&format!("steam://rungameid/{app_id}"))
        }
        LaunchMethod::Epic { app_name } => super::process::start_uri(&format!(
            "com.epicgames.launcher://apps/{app_name}?action=launch&silent=true"
        )),
        LaunchMethod::Uwp {
            package_family_name,
        } => super::process::start_uri(&format!(r"shell:appsFolder\{package_family_name}!App")),
        LaunchMethod::Uri { uri } => super::process::start_uri(uri),
    }
}

fn teardown(context: &Context) {
    tracing::info!("launch cancelled; tearing down what was started");
    context.job.terminate_all();
    publish_state(&context.app, ReadyState::Blocked);
}

fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

fn update(context: &Context, id: StepId, change: impl FnOnce(&mut StepView)) {
    let updated = {
        let Ok(mut guard) = context.views.lock() else {
            return;
        };
        let Some(view) = guard.iter_mut().find(|v| v.id == id) else {
            return;
        };
        change(view);
        view.clone()
    };
    if let Err(e) = context.app.emit(STEP_EVENT, &updated) {
        tracing::debug!(error = %e, "could not publish a step change");
    }
}

fn publish_state(app: &AppHandle, state: ReadyState) {
    if let Err(e) = app.emit(STATE_EVENT, &state) {
        tracing::debug!(error = %e, "could not publish the launch state");
    }
}

/// Evaluate a gate, resolving peripheral checks against the watch thread.
///
/// `PeripheralConnected` cannot be answered by `gates::is_open` alone, because
/// the authority for device status is the watcher — which has already debounced
/// it. Answering it here keeps one source of truth.
fn gate_open(gate: &ReadinessGate, started: Instant, devices: &DeviceSource) -> bool {
    match gate {
        ReadinessGate::PeripheralConnected { device } => {
            let connected = devices();
            connected.iter().any(|d| {
                let same = match (&device.instance_path, &d.device.instance_path) {
                    // The instance path is the only thing that separates two
                    // identical un-serialled devices.
                    (Some(want), Some(have)) => want == have,
                    _ => d.device.vid == device.vid && d.device.pid == device.pid,
                };
                same && d.status == tp_model::DeviceStatus::Connected
            })
        }
        ReadinessGate::All { gates } => gates.iter().all(|g| gate_open(g, started, devices)),
        ReadinessGate::Any { gates } => gates.iter().any(|g| gate_open(g, started, devices)),
        other => super::gates::is_open(other, started).unwrap_or(false),
    }
}
