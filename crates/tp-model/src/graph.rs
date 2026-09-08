//! The launch dependency graph.
//!
//! A profile is not a list of steps to run in order. It is a graph: peripheral
//! checks have nothing to do with SimHub starting, so they run at the same
//! time. A preflight that takes twenty seconds because it insists on being
//! sequential is a preflight people stop using.
//!
//! This module decides *what may run now*. Actually running it — threads,
//! processes, timeouts — sits on top, which means the scheduling rules that are
//! easy to get subtly wrong are tested here without spawning anything.
//!
//! ## The gate
//!
//! Every step declares a phase. No `Launch` step may begin until every
//! `Preflight` step has finished and none of the fatal ones failed. That
//! enforcement *is* the ready gate from the brief — it is not a separate
//! concept bolted on, it is a property of the graph.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Phase, ReadyState, Severity, StepId, StepSpec, StepStatus};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    #[error("step {0:?} is listed twice")]
    DuplicateStep(StepId),
    #[error("step {from:?} depends on {to:?}, which does not exist")]
    MissingDependency { from: StepId, to: StepId },
    #[error("these steps depend on each other in a loop: {0:?}")]
    Cycle(Vec<StepId>),
    #[error("step {step:?} is in the launch phase but depends on {dep:?}, which is teardown")]
    BackwardsPhase { step: StepId, dep: StepId },
}

/// Tracks which steps have run and decides what may run next.
#[derive(Debug, Clone)]
pub struct Scheduler {
    steps: Vec<StepSpec>,
    status: HashMap<StepId, StepStatus>,
}

impl Scheduler {
    /// Validate the graph up front.
    ///
    /// A cycle or a missing dependency is a broken profile, and finding out by
    /// watching a preflight hang forever is the worst possible way to learn it.
    pub fn new(steps: Vec<StepSpec>) -> Result<Self, GraphError> {
        let mut seen = HashSet::new();
        for s in &steps {
            if !seen.insert(s.id) {
                return Err(GraphError::DuplicateStep(s.id));
            }
        }
        for s in &steps {
            for dep in &s.depends_on {
                let Some(target) = steps.iter().find(|x| x.id == *dep) else {
                    return Err(GraphError::MissingDependency {
                        from: s.id,
                        to: *dep,
                    });
                };
                // Depending on a later phase can never be satisfied, because
                // the gate holds the later phase until this one finishes.
                if phase_order(target.phase) > phase_order(s.phase) {
                    return Err(GraphError::BackwardsPhase {
                        step: s.id,
                        dep: *dep,
                    });
                }
            }
        }
        detect_cycle(&steps)?;

        let status = steps.iter().map(|s| (s.id, StepStatus::Pending)).collect();
        Ok(Self { steps, status })
    }

    pub fn steps(&self) -> &[StepSpec] {
        &self.steps
    }

    pub fn status_of(&self, id: StepId) -> StepStatus {
        self.status.get(&id).copied().unwrap_or(StepStatus::Pending)
    }

    /// Every step that could start right now, in any order.
    ///
    /// The caller may run all of them at once; that is the point.
    pub fn runnable(&self) -> Vec<StepId> {
        if !self.phase_open(Phase::Preflight) && !self.phase_open(Phase::Launch) {
            return Vec::new();
        }
        self.steps
            .iter()
            .filter(|s| self.status_of(s.id) == StepStatus::Pending)
            .filter(|s| self.phase_open(s.phase))
            .filter(|s| s.depends_on.iter().all(|d| self.dependency_satisfied(*d)))
            .map(|s| s.id)
            .collect()
    }

    /// Is a dependency done in a way that lets its dependents proceed?
    ///
    /// A step that *failed* still satisfies its dependents when it was only a
    /// warning — that is what declaring it non-fatal means. Treating any
    /// failure as blocking would make Severity::Warning decorative.
    fn dependency_satisfied(&self, id: StepId) -> bool {
        match self.status_of(id) {
            StepStatus::Passed | StepStatus::Warning | StepStatus::Skipped => true,
            StepStatus::Failed => self
                .steps
                .iter()
                .find(|s| s.id == id)
                .is_some_and(|s| s.severity == Severity::Warning),
            StepStatus::Pending | StepStatus::Running => false,
        }
    }

