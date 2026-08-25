//! The run: one worker contract's execution, and exactly one record entry.
//!
//! `05 run state model` found that the five states arriving from `07 attach semantics` and `18 hang detection prior art` were never one
//! enum. They are three independent axes, and the third is never stored.

use serde::{Deserialize, Serialize};
use std::ops::Deref;

use crate::ids::{CellId, RunId, TaskId};
use crate::policy::{Budget, Irreversibility};

/// Owned by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "outcome")]
pub enum Lifecycle {
    Queued,
    Running,
    Finished(Outcome),
}

/// How a run ended. Four values, and the distinctions are load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    /// Something broke, and a retry is reasonable. Budget exhaustion lands here
    /// per `05 run state model`, because nobody chose it.
    Failed,
    /// A **human** decided not to. `05 run state model`: conflating this with `Failed` produces
    /// an auto-retry loop that fights the operator.
    Cancelled,
    /// The **manager** decided the run was unnecessary before it started, per
    /// `23 prototype loose ends`. Nothing broke, so a retry should not happen.
    Abandoned,
}

impl Outcome {
    /// Whether a manager should consider re-running. Only `Failed` invites one.
    pub fn invites_retry(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// Owned by whoever is attached, per `07 attach semantics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    #[default]
    Autonomous,
    /// Watching is passive and the agent is still driving.
    Observed,
    TakenOver,
}

/// Derived, never written. `05 run state model`: storing it would create two sources of truth
/// that can disagree, and a crash mid-transition would leave a run permanently
/// marked `stalled` when it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    Live,
    Stalled,
    LikelyHung,
}

/// The two numbers from `05 run state model`. Both configurable, and **neither kills anything**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LivenessThresholds {
    pub stalled_secs: u64,
    pub likely_hung_secs: u64,
}

impl Default for LivenessThresholds {
    fn default() -> Self {
        Self {
            stalled_secs: 120,
            likely_hung_secs: 600,
        }
    }
}

/// Why the liveness clock is currently paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    /// Control is not `autonomous`. `05 run state model`: a human thinking for three minutes is
    /// not a hang, and flagging it would be farseer blaming the operator for the
    /// operator's own pause.
    OperatorHasTheWheel,
    /// A known context compaction. Amended into `05 run state model` on 2026-08-23: a compaction
    /// is silence, which is exactly the shape `stalled` is built to catch, and it
    /// lands on the longest and most expensive runs.
    Compacting,
}

/// Tracks silence so liveness can be computed rather than stored.
///
/// The watchdog keys on **activity** - any bytes from the adapter - not on
/// **progress**. `05 run state model`: a high-end model reasoning for twenty minutes emits no
/// tool calls, and under the progress definition it would be flagged
/// `likely-hung` while working perfectly. Mechanical silence is a hang;
/// thinking hard is not.
///
/// Time is supplied by the caller as monotonic seconds, so this stays pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityClock {
    silent_since: u64,
    paused: Option<Pause>,
}

impl ActivityClock {
    pub fn started_at(now: u64) -> Self {
        Self {
            silent_since: now,
            paused: None,
        }
    }

    /// Any byte from the adapter, or an explicit adapter heartbeat.
    pub fn observe_activity(&mut self, now: u64) {
        self.silent_since = now;
    }

    pub fn pause(&mut self, reason: Pause) {
        self.paused = Some(reason);
    }

    /// Resuming discards the silence accumulated while paused, which is the
    /// point: that silence was not the worker's.
    pub fn resume(&mut self, now: u64) {
        self.paused = None;
        self.silent_since = now;
    }

    pub fn paused(&self) -> Option<Pause> {
        self.paused
    }

    pub fn liveness(&self, now: u64, thresholds: &LivenessThresholds) -> Liveness {
        if self.paused.is_some() {
            return Liveness::Live;
        }
        let silent = now.saturating_sub(self.silent_since);
        if silent >= thresholds.likely_hung_secs {
            Liveness::LikelyHung
        } else if silent >= thresholds.stalled_secs {
            Liveness::Stalled
        } else {
            Liveness::Live
        }
    }
}

/// Where a run's files live. `08 generalization test` proved this is a policy value rather than a
/// git flag, so `PlainDirectory` is one of the strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStrategy {
    /// `04 spike workspace teardown`'s default: a fresh git worktree per run. Two of the three runners in
    /// `10 runner inventory` refuse a directory that is not a repo.
    Worktree,
    PlainDirectory,
}

/// A workspace's own small state, per `05 run state model` section 8.
///
/// A run whose work is done must not stay open because of a directory, so this
/// belongs to the cell rather than the run, and retry belongs to the startup
/// sweep rather than to the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Live,
    /// Teardown failed. `04 spike workspace teardown` found the quarantine-by-rename fallback cannot
    /// work, so this is a state the operator is shown, not one to paper over.
    Orphaned,
}

/// The fields of a worker contract, before sealing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerContractSpec {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub cell_id: CellId,
    pub goal: String,
    pub workspace: WorkspaceStrategy,
    /// Which runner from the inventory executes this. `10 runner inventory` made the inventory a
    /// menu an author picks from.
    pub runner: String,
    /// The allowlist. `12 autonomy and deny list`: the only real isolation v1 has.
    pub tool_grants: Vec<String>,
    pub autonomy_ceiling: Irreversibility,
    pub budget: Budget,
    /// `05 run state model` put the gate here; `12 autonomy and deny list` made satisfying it a tool grant rather than
    /// a runtime concept, so this is prose the cell's own tools check.
    pub definition_of_done: String,
}

