//! Executes one sealed [`WorkerContract`] against a native runner, with the run row and progress events landing in the [`Store`].
//!
//! The manager's delegation decision remains in the live LLM conversation rather than this crate.
//! `farseer-api` exposes `delegate_to_worker` through MCP, then calls this same synchronous engine for the selected roster worker.
//! The four verbs from [Run state model and control semantics] also call this engine.
//!
//! [Run state model and control semantics]: ../../../.scratch/farseer/issues/05-run-state-model.md
//!
//! **The watchdog only reports.**
//! [Hang detection prior art] and [Run state model and control semantics] make `120s` stalled, `600s` likely-hung, and no auto-kill.
//! [`LivenessHandle`] remains queryable while [`StartedWorker::run_to_completion`] blocks on another thread, and only the API's explicit cancel verb closes the job.
//!
//! **`CancelToken::cancel` produces `Cancelled`, not `Failed`.**
//! Closing the job can happen before any terminal result or after a live stream-json session emitted a successful earlier turn and stayed open for steering.
//! [`CancelToken::was_cancelled`] is authoritative in both cases because every clone shares one flag.
//! A terminal report observed before cancellation keeps its cost, tokens, and result while its outcome becomes `Cancelled`.
//! Cancellation before any terminal result carries unknown report values and records zero usage.
//! This keeps the record honest about a human choosing not to proceed rather than something breaking.
//!
//! [Hang detection prior art]: ../../../.scratch/farseer/issues/18-hang-detection-prior-art.md
//!
//! Windows only, like the runner it drives - `farseer-runner`'s `spawn` and
//! `drive` modules are themselves `cfg(windows)`, so this crate would fail to
//! compile anywhere else regardless; the gate below makes that an intentional
//! boundary rather than an accidental one.
//!
//! **This crate never holds `Store` itself.** [`run_worker`] runs for as long
//! as the process it spawns, blocked on I/O the whole time; a caller sharing
//! one `Store` behind a mutex across a whole API (`farseer-api` does exactly
//! this) cannot afford to have that mutex held for a run's entire duration -
//! every other request, including reading the very events this run is
//! writing, would queue behind it. [`RunSink`] is the seam: a caller that
//! owns locking policy implements it however it likes - `farseer-api` locks
//! and releases per call - while a test can just hand over a bare `Store`,
//! since `Store` implements it directly.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use farseer_core::run::{ActivityClock, Liveness, LivenessThresholds, WorkerContract};
use farseer_core::{Actor, CellDefinition, EventKind, NewEvent, Outcome, Seq};
use farseer_runner::claude_code::{ParseError, RunnerSignal};
use farseer_runner::drive::drive;
use farseer_runner::resolve::resolve;
use farseer_runner::spawn::{CancelToken, SpawnError, StdinHandle, SupervisedProcess};
use farseer_store::{RunRow, Store, StoreError};

/// Where a run's events and row land. One method per write `run_worker`
/// makes - never a whole `Store`, so the caller decides how (or whether) to
/// lock around each one.
pub trait RunSink {
    fn append(&self, event: &NewEvent) -> Result<Seq, StoreError>;
    fn upsert_run(&self, row: &RunRow) -> Result<(), StoreError>;
}

impl RunSink for Store {
    fn append(&self, event: &NewEvent) -> Result<Seq, StoreError> {
        Store::append(self, event)
    }

    fn upsert_run(&self, row: &RunRow) -> Result<(), StoreError> {
        Store::upsert_run(self, row)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("runner `{0}` has no native implementation yet")]
    UnsupportedRunner(String),
    #[error("`{0}` is not on PATH")]
    ExecutableNotFound(String),
    #[error(transparent)]
    Spawn(#[from] SpawnError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the process exited without ever emitting a terminal result")]
    NoResult,
    /// [Run state model and control semantics] says explicit cancellation is never `Failed`.
    /// The report preserves any terminal cost, tokens, and result observed before cancellation.
    #[error("the run was cancelled")]
    Cancelled(RunReport),
}

#[derive(Debug)]
pub struct RunReport {
    pub outcome: Outcome,
    pub cost_usd_micros: Option<i64>,
    pub tokens: Option<i64>,
    /// User-visible terminal text for a supervising manager to relay.
    pub result: Option<String>,
    /// The last window the runner reported during this run, per `27 quota
    /// accounting`. The manager only carries it out: resolving which **account**
    /// it belongs to needs runner config, which is machine-wide and none of a
    /// run's business.
    ///
    /// Boxed so a rarely-present observation does not widen every `Result` this
    /// crate returns - `ManagerError::Cancelled` carries a whole report.
    pub window: Option<Box<farseer_runner::claude_code::RateLimitInfo>>,
}

/// Which role the process has in the cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunRole {
    Manager,
    #[default]
    Worker,
}