    /// Record an outcome, and skip anything that can no longer usefully run.
    ///
    /// A dependent of a failed *fatal* step is skipped rather than failed: it
    /// did not fail, it never got the chance, and telling the user four things
    /// broke when one did is noise.
    pub fn record(&mut self, id: StepId, status: StepStatus) {
        self.status.insert(id, status);
        if status != StepStatus::Failed {
            return;
        }
        let Some(step) = self.steps.iter().find(|s| s.id == id) else {
            return;
        };
        if step.severity != Severity::Fatal {
            return;
        }
        self.skip_dependents_of(id);
    }

    fn skip_dependents_of(&mut self, failed: StepId) {
        // Transitive: a step two hops downstream is just as unable to run.
        let mut blocked: HashSet<StepId> = HashSet::from([failed]);
        loop {
            let newly: Vec<StepId> = self
                .steps
                .iter()
                .filter(|s| self.status_of(s.id) == StepStatus::Pending)
                .filter(|s| !blocked.contains(&s.id))
                .filter(|s| s.depends_on.iter().any(|d| blocked.contains(d)))
                .map(|s| s.id)
                .collect();
            if newly.is_empty() {
                break;
            }
            for id in newly {
                self.status.insert(id, StepStatus::Skipped);
                blocked.insert(id);
            }
        }
    }

    /// Reset one step and everything downstream of it, so a retry re-runs what
    /// it has to and nothing more.
    ///
    /// Restarting SimHub because a pedal check failed is exactly the behaviour
    /// that makes people stop using a preflight.
    pub fn reset_from(&mut self, id: StepId) {
        let mut affected: HashSet<StepId> = HashSet::from([id]);
        loop {
            let newly: Vec<StepId> = self
                .steps
                .iter()
                .filter(|s| !affected.contains(&s.id))
                .filter(|s| s.depends_on.iter().any(|d| affected.contains(d)))
                .map(|s| s.id)
                .collect();
            if newly.is_empty() {
                break;
            }
            affected.extend(newly);
        }
        for id in affected {
            self.status.insert(id, StepStatus::Pending);
        }
    }

    /// Has every step in a phase reached a terminal state?
    pub fn phase_settled(&self, phase: Phase) -> bool {
        self.steps
            .iter()
            .filter(|s| s.phase == phase)
            .all(|s| is_terminal(self.status_of(s.id)))
    }

    /// May steps in this phase run?
    fn phase_open(&self, phase: Phase) -> bool {
        match phase {
            Phase::Preflight => true,
            // The gate. Launch waits for preflight to finish *and* pass.
            Phase::Launch => {
                self.phase_settled(Phase::Preflight) && !self.has_fatal_failure(Phase::Preflight)
            }
            // Teardown is driven explicitly, never scheduled alongside the rest.
            Phase::Teardown => false,
        }
    }

    pub fn has_fatal_failure(&self, phase: Phase) -> bool {
        self.steps.iter().any(|s| {
            s.phase == phase
                && s.severity == Severity::Fatal
                && matches!(self.status_of(s.id), StepStatus::Failed)
        })
    }

    /// What the ready panel should say.
    pub fn ready_state(&self) -> ReadyState {
        if !self.phase_settled(Phase::Preflight) {
            return ReadyState::Running;
        }
        if self.has_fatal_failure(Phase::Preflight) {
            return ReadyState::Blocked;
        }
        let warned = self.steps.iter().any(|s| {
            s.phase == Phase::Preflight
                && matches!(
                    self.status_of(s.id),
                    StepStatus::Warning | StepStatus::Failed | StepStatus::Skipped
                )
        });
        if warned {
            ReadyState::ReadyWithWarnings
        } else {
            ReadyState::Ready
        }
    }

