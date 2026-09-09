//! Turning a profile into the steps that run.
//!
//! A profile says *what the user wants*: this game, these peripherals must be
//! connected, start these utilities first. This turns that into the graph the
//! scheduler executes.
//!
//! Building it here rather than in the executor means the shape of a run — what
//! depends on what, what is fatal, what merely warns — is tested without
//! starting a single process.

use crate::{
    LaunchMethod, Necessity, Phase, Profile, ReadinessGate, Severity, StepAction, StepId, StepSpec,
};

/// Build the preflight and launch graph for a profile.
///
/// Ordering rules, and the reasoning for each:
///
/// * Peripheral checks depend on nothing, so they run while utilities boot.
///   That is the whole reason this is a graph.
/// * A utility depends only on the utilities it declares, never on peripheral
///   checks — SimHub does not care whether the pedals are plugged in.
/// * Launching depends on nothing explicitly: the phase gate already holds it
///   until every preflight step has passed.
pub fn build_steps(profile: &Profile) -> Vec<StepSpec> {
    let mut steps = Vec::new();
    let mut next_id = 1u32;
    let mut id = || {
        let v = StepId(next_id);
        next_id += 1;
        v
    };

    // Peripherals first in the list, so they are first on screen — they are
    // what the user is most likely to need to fix.
    for requirement in &profile.peripherals {
        let device = requirement.device.clone();
        steps.push(StepSpec {
            id: id(),
            label: format!("{} connected", device.display_name),
            phase: Phase::Preflight,
            depends_on: Vec::new(),
            gate: ReadinessGate::PeripheralConnected {
                device: device.clone(),
            },
            action: StepAction::CheckPeripheral { device },
            timeout_ms: 20_000,
            // Optional means optional. A warning that blocked the gate would
            // make the distinction meaningless.
            severity: match requirement.necessity {
                Necessity::Required => Severity::Fatal,
                Necessity::Optional => Severity::Warning,
            },
            fix: Some(crate::FixAction::Retry),
            min_visible_ms: 350,
        });
    }

    // Utilities, chained only where they declare a dependency on each other.
    let mut utility_ids: Vec<(String, StepId)> = Vec::new();
    for utility in &profile.utilities {
        let step_id = id();
        let depends_on = utility
            .after
            .iter()
            .filter_map(|name| {
                utility_ids
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(_, sid)| *sid)
            })
            .collect();

        steps.push(StepSpec {
            id: step_id,
            label: utility.label.clone(),
            phase: Phase::Preflight,
            depends_on,
            action: StepAction::EnsureProcess {
                exe_path: utility.exe_path.clone(),
                args: utility.args.clone(),
            },
            gate: utility.ready_when.clone(),
            timeout_ms: utility.timeout_ms,
            severity: if utility.required {
                Severity::Fatal
            } else {
                Severity::Warning
            },
            fix: Some(crate::FixAction::Retry),
            min_visible_ms: 350,
        });
        utility_ids.push((utility.label.clone(), step_id));
    }

    // Game settings, written from the rig. Only when the profile actually names
    // an adapter — a step that reports "no adapter" on every launch is a step
    // people stop reading.
    if !profile.game.adapter_id.is_empty() {
        let writable = id();
        steps.push(StepSpec {
            id: writable,
            label: "Game settings are writable".into(),
            phase: Phase::Preflight,
            depends_on: Vec::new(),
            action: StepAction::CheckConfigWritable,
            gate: ReadinessGate::Immediate,
            timeout_ms: 5_000,
            severity: Severity::Warning,
            fix: Some(crate::FixAction::Retry),
            min_visible_ms: 350,
        });
        steps.push(StepSpec {
            id: id(),
            label: "Write your rig into the game".into(),
            phase: Phase::Preflight,
            // Pointless to attempt if the file could not be written, and the
            // failure would be the same one reported twice.
            depends_on: vec![writable],
            action: StepAction::ApplyAdapter {
                adapter_id: profile.game.adapter_id.clone(),
            },
            gate: ReadinessGate::Immediate,
            timeout_ms: 15_000,
            // A warning: the settings may already be right from last time, and
            // refusing to race because a config write failed would be worse
            // than racing with last session's numbers.
            severity: Severity::Warning,
            fix: Some(crate::FixAction::Retry),
            min_visible_ms: 350,
        });
    }

    // The game itself. The action does the checking — the gate is immediate
    // because there is nothing to wait *for*: a game either is installed or is
    // not, and polling would not change the answer.
    steps.push(StepSpec {
        id: id(),
        label: format!("{} is installed", profile.name),
        phase: Phase::Preflight,
        depends_on: Vec::new(),
        action: StepAction::CheckGameInstalled,
        gate: ReadinessGate::Immediate,
        timeout_ms: 5_000,
        severity: Severity::Fatal,
        fix: None,
        min_visible_ms: 350,
    });

    // Not already running: launching a second copy of a sim is a good way to
    // lose the one that is already open. Only checkable where the executable
    // is knowable — a Steam id names no process until the profile's window
    // target says which one to expect.
    if let Some(exe) = expected_exe(profile) {
        steps.push(StepSpec {
            id: id(),
            label: format!("{exe} is not already running"),
            phase: Phase::Preflight,
            depends_on: Vec::new(),
            action: StepAction::CheckNotAlreadyRunning,
            // A real gate rather than an immediate pass. A check that always
            // passes is worse than no check: it teaches people to trust a row
            // that is not looking at anything.
            gate: ReadinessGate::ProcessAbsent { exe },
            timeout_ms: 5_000,
            // A warning, not fatal: someone who alt-tabbed out of a running sim
            // and pressed the button again should be told, not stopped.
            severity: Severity::Warning,
            fix: Some(crate::FixAction::Skip),
            min_visible_ms: 350,
        });
    }

    let launch_id = id();
    steps.push(StepSpec {
        id: launch_id,
        label: format!("Launch {}", profile.name),
        phase: Phase::Launch,
        depends_on: Vec::new(),
        action: StepAction::LaunchGame,
        // Where the executable is knowable, "launched" means the process is
        // up — not that the launcher accepted the request. A protocol launch
        // with no expected executable can honestly only claim the latter, and
        // the row's text says so.
        gate: match expected_exe(profile) {
            Some(exe) => ReadinessGate::ProcessExists { exe },
            None => ReadinessGate::Immediate,
        },
        timeout_ms: profile.window_plan.target.timeout_ms,
        severity: Severity::Fatal,
        fix: Some(crate::FixAction::Retry),
        min_visible_ms: 350,
    });

    // Placing the window, when the profile has a rectangle that was proven by
    // hand and the switch is on. Depends on the launch step: there is no window
    // to place until the game has started.
    if profile.window_plan.auto_apply {
        steps.push(StepSpec {
            id: id(),
            label: "Place the game window".into(),
            phase: Phase::Launch,
            depends_on: vec![launch_id],
            action: StepAction::ApplyWindowGeometry,
            gate: ReadinessGate::Immediate,
            // Long, because this waits for the window to exist. A sim showing a
            // splash screen and compiling shaders can take most of a minute
            // before it opens the window that matters.
            timeout_ms: profile.window_plan.target.timeout_ms.max(60_000),
            // A warning, not fatal: the game is running by this point, and
            // refusing to race because a window is a few pixels out would be
            // absurd.
            severity: Severity::Warning,
            fix: Some(crate::FixAction::Retry),
            min_visible_ms: 350,
        });
    }

    steps.extend(teardown_steps(profile, &mut id));
    steps
}