pub const RUN_ROLE_FIELD: &str = "_farseer_role";
pub const MANAGER_CELL_FIELD: &str = "_farseer_manager_cell";

impl RunRole {
    pub fn as_record_str(self) -> &'static str {
        match self {
            Self::Manager => "manager",
            Self::Worker => "worker",
        }
    }

    pub fn from_record_str(value: &str) -> Option<Self> {
        match value {
            "manager" => Some(Self::Manager),
            "worker" => Some(Self::Worker),
            _ => None,
        }
    }
}

/// Process launch facts which are not fields of the immutable worker contract.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Who caused this run to be queued in farseer's record.
    pub actor: Actor,
    pub role: RunRole,
    /// The run's pinned cell definition snapshot, never a live reload lookup.
    /// Managers use it to build their runtime context; delegated workers retain
    /// it so an operator rerun can reapply the same authority and worker cap.
    pub manager_cell: Option<CellDefinition>,
    /// Manager-only Claude Code MCP config generated after the API binds.
    pub claude_mcp_config: Option<PathBuf>,
    /// Manager identity and roster guidance, separate from the operator's goal.
    pub claude_append_system_prompt: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            actor: Actor::Operator,
            role: RunRole::Worker,
            manager_cell: None,
            claude_mcp_config: None,
            claude_append_system_prompt: None,
        }
    }
}

/// A worker process, spawned and waiting to be driven.
///
/// Split from spawning-and-driving in one call so a caller holds a
/// [`CancelToken`] *before* the blocking read loop starts: the token has to
/// exist before it might be needed, and `run_to_completion` takes `self` by
/// value precisely so it cannot be called twice on the same process.
pub struct StartedWorker {
    proc: SupervisedProcess,
    activity: Arc<Mutex<ActivityClock>>,
    monotonic_start: Instant,
    thresholds: LivenessThresholds,
    /// Which runner's stream-json dialect this process speaks - a plain
    /// function pointer, not a trait, since every runner's `parse_line` is
    /// already exactly this shape.
    parse: fn(&str) -> Result<Vec<RunnerSignal>, ParseError>,
    /// `Some` for a runner steer can actually reach - today only
    /// `claude_code::steer_frame`, per `invocation.rs`'s doc comment.
    /// `None` for a runner like Codex, whose own steering path does not
    /// exist (`codex exec resume` starts a new process, per `10 runner inventory`). Also the
    /// initial message: [`Self::bootstrap`] writes `contract.goal` through
    /// it as the first stdin line, since `--input-format stream-json`
    /// expects the goal there rather than as argv.
    steer_frame: Option<fn(&str) -> String>,
}

/// A cloneable handle that writes a steer message into a run's live process,
/// verified 2026-08-23 against `claude_code::steer_frame`'s wire shape.
/// `None` from [`StartedWorker::steer_handle`] rather than an instance of
/// this means the runner has no steering path at all - refuse the request
/// rather than writing a line nothing reads.
#[derive(Clone)]
pub struct SteerHandle {
    stdin: StdinHandle,
    frame: fn(&str) -> String,
}

impl SteerHandle {
    pub fn steer(&self, message: &str) -> std::io::Result<()> {
        self.stdin.write_line(&(self.frame)(message))
    }
}

/// A cloneable, read-only view onto a run's liveness - `18 hang detection prior art`/`05 run state model`'s watchdog,
/// minus the kill. Queryable from any thread: the clock behind it is shared
/// with whichever thread is inside [`StartedWorker::run_to_completion`], so a
/// caller does not need to wait for that call to return to ask how it's
/// doing.
#[derive(Clone)]
pub struct LivenessHandle {
    activity: Arc<Mutex<ActivityClock>>,
    monotonic_start: Instant,
    thresholds: LivenessThresholds,
}

impl LivenessHandle {
    pub fn liveness(&self) -> Liveness {
        let now = self.monotonic_start.elapsed().as_secs();
        self.activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .liveness(now, &self.thresholds)
    }
}