    /// Nothing left that could run.
    pub fn is_complete(&self) -> bool {
        self.runnable().is_empty()
            && self
                .steps
                .iter()
                .all(|s| s.phase == Phase::Teardown || is_terminal(self.status_of(s.id)))
    }
}

pub fn is_terminal(status: StepStatus) -> bool {
    matches!(
        status,
        StepStatus::Passed | StepStatus::Warning | StepStatus::Failed | StepStatus::Skipped
    )
}

fn phase_order(phase: Phase) -> u8 {
    match phase {
        Phase::Preflight => 0,
        Phase::Launch => 1,
        Phase::Teardown => 2,
    }
}

/// Depth-first cycle detection, reporting the loop rather than just its
/// existence — "these three steps depend on each other" is fixable, "your graph
/// has a cycle" is not.
fn detect_cycle(steps: &[StepSpec]) -> Result<(), GraphError> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unvisited,
        InProgress,
        Done,
    }

    let mut marks: HashMap<StepId, Mark> = steps.iter().map(|s| (s.id, Mark::Unvisited)).collect();
    let deps: HashMap<StepId, Vec<StepId>> =
        steps.iter().map(|s| (s.id, s.depends_on.clone())).collect();

    fn visit(
        id: StepId,
        deps: &HashMap<StepId, Vec<StepId>>,
        marks: &mut HashMap<StepId, Mark>,
        stack: &mut Vec<StepId>,
    ) -> Result<(), GraphError> {
        match marks.get(&id).copied().unwrap_or(Mark::Unvisited) {
            Mark::Done => return Ok(()),
            Mark::InProgress => {
                // Report from where the loop closes, not the whole walk.
                let start = stack.iter().position(|x| *x == id).unwrap_or(0);
                return Err(GraphError::Cycle(stack[start..].to_vec()));
            }
            Mark::Unvisited => {}
        }
        marks.insert(id, Mark::InProgress);
        stack.push(id);
        for dep in deps.get(&id).into_iter().flatten() {
            visit(*dep, deps, marks, stack)?;
        }
        stack.pop();
        marks.insert(id, Mark::Done);
        Ok(())
    }

    for s in steps {
        visit(s.id, &deps, &mut marks, &mut Vec::new())?;
    }
    Ok(())
}

