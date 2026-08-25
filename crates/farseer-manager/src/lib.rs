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
use farseer_runner::spawn::{CancelToken, SpawnError, StdinHandle, StdinMode, SupervisedProcess};
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
    #[error(transparent)]
    Acp(#[from] farseer_runner::acp_drive::AcpError),
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
    /// What the runner said about the session, if anything.
    ///
    /// `RunRow.model` existed from the beginning and was **always empty**,
    /// because nothing read the one line where a runner names its model.
    pub session: Option<farseer_runner::claude_code::SessionInfo>,
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

/// The ACP mode farseer opens a session in.
///
/// `goose acp` offers `auto`, `approve`, `smart_approve` and `chat`; only the
/// first runs unattended. An agent that does not offer this exact id opens in
/// its own default, which is a gap `29 harness protocol` recorded rather than
/// papered over - the mode names are not standardised, and guessing at another
/// agent's synonym would be inventing a grant `12 autonomy and deny list` did
/// not give.
pub const ACP_UNATTENDED_MODE: &str = "auto";

/// How farseer speaks to a runner, which decides three things at once: whether
/// a stdin exists, how the goal is delivered, and what ends the read loop.
///
/// It replaced an `Option<fn(&str) -> String>` because that field had quietly
/// become the answer to all three questions and could only express two of them.
/// `29 harness protocol` added the third case and the omission became a bug -
/// twice, in the same shape.
#[derive(Debug, Clone, Copy)]
pub enum Channel {
    /// Goal on argv, EOF at spawn, ends at end of stream.
    ///
    /// `28 operator surface` learned the second clause the hard way: `codex
    /// exec` waits for EOF **before it starts**, so an open pipe nobody writes
    /// to is a run that never begins.
    OneShot,
    /// Goal as the first stdin frame, steerable, ends at end of stream.
    ///
    /// Only Claude Code, per `invocation.rs`.
    Steered(fn(&str) -> String),
    /// An ACP agent: a JSON-RPC handshake, then the goal as `session/prompt`.
    ///
    /// **Ends at the terminal signal rather than at end of stream**, because an
    /// ACP agent does not exit when the turn ends - the session stays open for
    /// the next prompt. Waiting for EOF here waits forever, which is exactly how
    /// `29 harness protocol`'s first live run hung.
    Acp,
}

impl Channel {
    fn stdin_mode(self) -> StdinMode {
        match self {
            // A goal or a handshake has to reach it.
            Self::Steered(_) | Self::Acp => StdinMode::Live,
            Self::OneShot => StdinMode::Closed,
        }
    }

    /// Whether the read loop stops at the terminal signal instead of at end of
    /// stream.
    fn ends_at_terminal(self) -> bool {
        matches!(self, Self::Acp)
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
    /// How the goal gets in, whether steering can reach, and what ends the
    /// read loop. See [`Channel`].
    channel: Channel,
    /// Set once the ACP handshake has run, so a steer knows which session to
    /// address. `None` for every other channel, and for an ACP run that has not
    /// been bootstrapped yet.
    acp_session: Option<String>,
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
        channel: Channel,
    ) -> Result<Self, ManagerError> {
        Ok(Self {
            proc: SupervisedProcess::spawn(exe, args, cwd, channel.stdin_mode())?,
            activity: Arc::new(Mutex::new(ActivityClock::started_at(0))),
            monotonic_start: Instant::now(),
            thresholds,
            parse,
            channel,
            acp_session: None,
        })
    }

    pub fn cancel_token(&self) -> CancelToken {
        self.proc.cancel_token()
    }

    /// Fetch before calling the blocking `run_to_completion`, same reason as
    /// [`Self::cancel_token`]. `None` when this runner has no steering path.
    pub fn steer_handle(&self) -> Option<SteerHandle> {
        // Both halves have to be there: a frame to wrap the message in, and a
        // stdin to write it to. They are set together at spawn, and requiring
        // both here means a mismatch cannot produce a handle that writes into
        // nothing.
        let stdin = self.proc.stdin_handle()?;
        match self.channel {
            Channel::Steered(frame) => Some(SteerHandle { stdin, frame }),
            // An ACP steer is another `session/prompt` on the same session,
            // which needs the session id and a fresh request id - neither of
            // which fits a `fn(&str) -> String`. `20 worker control channel`
            // made steering the exception, so an ACP worker is unsteerable
            // until a manager needs it rather than speculatively.
            Channel::Acp | Channel::OneShot => None,
        }
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
    ///
    /// For [`Channel::Acp`] this is where the whole handshake happens -
    /// `initialize`, `session/new`, `session/set_mode`, then the goal as
    /// `session/prompt`. Signals arriving during the handshake are dropped
    /// rather than recorded: `bootstrap` has no store, and the one line this
    /// loses in practice is a `usage_update` that the agent sends again after
    /// the turn.
    pub fn bootstrap(&mut self, goal: &str, cwd: &Path) -> Result<(), ManagerError> {
        match self.channel {
            Channel::OneShot => {}
            Channel::Steered(frame) => self.proc.write_line(&frame(goal))?,
            Channel::Acp => {
                let mut next_id = 1;
                let mut discard = |_: Result<Vec<RunnerSignal>, ParseError>| {};
                let opened = farseer_runner::acp_drive::handshake(
                    &mut self.proc,
                    cwd,
                    // The mode that does not ask. `12 autonomy and deny list`
                    // decides autonomy before the run, and nobody is watching a
                    // prompt farseer did not expect.
                    Some(ACP_UNATTENDED_MODE),
                    &mut next_id,
                    &mut discard,
                )?;
                farseer_runner::acp_drive::prompt_on(
                    &self.proc,
                    &opened.session_id,
                    next_id,
                    goal,
                )?;
                self.acp_session = Some(opened.session_id);
            }
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
        let mut session: Option<farseer_runner::claude_code::SessionInfo> = None;
        let mut store_err = None;
        let cancel_on_store_failure = self.proc.cancel_token();
        let activity = Arc::clone(&self.activity);
        let monotonic_start = self.monotonic_start;

        let ends_at_terminal = self.channel.ends_at_terminal();
        let mut on_line = |parsed: Result<Vec<RunnerSignal>, ParseError>| {
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
                    RunnerSignal::Output(text) => {
                        // Recorded per turn, not held until the run ends: a
                        // manager on live stdin answers and waits, so holding it
                        // would mean the operator hears nothing until they close
                        // the session they are trying to talk to.
                        let event = NewEvent::new(
                            contract.cell_id.clone(),
                            contract.run_id,
                            EventKind::new(EventKind::MANAGER_ANSWERED),
                            progress_actor,
                            now_ms(),
                            serde_json::json!({ "text": text }),
                        );
                        if let Err(e) = sink.append(&event) {
                            store_err = Some(e);
                            cancel_on_store_failure.cancel();
                            return;
                        }
                        output = Some(text);
                    }
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
                            session: None,
                        });
                    }
                    // `27 quota accounting`: observed on every successful run,
                    // carried out on the report, and appended by the layer that
                    // knows which account it belongs to - on change only.
                    RunnerSignal::RateLimit(info) => window = Some(Box::new(info)),
                    // Straight into the record rather than onto the report:
                    // `28 operator surface`'s meta strip reads the event stream,
                    // and a reading that only survives to the end of the run
                    // cannot show a context window filling up while it fills.
                    RunnerSignal::Usage(info) => {
                        let event = NewEvent::new(
                            contract.cell_id.clone(),
                            contract.run_id,
                            EventKind::new(EventKind::USAGE_UPDATED),
                            progress_actor,
                            now_ms(),
                            serde_json::json!({
                                "used": info.used,
                                "size": info.size,
                                "cost_usd_micros": info.cost_usd_micros,
                            }),
                        );
                        if let Err(e) = sink.append(&event) {
                            store_err = Some(e);
                            cancel_on_store_failure.cancel();
                            return;
                        }
                    }
                    RunnerSignal::Session(info) => {
                        let event = NewEvent::new(
                            contract.cell_id.clone(),
                            contract.run_id,
                            EventKind::new(EventKind::SESSION_STARTED),
                            progress_actor,
                            now_ms(),
                            serde_json::json!({
                                "model": info.model,
                                "session_id": info.session_id,
                                "runner": contract.runner,
                            }),
                        );
                        if let Err(e) = sink.append(&event) {
                            store_err = Some(e);
                            cancel_on_store_failure.cancel();
                            return;
                        }
                        session = Some(info);
                    }
                }
            }
        };

        if ends_at_terminal {
            // A conversational runner stays alive after the work is done, so
            // end of stream never comes. `29 harness protocol`'s first live run
            // waited for it anyway and hung.
            while let Some(line) = self.proc.read_line()? {
                let parsed = (self.parse)(&line);
                let ended = farseer_runner::acp_drive::ends_turn(&parsed);
                on_line(parsed);
                if ended {
                    break;
                }
            }
        } else {
            drive(&mut self.proc, self.parse, on_line)?;
        }

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
                report.session = session.clone();
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
                session,
            })),
            None => Err(ManagerError::NoResult),
        }
    }
}