impl StartedWorker {
    /// `thresholds` per `05 run state model`: "both configurable, and neither kills
    /// anything" - the second half is enforced by this crate never calling
    /// [`CancelToken`] on the watchdog's own initiative, not by anything in
    /// the type here.
    pub fn spawn(
        exe: &Path,
        args: &[String],
        cwd: &Path,
        thresholds: LivenessThresholds,
        parse: fn(&str) -> Result<Vec<RunnerSignal>, ParseError>,
        steer_frame: Option<fn(&str) -> String>,
    ) -> Result<Self, ManagerError> {
        Ok(Self {
            proc: SupervisedProcess::spawn(exe, args, cwd)?,
            activity: Arc::new(Mutex::new(ActivityClock::started_at(0))),
            monotonic_start: Instant::now(),
            thresholds,
            parse,
            steer_frame,
        })
    }

    pub fn cancel_token(&self) -> CancelToken {
        self.proc.cancel_token()
    }

    /// Fetch before calling the blocking `run_to_completion`, same reason as
    /// [`Self::cancel_token`]. `None` when this runner has no steering path.
    pub fn steer_handle(&self) -> Option<SteerHandle> {
        self.steer_frame.map(|frame| SteerHandle {
            stdin: self.proc.stdin_handle(),
            frame,
        })
    }

    /// Fetch before calling the blocking `run_to_completion` - the handle
    /// has to exist before it might be queried, and it keeps working after
    /// `run_to_completion` consumes `self`, since the clock behind it is
    /// shared rather than owned.
    pub fn liveness_handle(&self) -> LivenessHandle {
        LivenessHandle {
            activity: Arc::clone(&self.activity),
            monotonic_start: self.monotonic_start,
            thresholds: self.thresholds,
        }
    }

    /// Writes `goal` as the first stdin line for a runner with a steer
    /// frame - `--input-format stream-json` (per `invocation.rs`) expects
    /// it there rather than as argv. **Must complete before this worker's
    /// [`SteerHandle`] reaches anywhere a caller could act on it**: both
    /// write to the same stdin pipe, and a steer message that lands first
    /// would reach Claude Code before the goal ever does. [`start_worker`]
    /// calls this itself, before `run_worker`'s `on_started` callback - the
    /// thing that makes the handle reachable at all - ever fires; a caller
    /// building a [`StartedWorker`] directly and skipping this is exercising
    /// something other than the real dispatch path.
    pub fn bootstrap(&self, goal: &str) -> Result<(), ManagerError> {
        if let Some(frame) = self.steer_frame {
            self.proc.write_line(&frame(goal))?;
        }
        Ok(())
    }

    /// Blocks until the process closes stdout. Every progress signal becomes
    /// a `NewEvent` appended to `store`, in the order it arrived. A line
    /// `farseer_runner::claude_code::parse_line` cannot parse is `05 run state model`'s
    /// activity signal same as any other line - the process is still there -
    /// and is otherwise skipped, since there is nothing shaped enough to
    /// record.
    pub fn run_to_completion(
        mut self,
        sink: &impl RunSink,
        contract: &WorkerContract,
        progress_actor: Actor,
        mut now_ms: impl FnMut() -> i64,
    ) -> Result<RunReport, ManagerError> {
        let mut report = None;
        let mut output = None;
        let mut window = None;
        let mut store_err = None;
        let cancel_on_store_failure = self.proc.cancel_token();
        let activity = Arc::clone(&self.activity);
        let monotonic_start = self.monotonic_start;

        drive(&mut self.proc, self.parse, |parsed| {
            // `05 run state model`: any bytes at all is activity, parse failure or not - the
            // process is still there regardless of what the line meant.
            let now = monotonic_start.elapsed().as_secs();
            activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .observe_activity(now);

            if store_err.is_some() {
                // Already failing the record; let the cancel below end the
                // child rather than spending more of its run on output
                // nothing can keep.
                return;
            }
            let Ok(signals) = parsed else {
                return;
            };
            for signal in signals {
                match signal {
                    RunnerSignal::Progress { kind, payload } => {
                        let event = NewEvent::new(
                            contract.cell_id.clone(),
                            contract.run_id,
                            kind,
                            progress_actor,
                            now_ms(),
                            payload,
                        );
                        if let Err(e) = sink.append(&event) {
                            store_err = Some(e);
                            // The record can no longer be trusted for this
                            // run; there is no point paying for more of it.
                            cancel_on_store_failure.cancel();
                            return;
                        }
                    }
                    RunnerSignal::Output(text) => output = Some(text),
                    RunnerSignal::Finished(f) => {
                        report = Some(RunReport {
                            outcome: f.outcome,
                            cost_usd_micros: f.cost_usd_micros,
                            tokens: f.tokens,
                            result: output.clone(),
                            // Attached after the stream ends: `10 runner
                            // inventory` observed `rate_limit_event` arriving
                            // around the terminal result, not before it.
                            window: None,
                        });
                    }
                    // `27 quota accounting`: observed on every successful run,
                    // carried out on the report, and appended by the layer that
                    // knows which account it belongs to - on change only.
                    RunnerSignal::RateLimit(info) => window = Some(Box::new(info)),
                }
            }
        })?;

        if let Some(e) = store_err {
            return Err(e.into());
        }
        // A stream-json process can emit a terminal result for one turn and
        // remain alive waiting for a later steer message. If the operator then
        // closes that live session, the explicit cancellation is authoritative
        // for the run even though an earlier turn completed successfully.
        let was_cancelled = self.proc.cancel_token().was_cancelled();
        match report {
            Some(mut report) => {
                if report.result.is_none() {
                    report.result = output;
                }
                report.window = window;
                if was_cancelled {
                    report.outcome = Outcome::Cancelled;
                    Err(ManagerError::Cancelled(report))
                } else {
                    Ok(report)
                }
            }
            None if was_cancelled => Err(ManagerError::Cancelled(RunReport {
                outcome: Outcome::Cancelled,
                cost_usd_micros: None,
                tokens: None,
                result: None,
                window,
            })),
            None => Err(ManagerError::NoResult),
        }
    }
}