/// Wire shape for the UI: one row of the preflight checklist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    pub id: StepId,
    pub label: String,
    pub phase: Phase,
    pub severity: Severity,
    pub status: StepStatus,
    pub detail: String,
    /// f64 rather than u64: a u64 crosses into JavaScript as `bigint`, which
    /// cannot be divided by a number without a cast at every call site. Elapsed
    /// milliseconds never approach the precision limit.
    pub elapsed_ms: Option<f64>,
    /// What the app actually did, so the row cannot claim credit it has not
    /// earned — "Already running" is derived from this, never written by hand.
    pub action_taken: crate::ActionTaken,
    pub can_retry: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReadinessGate, StepAction};

    fn step(id: u32, phase: Phase, severity: Severity, deps: &[u32]) -> StepSpec {
        StepSpec {
            id: StepId(id),
            label: format!("step {id}"),
            phase,
            depends_on: deps.iter().map(|d| StepId(*d)).collect(),
            action: StepAction::CheckDisplayTopology,
            gate: ReadinessGate::Immediate,
            timeout_ms: 5_000,
            severity,
            fix: None,
            min_visible_ms: 350,
        }
    }

    fn preflight(id: u32, deps: &[u32]) -> StepSpec {
        step(id, Phase::Preflight, Severity::Fatal, deps)
    }

    fn ids(mut v: Vec<StepId>) -> Vec<u32> {
        v.sort();
        v.into_iter().map(|s| s.0).collect()
    }

    #[test]
    fn independent_steps_all_run_at_once() {
        // The whole reason this is a graph. Peripheral checks have nothing to
        // do with SimHub starting, and waiting for one before the other is how
        // a preflight becomes twenty seconds long.
        let s = Scheduler::new(vec![
            preflight(1, &[]),
            preflight(2, &[]),
            preflight(3, &[]),
        ])
        .unwrap();
        assert_eq!(ids(s.runnable()), vec![1, 2, 3]);
    }

    #[test]
    fn a_dependent_waits_for_what_it_needs() {
        let mut s = Scheduler::new(vec![preflight(1, &[]), preflight(2, &[1])]).unwrap();
        assert_eq!(ids(s.runnable()), vec![1]);
        s.record(StepId(1), StepStatus::Passed);
        assert_eq!(ids(s.runnable()), vec![2]);
    }

    #[test]
    fn a_warning_does_not_block_what_follows() {
        // A warning is not a failure. If it stopped the graph it would be one.
        let mut s = Scheduler::new(vec![
            step(1, Phase::Preflight, Severity::Warning, &[]),
            preflight(2, &[1]),
        ])
        .unwrap();
        s.record(StepId(1), StepStatus::Warning);
        assert_eq!(ids(s.runnable()), vec![2]);
    }

    #[test]
    fn a_fatal_failure_skips_its_dependents_rather_than_failing_them() {
        // Four red rows for one cause is noise. They did not fail; they never
        // got the chance.
        let mut s = Scheduler::new(vec![
            preflight(1, &[]),
            preflight(2, &[1]),
            preflight(3, &[2]),
        ])
        .unwrap();
        s.record(StepId(1), StepStatus::Failed);
        assert_eq!(s.status_of(StepId(2)), StepStatus::Skipped);
        assert_eq!(s.status_of(StepId(3)), StepStatus::Skipped, "transitively");
        assert!(s.runnable().is_empty());
    }

    #[test]
    fn a_non_fatal_failure_lets_the_rest_carry_on() {
        let mut s = Scheduler::new(vec![
            step(1, Phase::Preflight, Severity::Warning, &[]),
            preflight(2, &[1]),
        ])
        .unwrap();
        s.record(StepId(1), StepStatus::Failed);
        assert_eq!(s.status_of(StepId(2)), StepStatus::Pending);
        assert_eq!(ids(s.runnable()), vec![2]);
    }

    // ----------------------------------------------------------- the gate

    #[test]
    fn launch_waits_for_every_preflight_step() {
        let mut s = Scheduler::new(vec![
            preflight(1, &[]),
            preflight(2, &[]),
            step(10, Phase::Launch, Severity::Fatal, &[]),
        ])
        .unwrap();

        assert_eq!(ids(s.runnable()), vec![1, 2], "launch is held back");
        s.record(StepId(1), StepStatus::Passed);
        assert_eq!(
            ids(s.runnable()),
            vec![2],
            "still held: one preflight step is outstanding"
        );
        s.record(StepId(2), StepStatus::Passed);
        assert_eq!(ids(s.runnable()), vec![10], "gate opens");
    }

    #[test]
    fn a_failed_preflight_never_opens_the_gate() {
        let mut s = Scheduler::new(vec![
            preflight(1, &[]),
            step(10, Phase::Launch, Severity::Fatal, &[]),
        ])
        .unwrap();
        s.record(StepId(1), StepStatus::Failed);
        assert!(s.runnable().is_empty(), "the game must not launch");
        assert_eq!(s.ready_state(), ReadyState::Blocked);
    }

    #[test]
    fn warnings_open_the_gate_but_say_so() {
        let mut s = Scheduler::new(vec![
            step(1, Phase::Preflight, Severity::Warning, &[]),
            step(10, Phase::Launch, Severity::Fatal, &[]),
        ])
        .unwrap();
        s.record(StepId(1), StepStatus::Warning);
        assert_eq!(s.ready_state(), ReadyState::ReadyWithWarnings);
        assert_eq!(ids(s.runnable()), vec![10]);
    }

    #[test]
    fn everything_passing_is_simply_ready() {
        let mut s = Scheduler::new(vec![preflight(1, &[]), preflight(2, &[])]).unwrap();
        s.record(StepId(1), StepStatus::Passed);
        assert_eq!(
            s.ready_state(),
            ReadyState::Running,
            "not while one is outstanding"
        );
        s.record(StepId(2), StepStatus::Passed);
        assert_eq!(s.ready_state(), ReadyState::Ready);
    }

    #[test]
    fn teardown_never_schedules_itself() {
        // It runs on exit, driven explicitly — not alongside the launch.
        let s = Scheduler::new(vec![step(1, Phase::Teardown, Severity::Fatal, &[])]).unwrap();
        assert!(s.runnable().is_empty());
    }

    // ------------------------------------------------------------- retry

    #[test]
    fn a_retry_reruns_only_what_it_has_to() {
        // Restarting SimHub because a pedal check failed is exactly what makes
        // people stop using a preflight.
        let mut s = Scheduler::new(vec![
            preflight(1, &[]),
            preflight(2, &[]),
            preflight(3, &[2]),
        ])
        .unwrap();
        s.record(StepId(1), StepStatus::Passed);
        s.record(StepId(2), StepStatus::Failed);

        s.reset_from(StepId(2));
        assert_eq!(s.status_of(StepId(1)), StepStatus::Passed, "untouched");
        assert_eq!(s.status_of(StepId(2)), StepStatus::Pending);
        assert_eq!(
            s.status_of(StepId(3)),
            StepStatus::Pending,
            "downstream comes back too"
        );
    }

    // ------------------------------------------------------- broken graphs

    #[test]
    fn a_cycle_is_refused_and_named() {
        // Discovering this by watching a preflight hang forever would be the
        // worst possible way to learn it.
        let err = Scheduler::new(vec![preflight(1, &[2]), preflight(2, &[1])]).unwrap_err();
        match err {
            GraphError::Cycle(loop_) => assert_eq!(loop_.len(), 2, "{loop_:?}"),
            other => panic!("expected a cycle, got {other:?}"),
        }
    }

    #[test]
    fn a_self_dependency_is_a_cycle() {
        assert!(matches!(
            Scheduler::new(vec![preflight(1, &[1])]).unwrap_err(),
            GraphError::Cycle(_)
        ));
    }

    #[test]
    fn a_missing_dependency_is_refused() {
        assert_eq!(
            Scheduler::new(vec![preflight(1, &[99])]).unwrap_err(),
            GraphError::MissingDependency {
                from: StepId(1),
                to: StepId(99)
            }
        );
    }

    #[test]
    fn duplicate_ids_are_refused() {
        assert_eq!(
            Scheduler::new(vec![preflight(1, &[]), preflight(1, &[])]).unwrap_err(),
            GraphError::DuplicateStep(StepId(1))
        );
    }

    #[test]
    fn a_preflight_step_cannot_depend_on_a_launch_step() {
        // The gate holds launch until preflight finishes, so such a dependency
        // could never be satisfied — it would hang, silently, forever.
        assert_eq!(
            Scheduler::new(vec![
                step(1, Phase::Preflight, Severity::Fatal, &[10]),
                step(10, Phase::Launch, Severity::Fatal, &[]),
            ])
            .unwrap_err(),
            GraphError::BackwardsPhase {
                step: StepId(1),
                dep: StepId(10)
            }
        );
    }

    #[test]
    fn a_diamond_runs_its_middle_in_parallel_and_joins() {
        let mut s = Scheduler::new(vec![
            preflight(1, &[]),
            preflight(2, &[1]),
            preflight(3, &[1]),
            preflight(4, &[2, 3]),
        ])
        .unwrap();
        s.record(StepId(1), StepStatus::Passed);
        assert_eq!(ids(s.runnable()), vec![2, 3], "both middles at once");
        s.record(StepId(2), StepStatus::Passed);
        assert_eq!(
            ids(s.runnable()),
            vec![3],
            "the join waits; 3 has still to run"
        );
        s.record(StepId(3), StepStatus::Passed);
        assert_eq!(ids(s.runnable()), vec![4]);
        s.record(StepId(4), StepStatus::Passed);
        assert!(s.is_complete());
    }
}