/// The ACP runners, as `name -> (executable, subcommand)`.
///
/// Both were verified installed and speaking ACP on 2026-08-26. The list is
/// short on purpose: an entry here is a claim that farseer has **seen this
/// agent's output**, which is `10 runner inventory`'s rule, not a claim that
/// ACP agents in general work.
pub const ACP_RUNNERS: [(&str, &str, &str); 2] = [
    ("goose-acp", "goose", "acp"),
    ("opencode-acp", "opencode", "acp"),
];

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
                        edits_granted: options
                            .manager_cell
                            .as_ref()
                            .is_some_and(|cell| cell.has_shell_grant()),
                    },
                ),
                cwd,
                thresholds,
                farseer_runner::claude_code::parse_line,
                if options.role == RunRole::Manager {
                    Channel::Steered(farseer_runner::claude_code::steer_frame)
                } else {
                    Channel::OneShot
                },
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
                Channel::OneShot,
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
                Channel::OneShot,
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
                Channel::OneShot,
            )
        }
        // The ACP runners, per `29 harness protocol`. A runner name here means
        // **an executable and a subcommand**, which is a shape `10 runner
        // inventory` never had to carry: `goose` and `goose-acp` are the same
        // binary offering two different faces, and they are different runners
        // because they report different things.
        other => match ACP_RUNNERS.iter().find(|(name, _, _)| *name == other) {
            Some((_, exe_name, subcommand)) => {
                let exe = resolve(exe_name)
                    .ok_or_else(|| ManagerError::ExecutableNotFound((*exe_name).into()))?;
                StartedWorker::spawn(
                    &exe,
                    &[(*subcommand).to_string()],
                    cwd,
                    thresholds,
                    farseer_runner::acp::parse_line,
                    Channel::Acp,
                )
            }
            None => return Err(ManagerError::UnsupportedRunner(other.to_string())),
        },
    }?;
    let mut started = started;
    started.bootstrap(&contract.goal, cwd)?;
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
    sink.upsert_run(&row(contract, None, 0, 0, started_ts, None, String::new()))?;
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
    // `05 run state model` named this lifecycle kind and nothing emitted it, so
    // `16 local api surface`'s promise that "the answer arrives on the event
    // stream" was unkept: an operator who instructed a manager could see it
    // start and never see what it said. The terminal text only travelled back
    // to a *delegating* manager, over MCP.
    //
    // A failure to record this must not turn a finished run into a failed one,
    // so the append is best-effort.
    let _ = sink.append(&NewEvent::new(
        contract.cell_id.clone(),
        contract.run_id,
        EventKind::new(EventKind::RUN_FINISHED),
        match options.role {
            RunRole::Manager => Actor::Manager,
            RunRole::Worker => Actor::Worker,
        },
        finished_ts,
        finished_payload(&result),
    ));

    result
}