/// `contract.runner` selects one of the four verified native stream-json dialects: Claude Code, Codex, cursor-agent, or Goose.
/// The ACP runner from `20 worker control channel` remains unimplemented, so anything else is `UnsupportedRunner`.
///
/// A Claude Code manager bootstraps the goal onto live stdin before exposing its steer handle.
/// A Claude Code worker receives one positional goal and no live-input mode, so a synchronous delegation returns after one turn instead of waiting forever for steering.
pub fn start_worker(
    contract: &WorkerContract,
    cwd: &Path,
    thresholds: LivenessThresholds,
    options: &RunOptions,
) -> Result<StartedWorker, ManagerError> {
    let started = match contract.runner.as_str() {
        "claude-code" => {
            let exe = resolve("claude")
                .ok_or_else(|| ManagerError::ExecutableNotFound("claude".into()))?;
            StartedWorker::spawn(
                &exe,
                &farseer_runner::invocation::build_args(
                    contract,
                    farseer_runner::invocation::ClaudeCodeLaunch {
                        live_input: options.role == RunRole::Manager,
                        mcp_config: options.claude_mcp_config.as_deref(),
                        append_system_prompt: options.claude_append_system_prompt.as_deref(),
                    },
                ),
                cwd,
                thresholds,
                farseer_runner::claude_code::parse_line,
                (options.role == RunRole::Manager)
                    .then_some(farseer_runner::claude_code::steer_frame as fn(&str) -> String),
            )
        }
        "codex" => {
            let exe =
                resolve("codex").ok_or_else(|| ManagerError::ExecutableNotFound("codex".into()))?;
            StartedWorker::spawn(
                &exe,
                &farseer_runner::codex::build_args(contract),
                cwd,
                thresholds,
                farseer_runner::codex::parse_line,
                // Codex has no steering path: `codex exec resume` starts a
                // new process rather than continuing this one, per `10 runner inventory`.
                None,
            )
        }
        "cursor-agent" => {
            let exe = resolve("cursor-agent")
                .ok_or_else(|| ManagerError::ExecutableNotFound("cursor-agent".into()))?;
            StartedWorker::spawn(
                &exe,
                &farseer_runner::cursor_agent::build_args(contract),
                cwd,
                thresholds,
                farseer_runner::cursor_agent::parse_line,
                // No `--input-format` flag exists on this CLI at all, per
                // `10 runner inventory` - `--resume`/`--continue` restart into a new process,
                // same shape as Codex.
                None,
            )
        }
        "goose" => {
            let exe =
                resolve("goose").ok_or_else(|| ManagerError::ExecutableNotFound("goose".into()))?;
            StartedWorker::spawn(
                &exe,
                &farseer_runner::goose::build_args(contract),
                cwd,
                thresholds,
                farseer_runner::goose::parse_line,
                // `-r/--resume` restarts into a new process rather than
                // continuing this one, and no `--input-format`-style flag
                // exists, per this crate's own 2026-08-24 probe.
                None,
            )
        }
        other => return Err(ManagerError::UnsupportedRunner(other.to_string())),
    }?;
    started.bootstrap(&contract.goal)?;
    Ok(started)
}