/// What a manager gives a worker. **Immutable for the life of the run.**
///
/// `05 run state model`: immutability is what makes the record answerable after the fact.
/// "What was this worker allowed to do" has one answer, not a timeline of them.
/// `07 attach semantics` established that intervention does not void the contract, which only
/// holds if the contract cannot drift.
///
/// Steering moves **within** the worker contract. Changing the contract is a new run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerContract(WorkerContractSpec);

impl WorkerContract {
    pub fn seal(spec: WorkerContractSpec) -> Self {
        Self(spec)
    }
}

impl Deref for WorkerContract {
    type Target = WorkerContractSpec;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The manager's four verbs, per `05 run state model` as corrected by `20 worker control channel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerVerb {
    /// Same run, same contract, new instruction, **delivered at the next turn
    /// boundary**. `20 worker control channel` surveyed the channels and found no harness supports
    /// interrupting a turn already in flight, so farseer must not promise it.
    Steer,
    /// New run against the same task, because a contract field changed.
    ReScope,
    Cancel,
    /// New run, fresh workspace.
    ReRun,
}

impl ManagerVerb {
    /// Whether the operator calling this directly leaves a mark.
    ///
    /// `16 local api surface` section 8: re-scope and re-run are normally manager decisions, and a
    /// manager that silently discovers its plan changed underneath it will
    /// re-plan badly. Do not restrict the human, do record that it was the human.
    pub fn operator_call_is_recorded(self) -> bool {
        matches!(self, Self::ReScope | Self::ReRun)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: LivenessThresholds = LivenessThresholds {
        stalled_secs: 120,
        likely_hung_secs: 600,
    };

    #[test]
    fn cancelled_is_never_failed_and_only_failed_invites_a_retry() {
        assert!(Outcome::Failed.invites_retry());
        assert!(!Outcome::Cancelled.invites_retry());
        assert!(!Outcome::Abandoned.invites_retry());
        assert!(!Outcome::Ok.invites_retry());
    }

    #[test]
    fn silence_crosses_both_thresholds_in_order() {
        let clock = ActivityClock::started_at(0);
        assert_eq!(clock.liveness(119, &T), Liveness::Live);
        assert_eq!(clock.liveness(120, &T), Liveness::Stalled);
        assert_eq!(clock.liveness(599, &T), Liveness::Stalled);
        assert_eq!(clock.liveness(600, &T), Liveness::LikelyHung);
    }

    #[test]
    fn a_model_thinking_for_twenty_minutes_stays_live_if_it_streams() {
        let mut clock = ActivityClock::started_at(0);
        for minute in 1..=20 {
            clock.observe_activity(minute * 60);
            assert_eq!(clock.liveness(minute * 60 + 1, &T), Liveness::Live);
        }
    }

    #[test]
    fn the_clock_pauses_while_the_operator_has_the_wheel() {
        let mut clock = ActivityClock::started_at(0);
        clock.pause(Pause::OperatorHasTheWheel);
        assert_eq!(clock.liveness(10_000, &T), Liveness::Live);
    }

    #[test]
    fn a_long_compaction_does_not_trip_a_false_stall() {
        let mut clock = ActivityClock::started_at(0);
        clock.pause(Pause::Compacting);
        assert_eq!(clock.liveness(900, &T), Liveness::Live);
        clock.resume(900);
        assert_eq!(clock.liveness(901, &T), Liveness::Live);
    }

    #[test]
    fn observing_does_not_pause_the_clock() {
        // `05 run state model`: watching is passive, the agent is still driving, and that is
        // precisely when the watchdog should be live.
        let clock = ActivityClock::started_at(0);
        assert_eq!(clock.liveness(700, &T), Liveness::LikelyHung);
    }

    #[test]
    fn resuming_forgets_the_silence_that_was_not_the_workers() {
        let mut clock = ActivityClock::started_at(0);
        clock.pause(Pause::OperatorHasTheWheel);
        clock.resume(5_000);
        assert_eq!(clock.liveness(5_119, &T), Liveness::Live);
        assert_eq!(clock.liveness(5_120, &T), Liveness::Stalled);
    }

    #[test]
    fn a_sealed_contract_exposes_its_fields_for_reading_only() {
        let contract = WorkerContract::seal(WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "ship it".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "claude-code".into(),
            tool_grants: vec!["shell".into()],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "cargo test passes".into(),
        });
        assert_eq!(contract.goal, "ship it");
        assert_eq!(contract.tool_grants, ["shell"]);
    }

    #[test]
    fn operator_rescope_and_rerun_leave_a_mark_but_steer_and_cancel_do_not() {
        assert!(ManagerVerb::ReScope.operator_call_is_recorded());
        assert!(ManagerVerb::ReRun.operator_call_is_recorded());
        assert!(!ManagerVerb::Steer.operator_call_is_recorded());
        assert!(!ManagerVerb::Cancel.operator_call_is_recorded());
    }
}