/// What a finished run has to say for itself.
///
/// `02 record scope` scrubs on write, so the text is scrubbed like any other
/// payload rather than being trusted because a manager wrote it.
fn finished_payload(result: &Result<RunReport, ManagerError>) -> serde_json::Value {
    let (outcome, text, cost, tokens) = match result {
        Ok(report) => (
            outcome_str(report.outcome),
            report.result.clone(),
            report.cost_usd_micros,
            report.tokens,
        ),
        Err(ManagerError::Cancelled(report)) => (
            outcome_str(Outcome::Cancelled),
            report.result.clone(),
            report.cost_usd_micros,
            report.tokens,
        ),
        // A run that never produced a report has nothing to quote, and inventing
        // an apology in the manager's voice would put words in the record that
        // no agent said.
        Err(error) => (
            outcome_str(Outcome::Failed),
            Some(error.to_string()),
            None,
            None,
        ),
    };
    serde_json::json!({
        "outcome": outcome,
        "text": text,
        "cost_usd_micros": cost,
        "tokens": tokens,
    })
}

fn finished_row(
    contract: &WorkerContract,
    result: &Result<RunReport, ManagerError>,
    started_ts: i64,
    finished_ts: i64,
) -> RunRow {
    let model = match result {
        Ok(report) | Err(ManagerError::Cancelled(report)) => report
            .session
            .as_ref()
            .and_then(|session| session.model.clone())
            .unwrap_or_default(),
        Err(_) => String::new(),
    };
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
        model,
    )
}