/// What to undo when the session ends.
///
/// Never scheduled with the rest — `Scheduler` refuses to run a teardown step
/// alongside a launch one — because teardown runs when the *game exits*, which
/// may be an hour later, or when the user cancels. Building the steps here
/// anyway means the plan is complete and inspectable before anything starts,
/// rather than assembled in a hurry at the point of failure.
fn teardown_steps(profile: &Profile, id: &mut impl FnMut() -> StepId) -> Vec<StepSpec> {
    let mut steps = Vec::new();

    if profile.teardown.restore_configs {
        steps.push(StepSpec {
            id: id(),
            label: "Put your game settings back".into(),
            phase: Phase::Teardown,
            depends_on: Vec::new(),
            action: StepAction::RestoreConfigs,
            gate: ReadinessGate::Immediate,
            timeout_ms: 15_000,
            severity: Severity::Warning,
            fix: Some(crate::FixAction::Retry),
            min_visible_ms: 350,
        });
    }

    if profile.teardown.restore_display {
        steps.push(StepSpec {
            id: id(),
            label: "Put your displays back".into(),
            phase: Phase::Teardown,
            depends_on: Vec::new(),
            action: StepAction::RestoreDisplay,
            gate: ReadinessGate::Immediate,
            timeout_ms: 30_000,
            severity: Severity::Warning,
            fix: Some(crate::FixAction::Retry),
            min_visible_ms: 350,
        });
    }

    if profile.teardown.close_utilities {
        steps.push(StepSpec {
            id: id(),
            label: "Close the utilities".into(),
            phase: Phase::Teardown,
            depends_on: Vec::new(),
            action: StepAction::CloseUtilities,
            gate: ReadinessGate::Immediate,
            timeout_ms: 15_000,
            severity: Severity::Warning,
            fix: None,
            min_visible_ms: 350,
        });
    }

    steps
}