/// The whole run row lifecycle: `upsert_run` with no outcome at the start,
/// `upsert_run` again with the final one at the end - so a run that this
/// process crashes mid-flight is left `running` forever rather than silently
/// missing, matching `17 cell lifecycle`'s choice to surface an orphan rather than paper
/// over it.
///
/// `on_started` fires once, after the process spawns but before the blocking
/// read loop starts, with a [`CancelToken`] and [`LivenessHandle`] a caller
/// can stash somewhere reachable from another thread - a registry an HTTP
/// handler looks runs up in, for instance. It never fires if `start_worker`
/// itself fails, since there is nothing to cancel or watch yet.
pub fn run_worker(
    sink: &impl RunSink,
    contract: &WorkerContract,
    cwd: &Path,
    thresholds: LivenessThresholds,
    options: &RunOptions,
    mut now_ms: impl FnMut() -> i64,
    on_started: impl FnOnce(CancelToken, LivenessHandle, Option<SteerHandle>),
) -> Result<RunReport, ManagerError> {
    let started_ts = now_ms();
    sink.upsert_run(&row(contract, None, 0, 0, started_ts, None))?;
    // `05 run state model`: immutability makes the sealed contract one durable
    // answer after the process exits. The same payload pins whether this was a
    // manager or worker and snapshots the manager definition used for later
    // re-run or re-scope. The caller supplies who caused the queue operation.
    let mut queued_payload = serde_json::to_value(contract).unwrap_or(serde_json::Value::Null);
    if let Some(payload) = queued_payload.as_object_mut() {
        payload.insert(
            RUN_ROLE_FIELD.into(),
            serde_json::Value::String(options.role.as_record_str().into()),
        );
        if let Some(cell) = &options.manager_cell {
            payload.insert(
                MANAGER_CELL_FIELD.into(),
                serde_json::to_value(cell).unwrap_or(serde_json::Value::Null),
            );
        }
    }
    sink.append(&NewEvent::new(
        contract.cell_id.clone(),
        contract.run_id,
        EventKind::new(EventKind::RUN_QUEUED),
        options.actor,
        started_ts,
        queued_payload,
    ))?;

    let result = start_worker(contract, cwd, thresholds, options).and_then(|started| {
        on_started(
            started.cancel_token(),
            started.liveness_handle(),
            started.steer_handle(),
        );
        let progress_actor = match options.role {
            RunRole::Manager => Actor::Manager,
            RunRole::Worker => Actor::Worker,
        };
        started.run_to_completion(sink, contract, progress_actor, &mut now_ms)
    });

    let finished_ts = now_ms();
    sink.upsert_run(&finished_row(contract, &result, started_ts, finished_ts))?;

    result
}

fn finished_row(
    contract: &WorkerContract,
    result: &Result<RunReport, ManagerError>,
    started_ts: i64,
    finished_ts: i64,
) -> RunRow {
    let (outcome, usd_micros, tokens) = match result {
        Ok(report) => (
            report.outcome,
            report.cost_usd_micros.unwrap_or(0).max(0) as u64,
            report.tokens.unwrap_or(0).max(0) as u64,
        ),
        Err(ManagerError::Cancelled(report)) => (
            Outcome::Cancelled,
            report.cost_usd_micros.unwrap_or(0).max(0) as u64,
            report.tokens.unwrap_or(0).max(0) as u64,
        ),
        Err(_) => (Outcome::Failed, 0, 0),
    };
    row(
        contract,
        Some(outcome),
        usd_micros,
        tokens,
        started_ts,
        Some(finished_ts),
    )
}

fn row(
    contract: &WorkerContract,
    outcome: Option<Outcome>,
    usd_micros: u64,
    tokens: u64,
    started_ts: i64,
    finished_ts: Option<i64>,
) -> RunRow {
    RunRow {
        run_id: contract.run_id,
        task_id: contract.task_id,
        cell_id: contract.cell_id.clone(),
        runner: contract.runner.clone(),
        // A `WorkerContract` names a runner, not a model, so per-model
        // attribution (`11 analytics questions`) waits on the contract carrying one.
        model: String::new(),
        outcome: outcome.map(outcome_str).map(str::to_string),
        usd_micros,
        tokens,
        operator_touched: false,
        started_ts,
        finished_ts,
    }
}