fn row(
    contract: &WorkerContract,
    outcome: Option<Outcome>,
    usd_micros: u64,
    tokens: u64,
    started_ts: i64,
    finished_ts: Option<i64>,
    model: String,
) -> RunRow {
    RunRow {
        run_id: contract.run_id,
        task_id: contract.task_id,
        cell_id: contract.cell_id.clone(),
        runner: contract.runner.clone(),
        // Observed, not configured. A `WorkerContract` names a runner and never
        // a model, and this used to be empty for that reason - but the runner
        // announces the model it actually used, which is the better answer for
        // `11 analytics questions` anyway: what ran, rather than what was asked
        // for. Empty until it says so.
        model,
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
            Channel::OneShot,
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
    fn a_finished_run_says_what_it_answered() {
        // `16 local api surface` promised the answer arrives on the stream, and
        // for an operator-instructed manager nothing put it there: the terminal
        // text only travelled back to a delegating manager over MCP.
        let payload = finished_payload(&Ok(RunReport {
            outcome: Outcome::Ok,
            cost_usd_micros: Some(12_000),
            tokens: Some(340),
            result: Some("the changelog is posted".into()),
            window: None,
            session: None,
        }));

        assert_eq!(payload["outcome"], "ok");
        assert_eq!(payload["text"], "the changelog is posted");
        assert_eq!(payload["cost_usd_micros"], 12_000);
    }

    #[test]
    fn a_cancelled_run_still_reports_what_it_managed_to_say() {
        let payload = finished_payload(&Err(ManagerError::Cancelled(RunReport {
            outcome: Outcome::Cancelled,
            cost_usd_micros: None,
            tokens: None,
            result: Some("halfway through".into()),
            window: None,
            session: None,
        })));

        assert_eq!(payload["outcome"], "cancelled");
        assert_eq!(payload["text"], "halfway through");
    }

    #[test]
    fn a_run_with_no_report_quotes_the_error_rather_than_inventing_a_voice() {
        let payload = finished_payload(&Err(ManagerError::NoResult));
        assert_eq!(payload["outcome"], "failed");
        assert!(payload["text"].as_str().is_some_and(|t| !t.is_empty()));
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
            session: None,
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
            session: None,
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
            Channel::OneShot,
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
            Channel::Steered(farseer_runner::claude_code::steer_frame),
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
            Channel::OneShot,
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

    #[test]
    fn a_channel_decides_the_stdin_and_what_ends_the_read_loop() {
        // The three questions that used to be answered by one `Option<fn>`.
        assert!(matches!(Channel::OneShot.stdin_mode(), StdinMode::Closed));
        assert!(matches!(
            Channel::Steered(farseer_runner::claude_code::steer_frame).stdin_mode(),
            StdinMode::Live
        ));
        assert!(matches!(Channel::Acp.stdin_mode(), StdinMode::Live));

        // Only a conversational runner stops early, because only it stays alive
        // after the work is done.
        assert!(Channel::Acp.ends_at_terminal());
        assert!(!Channel::OneShot.ends_at_terminal());
        assert!(!Channel::Steered(farseer_runner::claude_code::steer_frame).ends_at_terminal());
    }

    #[test]
    fn an_acp_runner_name_means_an_executable_and_a_subcommand() {
        // `10 runner inventory` only ever carried bare executables. `goose` and
        // `goose-acp` are one binary offering two faces, and they are separate
        // runners because they report different things - the ACP face names a
        // context window and the native one does not.
        assert_eq!(ACP_RUNNERS[0], ("goose-acp", "goose", "acp"));
        assert!(
            ACP_RUNNERS.iter().all(|(name, exe, _)| name != exe),
            "an ACP runner is never just its executable name"
        );
    }

    #[test]
    fn an_unknown_runner_is_still_unsupported_after_the_acp_list_is_consulted() {
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "irrelevant".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "goose-acp-typo".into(),
            tool_grants: vec![],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: String::new(),
        };
        let contract = WorkerContract::seal(spec);
        let result = start_worker(
            &contract,
            &std::env::temp_dir(),
            LivenessThresholds::default(),
            &RunOptions::default(),
        );
        assert!(matches!(result, Err(ManagerError::UnsupportedRunner(_))));
    }

    /// Live, end to end, through the same `run_worker` an HTTP request reaches.
    ///
    /// Claude Code is deliberately not involved: the operator uses it
    /// interactively and farseer competing for that session is a conflict
    /// farseer should not create.
    ///
    /// Run with: `cargo test -p farseer-manager acp -- --ignored --nocapture`
    #[test]
    #[ignore = "spawns a real `goose acp` and spends a subscription"]
    fn an_acp_run_reaches_the_record_with_a_context_window() {
        let store = Store::open_in_memory().unwrap();
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "Say hello in one short sentence.".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "goose-acp".into(),
            tool_grants: vec![],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: String::new(),
        };
        let run_id = spec.run_id;
        let contract = WorkerContract::seal(spec);

        let mut tick = 100i64;
        let report = run_worker(
            &store,
            &contract,
            &std::env::current_dir().unwrap(),
            LivenessThresholds::default(),
            &RunOptions::default(),
            || {
                tick += 1;
                tick
            },
            |_, _, steer| {
                assert!(
                    steer.is_none(),
                    "an ACP steer needs a session id, which no `fn(&str) -> String` carries"
                );
            },
        )
        .expect("the run completes rather than waiting for an EOF that never comes");

        assert_eq!(report.outcome, Outcome::Ok);
        let events = store
            .scan(0, 200, &farseer_store::ScanFilter::run(run_id))
            .unwrap();
        let kinds: Vec<&str> = events.iter().map(|event| event.kind.as_str()).collect();
        assert!(
            kinds.contains(&EventKind::USAGE_UPDATED),
            "the denominator is the reason this runner exists, and it must reach the record: {kinds:?}"
        );
        assert!(kinds.contains(&EventKind::MANAGER_ANSWERED), "{kinds:?}");
    }
}