/// The executable this profile's game will run as, where anything knows it.
///
/// The window target wins: for a Steam or Epic launch it is the only thing that
/// names the process, and a user who has filled it in has told us more than the
/// launch method can.
pub fn expected_exe(profile: &Profile) -> Option<String> {
    profile
        .window_plan
        .target
        .exe_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| launch_exe(&profile.game.launch))
}

/// The executable a launch method will eventually produce, where it is knowable.
pub fn launch_exe(method: &LaunchMethod) -> Option<String> {
    match method {
        LaunchMethod::Executable { path, .. } => {
            path.rsplit(['\\', '/']).next().map(str::to_string)
        }
        // A protocol launch names no executable. That is exactly why the window
        // matcher accepts an exe name from the profile instead.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn profile() -> Profile {
        Profile {
            schema_version: PROFILE_SCHEMA_VERSION,
            id: uuid::Uuid::nil(),
            name: "Assetto Corsa".into(),
            game: GameRef {
                adapter_id: String::new(),
                install_path: None,
                launch: LaunchMethod::Steam {
                    app_id: "244210".into(),
                },
                platform: Platform::Steam,
                art_path: None,
            },
            rig: RigBinding {
                rig_id: uuid::Uuid::nil(),
                computed_against_revision: 0,
                derived_snapshot: Default::default(),
            },
            session_mode: SessionMode::CenterOnly,
            window_plan: WindowPlan {
                target: WindowTarget {
                    exe_name: None,
                    window_class: None,
                    title_regex: None,
                    min_size: (640, 480),
                    require_visible: true,
                    timeout_ms: 30_000,
                },
                rect: RectSource::FromGeometry,
                rect_means: RectMeans::ClientArea,
                borderless: true,
                always_on_top: false,
                hide_taskbar: false,
                watchdog: WatchdogPolicy::default(),
                auto_apply: false,
            },
            peripherals: Vec::new(),
            utilities: Vec::new(),
            steps: Vec::new(),
            teardown: TeardownPolicy::default(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn device(name: &str) -> DeviceRef {
        DeviceRef {
            vid: 1,
            pid: 2,
            serial: None,
            instance_path: Some(name.into()),
            display_name: name.into(),
        }
    }

    fn utility(label: &str, after: &[&str]) -> UtilitySpec {
        UtilitySpec {
            label: label.into(),
            exe_path: format!(r"C:\{label}.exe"),
            args: Vec::new(),
            ready_when: ReadinessGate::ProcessExists {
                exe: format!("{label}.exe"),
            },
            timeout_ms: 30_000,
            required: true,
            after: after.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn peripheral_checks_depend_on_nothing() {
        // The whole reason this is a graph: pedals check while SimHub boots.
        let mut p = profile();
        p.peripherals = vec![PeripheralRequirement {
            device: device("Pedals"),
            necessity: Necessity::Required,
            expect_dinput_slot: None,
            expect_instance_guid: None,
            vendor_process: None,
            vjoy: None,
        }];
        p.utilities = vec![utility("SimHub", &[])];

        let steps = build_steps(&p);
        let pedals = steps.iter().find(|s| s.label.contains("Pedals")).unwrap();
        let simhub = steps.iter().find(|s| s.label == "SimHub").unwrap();
        assert!(pedals.depends_on.is_empty());
        assert!(
            simhub.depends_on.is_empty(),
            "SimHub does not care about pedals"
        );
    }

    #[test]
    fn required_is_fatal_and_optional_only_warns() {
        // Otherwise the distinction is decorative.
        let mut p = profile();
        p.peripherals = vec![
            PeripheralRequirement {
                device: device("Wheel"),
                necessity: Necessity::Required,
                expect_dinput_slot: None,
                expect_instance_guid: None,
                vendor_process: None,
                vjoy: None,
            },
            PeripheralRequirement {
                device: device("Button box"),
                necessity: Necessity::Optional,
                expect_dinput_slot: None,
                expect_instance_guid: None,
                vendor_process: None,
                vjoy: None,
            },
        ];
        let steps = build_steps(&p);
        assert_eq!(
            steps
                .iter()
                .find(|s| s.label.contains("Wheel"))
                .unwrap()
                .severity,
            Severity::Fatal
        );
        assert_eq!(
            steps
                .iter()
                .find(|s| s.label.contains("Button box"))
                .unwrap()
                .severity,
            Severity::Warning
        );
    }

    #[test]
    fn utilities_chain_only_where_they_declare_it() {
        let mut p = profile();
        p.utilities = vec![
            utility("vJoy feeder", &[]),
            utility("SimHub", &["vJoy feeder"]),
        ];
        let steps = build_steps(&p);

        let feeder = steps.iter().find(|s| s.label == "vJoy feeder").unwrap();
        let simhub = steps.iter().find(|s| s.label == "SimHub").unwrap();
        assert!(feeder.depends_on.is_empty());
        assert_eq!(simhub.depends_on, vec![feeder.id]);
    }

    #[test]
    fn a_dependency_on_something_absent_is_dropped_not_dangling() {
        // A dangling id would fail graph construction and make the whole
        // profile unrunnable because of one stale name.
        let mut p = profile();
        p.utilities = vec![utility("SimHub", &["something removed"])];
        let steps = build_steps(&p);
        assert!(steps
            .iter()
            .find(|s| s.label == "SimHub")
            .unwrap()
            .depends_on
            .is_empty());
        assert!(Scheduler::new(steps).is_ok());
    }

    #[test]
    fn the_window_targets_exe_name_beats_the_launch_method() {
        // A Steam id names no process. The window target is the only place that
        // can, and a user who filled it in has told us more than the launch
        // method ever could.
        let mut p = profile();
        assert_eq!(expected_exe(&p), None, "a Steam launch names no executable");

        p.window_plan.target.exe_name = Some("acs.exe".into());
        assert_eq!(expected_exe(&p), Some("acs.exe".into()));

        // Blank is not an answer.
        p.window_plan.target.exe_name = Some("  ".into());
        assert_eq!(expected_exe(&p), None);
    }

    #[test]
    fn the_not_already_running_check_actually_checks_something() {
        // It used to be an Immediate gate against an action that always passed,
        // which is worse than no check at all: it teaches people to trust a row
        // that is not looking at anything.
        let mut p = profile();
        p.window_plan.target.exe_name = Some("acs.exe".into());

        let steps = build_steps(&p);
        let step = steps
            .iter()
            .find(|s| matches!(s.action, StepAction::CheckNotAlreadyRunning))
            .expect("the check exists when the executable is knowable");
        assert_eq!(
            step.gate,
            ReadinessGate::ProcessAbsent {
                exe: "acs.exe".into()
            }
        );
        // A warning: someone who alt-tabbed out of a running sim should be told,
        // not stopped.
        assert_eq!(step.severity, Severity::Warning);
    }

    #[test]
    fn a_launch_with_no_knowable_executable_claims_only_the_handoff() {
        // Steam takes the request and the game appears later as a grandchild we
        // never started. Waiting on a process we cannot name would time out on
        // every launch.
        let steps = build_steps(&profile());
        let launch = steps.iter().find(|s| s.phase == Phase::Launch).unwrap();
        assert_eq!(launch.gate, ReadinessGate::Immediate);
        assert!(
            !steps
                .iter()
                .any(|s| matches!(s.action, StepAction::CheckNotAlreadyRunning)),
            "nothing to check when no executable is knowable"
        );
    }

    #[test]
    fn every_failable_step_offers_a_way_out() {
        // A red row with no button is a dead end. The check steps that cannot
        // be retried into passing are the exception, and they are the ones the
        // user fixes elsewhere.
        let mut p = profile();
        p.window_plan.target.exe_name = Some("acs.exe".into());
        p.peripherals = vec![PeripheralRequirement {
            device: device("Wheel"),
            necessity: Necessity::Required,
            expect_dinput_slot: None,
            expect_instance_guid: None,
            vendor_process: None,
            vjoy: None,
        }];
        p.utilities = vec![utility("SimHub", &[])];

        for step in build_steps(&p) {
            if matches!(step.action, StepAction::CheckGameInstalled) {
                continue; // Retrying cannot install a game.
            }
            assert!(step.fix.is_some(), "{} has no fix action", step.label);
        }
    }

    #[test]
    fn window_placement_is_only_planned_when_the_switch_is_on() {
        // And it waits for the launch, because there is no window to place
        // until the game has started.
        let mut p = profile();
        assert!(!build_steps(&p)
            .iter()
            .any(|s| matches!(s.action, StepAction::ApplyWindowGeometry)));

        p.window_plan.auto_apply = true;
        let steps = build_steps(&p);
        let launch = steps
            .iter()
            .find(|s| matches!(s.action, StepAction::LaunchGame))
            .unwrap();
        let place = steps
            .iter()
            .find(|s| matches!(s.action, StepAction::ApplyWindowGeometry))
            .unwrap();

        assert_eq!(place.depends_on, vec![launch.id]);
        assert_eq!(place.phase, Phase::Launch);
        // The game is already running by then; a few pixels out is not a reason
        // to call the launch a failure.
        assert_eq!(place.severity, Severity::Warning);
        assert!(Scheduler::new(steps).is_ok());
    }

    #[test]
    fn teardown_is_planned_but_never_runnable_alongside_the_session() {
        // The plan is complete and inspectable before anything starts, rather
        // than assembled in a hurry at the point of failure — but the scheduler
        // must never run it next to a launch step.
        let p = profile();
        let steps = build_steps(&p);
        let teardown: Vec<_> = steps
            .iter()
            .filter(|s| s.phase == Phase::Teardown)
            .collect();
        // The default policy restores configs and the display, and leaves
        // utilities alone.
        assert_eq!(teardown.len(), 2);

        let scheduler = Scheduler::new(steps.clone()).unwrap();
        for id in scheduler.runnable() {
            let step = steps.iter().find(|s| s.id == id).unwrap();
            assert_ne!(step.phase, Phase::Teardown, "{}", step.label);
        }
    }

    #[test]
    fn the_teardown_policy_decides_what_is_planned() {
        let mut p = profile();
        p.teardown = TeardownPolicy {
            restore_display: false,
            restore_configs: false,
            close_utilities: true,
            keep_utilities_on_cancel: true,
        };
        let steps = build_steps(&p);
        let actions: Vec<_> = steps
            .iter()
            .filter(|s| s.phase == Phase::Teardown)
            .map(|s| s.action.clone())
            .collect();
        assert_eq!(actions, vec![StepAction::CloseUtilities]);
    }

    #[test]
    fn game_settings_are_only_written_when_an_adapter_is_named() {
        // A step reporting "no adapter" on every launch is a step people stop
        // reading.
        let mut p = profile();
        assert!(!build_steps(&p)
            .iter()
            .any(|s| matches!(s.action, StepAction::ApplyAdapter { .. })));

        p.game.adapter_id = "assetto_corsa".into();
        let steps = build_steps(&p);
        let writable = steps
            .iter()
            .find(|s| matches!(s.action, StepAction::CheckConfigWritable))
            .unwrap();
        let write = steps
            .iter()
            .find(|s| matches!(s.action, StepAction::ApplyAdapter { .. }))
            .unwrap();
        // Pointless to attempt the write if the file could not be written, and
        // the failure would be the same one reported twice.
        assert_eq!(write.depends_on, vec![writable.id]);
        assert!(Scheduler::new(steps).is_ok());
    }

    #[test]
    fn the_graph_it_builds_is_always_valid() {
        let mut p = profile();
        p.peripherals = vec![PeripheralRequirement {
            device: device("Wheel"),
            necessity: Necessity::Required,
            expect_dinput_slot: None,
            expect_instance_guid: None,
            vendor_process: None,
            vjoy: None,
        }];
        p.utilities = vec![
            utility("A", &[]),
            utility("B", &["A"]),
            utility("C", &["B"]),
        ];
        assert!(Scheduler::new(build_steps(&p)).is_ok());
    }

    #[test]
    fn launching_is_held_by_the_phase_gate_not_by_dependencies() {
        // Listing every preflight step as a dependency would duplicate what the
        // gate already guarantees, and go stale the moment a step is added.
        let steps = build_steps(&profile());
        let launch = steps.iter().find(|s| s.phase == Phase::Launch).unwrap();
        assert!(launch.depends_on.is_empty());

        let mut scheduler = Scheduler::new(steps.clone()).unwrap();
        assert!(
            !scheduler.runnable().contains(&launch.id),
            "held until preflight passes"
        );
        for s in steps.iter().filter(|s| s.phase == Phase::Preflight) {
            scheduler.record(s.id, StepStatus::Passed);
        }
        assert!(scheduler.runnable().contains(&launch.id));
    }

    #[test]
    fn a_protocol_launch_names_no_executable() {
        assert_eq!(
            launch_exe(&LaunchMethod::Steam { app_id: "1".into() }),
            None
        );
        assert_eq!(
            launch_exe(&LaunchMethod::Executable {
                path: r"D:\Games\acs.exe".into(),
                args: vec![],
                working_dir: None
            }),
            Some("acs.exe".into())
        );
    }

    #[test]
    fn every_peripheral_row_offers_a_retry() {
        // A red row with no button is a dead end.
        let mut p = profile();
        p.peripherals = vec![PeripheralRequirement {
            device: device("Pedals"),
            necessity: Necessity::Required,
            expect_dinput_slot: None,
            expect_instance_guid: None,
            vendor_process: None,
            vjoy: None,
        }];
        let steps = build_steps(&p);
        assert!(steps
            .iter()
            .find(|s| s.label.contains("Pedals"))
            .unwrap()
            .fix
            .is_some());
    }
}
