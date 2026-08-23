//! The manager loop's first slice: run one [`WorkerContract`] against the
//! Claude Code runner, end to end, with the run row and its progress events
//! landing in the [`Store`].
//!
//! **What this is not.** `05`'s four manager verbs - steer, re-scope, cancel,
//! re-run - are not implemented here; this is the synchronous execution path
//! one of them will eventually call. There is no watchdog thread either:
//! [`StartedWorker::run_to_completion`] blocks inside `read_line`, so nothing
//! can notice a stall *while blocked* without a second thread polling
//! wall-clock time against the last-activity timestamp this crate does not
//! yet keep anywhere durable. [`farseer_runner::spawn::CancelToken`] is the
//! seam that watchdog would call into - it already works from another
//! thread, proven in `farseer-runner`'s own tests - so adding the watchdog
//! is wiring, not new mechanism.
//!
//! **`CancelToken::cancel` does not yet produce `05`'s `Cancelled` outcome.**
//! Closing the job ends the process without its terminal `result` line ever
//! arriving, so [`StartedWorker::run_to_completion`] sees end-of-stream with
//! no [`RunnerSignal::Finished`] and returns [`ManagerError::NoResult`],
//! which [`run_worker`] then records as `Failed`. `05` is explicit that
//! `cancelled` is never `failed` - a human choosing not to proceed must not
//! read as something having broken. Fixing this needs the caller of
//! `cancel()` to tell `run_worker` a cancellation is in flight, which is
//! exactly the manager-verb wiring this crate does not have yet.

use std::path::Path;

use farseer_core::{Actor, NewEvent, Outcome, run::WorkerContract};
use farseer_runner::claude_code::RunnerSignal;
use farseer_runner::drive::drive;
use farseer_runner::invocation::build_args;
use farseer_runner::resolve::resolve;
use farseer_runner::spawn::{CancelToken, SpawnError, SupervisedProcess};
use farseer_store::{RunRow, Store, StoreError};

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
}

pub struct RunReport {
    pub outcome: Outcome,
    pub cost_usd_micros: Option<i64>,
    pub tokens: Option<i64>,
}

/// A worker process, spawned and waiting to be driven.
///
/// Split from spawning-and-driving in one call so a caller holds a
/// [`CancelToken`] *before* the blocking read loop starts: the token has to
/// exist before it might be needed, and `run_to_completion` takes `self` by
/// value precisely so it cannot be called twice on the same process.
pub struct StartedWorker {
    proc: SupervisedProcess,
}

impl StartedWorker {
    pub fn spawn(exe: &Path, args: &[String], cwd: &Path) -> Result<Self, ManagerError> {
        Ok(Self {
            proc: SupervisedProcess::spawn(exe, args, cwd)?,
        })
    }

    pub fn cancel_token(&self) -> CancelToken {
        self.proc.cancel_token()
    }

    /// Blocks until the process closes stdout. Every progress signal becomes
    /// a `NewEvent` appended to `store`, in the order it arrived. A line
    /// `farseer_runner::claude_code::parse_line` cannot parse is `05`'s
    /// activity signal same as any other line - the process is still there -
    /// and is otherwise skipped, since there is nothing shaped enough to
    /// record.
    pub fn run_to_completion(
        mut self,
        store: &Store,
        contract: &WorkerContract,
        mut now_ms: impl FnMut() -> i64,
    ) -> Result<RunReport, ManagerError> {
        let mut report = None;
        let mut store_err = None;
        let cancel_on_store_failure = self.proc.cancel_token();

        drive(&mut self.proc, |parsed| {
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
                            Actor::Worker,
                            now_ms(),
                            payload,
                        );
                        if let Err(e) = store.append(&event) {
                            store_err = Some(e);
                            // The record can no longer be trusted for this
                            // run; there is no point paying for more of it.
                            cancel_on_store_failure.cancel();
                            return;
                        }
                    }
                    RunnerSignal::Finished(f) => {
                        report = Some(RunReport {
                            outcome: f.outcome,
                            cost_usd_micros: f.cost_usd_micros,
                            tokens: f.tokens,
                        });
                    }
                    // `27`'s quota accounting is not wired yet: observed,
                    // dropped, rather than guessed at.
                    RunnerSignal::RateLimit(_) => {}
                }
            }
        })?;

        if let Some(e) = store_err {
            return Err(e.into());
        }
        report.ok_or(ManagerError::NoResult)
    }
}

/// `20` chose two runner implementations for v1; only `claude-code` is wired
/// to a process yet, so it is the only value `contract.runner` may hold here.
pub fn start_worker(contract: &WorkerContract, cwd: &Path) -> Result<StartedWorker, ManagerError> {
    match contract.runner.as_str() {
        "claude-code" => {
            let exe = resolve("claude")
                .ok_or_else(|| ManagerError::ExecutableNotFound("claude".into()))?;
            StartedWorker::spawn(&exe, &build_args(contract), cwd)
        }
        other => Err(ManagerError::UnsupportedRunner(other.to_string())),
    }
}

/// The whole run row lifecycle: `upsert_run` with no outcome at the start,
/// `upsert_run` again with the final one at the end - so a run that this
/// process crashes mid-flight is left `running` forever rather than silently
/// missing, matching `17`'s choice to surface an orphan rather than paper
/// over it.
pub fn run_worker(
    store: &Store,
    contract: &WorkerContract,
    cwd: &Path,
    mut now_ms: impl FnMut() -> i64,
) -> Result<RunReport, ManagerError> {
    let started_ts = now_ms();
    store.upsert_run(&row(contract, None, 0, 0, started_ts, None))?;

    let result = start_worker(contract, cwd)
        .and_then(|started| started.run_to_completion(store, contract, &mut now_ms));

    let finished_ts = now_ms();
    let (outcome, usd_micros, tokens) = match &result {
        Ok(report) => (
            report.outcome,
            report.cost_usd_micros.unwrap_or(0).max(0) as u64,
            report.tokens.unwrap_or(0).max(0) as u64,
        ),
        Err(_) => (Outcome::Failed, 0, 0),
    };
    store.upsert_run(&row(
        contract,
        Some(outcome),
        usd_micros,
        tokens,
        started_ts,
        Some(finished_ts),
    ))?;

    result
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
        // attribution (`11`) waits on the contract carrying one.
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

        let report = started.run_to_completion(&store, &contract, || 1).unwrap();

        assert_eq!(report.outcome, Outcome::Ok);
        assert_eq!(report.cost_usd_micros, Some(50_000));
        assert_eq!(report.tokens, Some(15));

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

        let report = started.run_to_completion(&store, &contract, || 1).unwrap();
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
        let result = run_worker(&store, &contract, &std::env::temp_dir(), || {
            tick += 1;
            tick
        });

        assert!(matches!(result, Err(ManagerError::UnsupportedRunner(_))));
        let row = store.run(run_id).unwrap().unwrap();
        assert_eq!(row.outcome.as_deref(), Some("failed"));
        assert!(row.finished_ts.is_some());
        assert!(row.started_ts < row.finished_ts.unwrap());
    }

    #[test]
    fn cancelling_ends_the_run_without_a_result_event_rather_than_hanging() {
        let store = Store::open_in_memory().unwrap();
        let contract = contract();
        let started = StartedWorker::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &["/c".into(), "ping -n 30 127.0.0.1 >nul".into()],
            &std::env::current_dir().unwrap(),
        )
        .unwrap();
        let token = started.cancel_token();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            token.cancel();
        });

        let result = started.run_to_completion(&store, &contract, || 1);
        assert!(matches!(result, Err(ManagerError::NoResult)));
    }
}