fn outcome_str(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Ok => "ok",
        Outcome::Failed => "failed",
        Outcome::Cancelled => "cancelled",
        Outcome::Abandoned => "abandoned",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use farseer_core::event::EventKind;
    use farseer_core::policy::{Budget, Irreversibility};
    use farseer_core::run::{WorkerContractSpec, WorkspaceStrategy};
    use farseer_core::{CellId, RunId, TaskId};
    use farseer_store::ScanFilter;

    fn contract() -> WorkerContract {
        WorkerContract::seal(WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "irrelevant - this test spawns cmd.exe directly".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "claude-code".into(),
            tool_grants: vec![],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "".into(),
        })
    }

    /// A fixture file read with `type`, exactly as `farseer-runner`'s own
    /// `drive` test does - `cmd`'s quoting mangles JSON passed through
    /// `echo` directly.
    fn fixture_process(lines: &[&str]) -> (tempfile::TempDir, StartedWorker) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lines.txt");
        std::fs::write(path.clone(), lines.join("\r\n") + "\r\n").unwrap();
        let started = StartedWorker::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &["/c".into(), "type".into(), path.to_str().unwrap().into()],
            &std::env::current_dir().unwrap(),
            LivenessThresholds::default(),
            farseer_runner::claude_code::parse_line,
            None,
        )
        .unwrap();
        (dir, started)
    }

    #[test]
    fn a_progress_signal_becomes_an_event_in_the_store_in_arrival_order() {
        let store = Store::open_in_memory().unwrap();
        let contract = contract();
        let (_dir, started) = fixture_process(&[
            r#"{"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"tool_use","id":"toolu_1","name":"Read"}}}"#,
            r#"{"type":"result","subtype":"success","total_cost_usd":0.05,"usage":{"input_tokens":10,"output_tokens":5}}"#,
        ]);

        let report = started
            .run_to_completion(&store, &contract, Actor::Worker, || 1)
            .unwrap();

        assert_eq!(report.outcome, Outcome::Ok);
        assert_eq!(report.cost_usd_micros, Some(50_000));
        assert_eq!(report.tokens, Some(15));
        assert_eq!(report.result, None);

        let events = store.scan(0, 10, &ScanFilter::default()).unwrap();
        assert_eq!(
            events.len(),
            1,
            "only the progress signal is recorded, not the result"
        );
        assert_eq!(events[0].kind.as_str(), EventKind::TOOL_CALL_STARTED);
        assert_eq!(events[0].run_id, contract.run_id);
        assert_eq!(events[0].actor, Actor::Worker);
    }

    #[test]
    fn a_malformed_line_is_skipped_rather_than_aborting_the_run() {
        let store = Store::open_in_memory().unwrap();
        let contract = contract();
        let (_dir, started) = fixture_process(&[
            "not json at all",
            r#"{"type":"result","subtype":"success"}"#,
        ]);

        let report = started
            .run_to_completion(&store, &contract, Actor::Worker, || 1)
            .unwrap();
        assert_eq!(report.outcome, Outcome::Ok);
        assert!(
            store
                .scan(0, 10, &ScanFilter::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn run_worker_writes_a_running_row_then_a_finished_one_even_on_failure() {
        let store = Store::open_in_memory().unwrap();
        // Deterministic across machines: an unsupported runner name fails in
        // `start_worker` before anything is spawned, whether or not `claude`
        // itself happens to be installed here. This is the row lifecycle,
        // not the happy path - that's covered by the two tests above driving
        // `run_to_completion` directly against a real spawned process.
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "irrelevant".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "not-a-real-runner".into(),
            tool_grants: vec![],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "".into(),
        };
        let run_id = spec.run_id;
        let contract = WorkerContract::seal(spec);

        let mut tick = 100i64;
        let result = run_worker(
            &store,
            &contract,
            &std::env::temp_dir(),
            LivenessThresholds::default(),
            &RunOptions::default(),
            || {
                tick += 1;
                tick
            },
            |_, _, _| panic!("on_started must not fire when start_worker itself fails"),
        );

        assert!(matches!(result, Err(ManagerError::UnsupportedRunner(_))));
        let row = store.run(run_id).unwrap().unwrap();
        assert_eq!(row.outcome.as_deref(), Some("failed"));
        assert!(row.finished_ts.is_some());
        assert!(row.started_ts < row.finished_ts.unwrap());
    }

    #[test]
    fn a_cancelled_run_row_preserves_usage_from_an_observed_terminal_result() {
        let contract = contract();
        let result = Err(ManagerError::Cancelled(RunReport {
            outcome: Outcome::Cancelled,
            cost_usd_micros: Some(123_456),
            tokens: Some(789),
            result: Some("reported result".into()),
            window: None,
        }));

        let row = finished_row(&contract, &result, 1, 2);

        assert_eq!(row.outcome.as_deref(), Some("cancelled"));
        assert_eq!(row.usd_micros, 123_456);
        assert_eq!(row.tokens, 789);
    }

    #[test]
    fn a_cancelled_run_row_without_a_terminal_result_records_zero_usage() {
        let contract = contract();
        let result = Err(ManagerError::Cancelled(RunReport {
            outcome: Outcome::Cancelled,
            cost_usd_micros: None,
            tokens: None,
            result: None,
            window: None,
        }));

        let row = finished_row(&contract, &result, 1, 2);

        assert_eq!(row.outcome.as_deref(), Some("cancelled"));
        assert_eq!(row.usd_micros, 0);
        assert_eq!(row.tokens, 0);
    }

    #[test]
    fn run_worker_records_the_sealed_contract_as_a_run_queued_event_even_on_failure() {
        // `05 run state model`: immutability is what makes "what was this worker allowed to
        // do" answerable after the fact - but nothing durable carried the
        // goal or the grants until this event. Written even on a failure
        // path, since the record should say what was attempted regardless
        // of what happened next.
        let store = Store::open_in_memory().unwrap();
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "reconstruct me later".into(),
            workspace: WorkspaceStrategy::PlainDirectory,
            runner: "not-a-real-runner".into(),
            tool_grants: vec!["shell".into()],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "done".into(),
        };
        let run_id = spec.run_id;
        let contract = WorkerContract::seal(spec);

        let _ = run_worker(
            &store,
            &contract,
            &std::env::temp_dir(),
            LivenessThresholds::default(),
            &RunOptions::default(),
            || 1,
            |_, _, _| {},
        );

        let events = store
            .scan(0, 10, &farseer_store::ScanFilter::run(run_id))
            .unwrap();
        let queued = events
            .iter()
            .find(|e| e.kind.as_str() == EventKind::RUN_QUEUED)
            .expect("a run_queued event should have been written");
        assert_eq!(queued.payload["goal"], "reconstruct me later");
        assert_eq!(queued.payload["tool_grants"][0], "shell");
        let rebuilt: WorkerContractSpec = serde_json::from_value(queued.payload.clone()).unwrap();
        assert_eq!(rebuilt.goal, contract.goal);
        assert_eq!(rebuilt.definition_of_done, contract.definition_of_done);
    }

    fn completed_turn_fixture(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
        if line != "done" {
            return Ok(Vec::new());
        }
        Ok(vec![
            RunnerSignal::Output("ok".into()),
            RunnerSignal::Finished(farseer_runner::claude_code::FinishedSignal {
                outcome: Outcome::Ok,
                cost_usd_micros: Some(1),
                tokens: Some(1),
            }),
        ])
    }

    #[test]
    fn cancelling_after_a_completed_turn_returns_cancelled_with_the_turn_report() {
        let store = Store::open_in_memory().unwrap();
        let contract = contract();
        let dir = tempfile::tempdir().unwrap();
        let result_path = dir.path().join("result.txt");
        std::fs::write(&result_path, "done\r\n").unwrap();
        let script_path = dir.path().join("result-then-wait.cmd");
        std::fs::write(
            &script_path,
            format!(
                "@echo off\r\ntype \"{}\"\r\nping -n 30 127.0.0.1 >nul\r\n",
                result_path.display()
            ),
        )
        .unwrap();
        let started = StartedWorker::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &[
                "/d".into(),
                "/c".into(),
                script_path.to_string_lossy().into_owned(),
            ],
            &std::env::current_dir().unwrap(),
            LivenessThresholds::default(),
            completed_turn_fixture,
            None,
        )
        .unwrap();
        let token = started.cancel_token();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            token.cancel();
        });

        let result = started.run_to_completion(&store, &contract, Actor::Worker, || 1);
        let Err(ManagerError::Cancelled(report)) = result else {
            panic!("expected cancellation with the completed turn report, got {result:?}");
        };
        assert_eq!(report.outcome, Outcome::Cancelled);
        assert_eq!(report.cost_usd_micros, Some(1));
        assert_eq!(report.tokens, Some(1));
        assert_eq!(report.result.as_deref(), Some("ok"));
    }

    #[test]
    fn cancelling_before_any_result_returns_cancelled_with_an_unknown_report() {
        let store = Store::open_in_memory().unwrap();
        let contract = contract();
        let started = StartedWorker::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &["/c".into(), "ping -n 30 127.0.0.1 >nul".into()],
            &std::env::current_dir().unwrap(),
            LivenessThresholds::default(),
            farseer_runner::claude_code::parse_line,
            // Exercises the bootstrap-write path alongside cancellation:
            // `ping`'s own stdin is unread, so writing the goal frame to
            // it before the read loop starts must not itself break anything.
            Some(farseer_runner::claude_code::steer_frame),
        )
        .unwrap();
        assert!(
            started.steer_handle().is_some(),
            "a runner with a steer frame exposes a steer handle"
        );
        let token = started.cancel_token();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            token.cancel();
        });

        // `05 run state model`: cancelled is never failed. Without the result line a crash
        // and a deliberate cancel look identical on the wire; `Cancelled`
        // proves the distinction survived rather than collapsing to `NoResult`.
        let result = started.run_to_completion(&store, &contract, Actor::Worker, || 1);
        let Err(ManagerError::Cancelled(report)) = result else {
            panic!("expected cancellation with an unknown report, got {result:?}");
        };
        assert_eq!(report.outcome, Outcome::Cancelled);
        assert_eq!(report.cost_usd_micros, None);
        assert_eq!(report.tokens, None);
        assert_eq!(report.result, None);
    }

    #[test]
    fn start_worker_dispatches_codex_rather_than_refusing_it_as_unsupported() {
        // Whether `codex` actually resolves on this machine varies, but
        // either way `start_worker` must not answer `UnsupportedRunner` -
        // that would mean the dispatch itself is broken, not the
        // environment. Cancelled almost immediately, bounding the cost of a
        // real invocation if `codex` happens to be installed here.
        let store = Store::open_in_memory().unwrap();
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "reply with just the word ok".into(),
            workspace: WorkspaceStrategy::PlainDirectory,
            runner: "codex".into(),
            tool_grants: vec![],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "".into(),
        };
        let contract = WorkerContract::seal(spec);

        let result = run_worker(
            &store,
            &contract,
            &std::env::temp_dir(),
            LivenessThresholds::default(),
            &RunOptions::default(),
            || 1,
            |token, _liveness, steer| {
                assert!(
                    steer.is_none(),
                    "codex has no steering path - `codex exec resume` starts a new process"
                );
                token.cancel();
            },
        );

        assert!(
            !matches!(result, Err(ManagerError::UnsupportedRunner(_))),
            "codex must dispatch to a process, not fall through to the unsupported-runner error: {result:?}"
        );
    }

    #[test]
    fn start_worker_dispatches_goose_rather_than_refusing_it_as_unsupported() {
        let store = Store::open_in_memory().unwrap();
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "reply with just the word ok".into(),
            workspace: WorkspaceStrategy::PlainDirectory,
            runner: "goose".into(),
            tool_grants: vec![],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "".into(),
        };
        let contract = WorkerContract::seal(spec);

        let result = run_worker(
            &store,
            &contract,
            &std::env::temp_dir(),
            LivenessThresholds::default(),
            &RunOptions::default(),
            || 1,
            |token, _liveness, steer| {
                assert!(
                    steer.is_none(),
                    "goose has no steering path - `-r/--resume` starts a new process"
                );
                token.cancel();
            },
        );

        assert!(
            !matches!(result, Err(ManagerError::UnsupportedRunner(_))),
            "goose must dispatch to a process, not fall through to the unsupported-runner error: {result:?}"
        );
    }

    #[test]
    fn a_silent_process_goes_stalled_then_likely_hung_from_elapsed_time_alone() {
        // `ping`'s own output is redirected to `nul` inside the child, so
        // zero lines ever reach `drive` - this tests the handle purely
        // against wall-clock time, with `run_to_completion` never called.
        let started = StartedWorker::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &["/c".into(), "ping -n 30 127.0.0.1 >nul".into()],
            &std::env::current_dir().unwrap(),
            LivenessThresholds {
                stalled_secs: 0,
                likely_hung_secs: 1,
            },
            farseer_runner::claude_code::parse_line,
            None,
        )
        .unwrap();
        let handle = started.liveness_handle();

        assert_eq!(
            handle.liveness(),
            Liveness::Stalled,
            "0s: past the stalled threshold immediately, not yet likely-hung"
        );
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(handle.liveness(), Liveness::LikelyHung);

        started.cancel_token().cancel();
    }

    #[test]
    fn a_real_line_keeps_the_handle_live_after_the_run_finishes() {
        let store = Store::open_in_memory().unwrap();
        let contract = contract();
        let (_dir, started) = fixture_process(&[r#"{"type":"result","subtype":"success"}"#]);
        let handle = started.liveness_handle();

        started
            .run_to_completion(&store, &contract, Actor::Worker, || 1)
            .unwrap();

        assert_eq!(
            handle.liveness(),
            Liveness::Live,
            "the result line was activity moments ago, well inside the default 120s threshold"
        );
    }
}
