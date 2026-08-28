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
use std::sync::atomic::{AtomicI64, Ordering};
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
    /// Append a window transition, per `27 quota accounting`'s on-change rule.
    ///
    /// Here rather than only at the end of a run because **a window is a fact
    /// about the machine, not about the run**. A manager stays open for as long
    /// as the operator is talking to it, and a quota surface that waits for the
    /// run to finish shows nothing during exactly the period the operator is
    /// spending. Same correction `28 operator surface` already made for the
    /// context window.
    fn observe_window(
        &self,
        cell_id: &farseer_core::CellId,
        run_id: farseer_core::RunId,
        observation: &farseer_core::WindowObservation,
        ts: i64,
    ) -> Result<bool, StoreError>;
}

impl RunSink for Store {
    fn append(&self, event: &NewEvent) -> Result<Seq, StoreError> {
        Store::append(self, event)
    }

    fn upsert_run(&self, row: &RunRow) -> Result<(), StoreError> {
        Store::upsert_run(self, row)
    }

    fn observe_window(
        &self,
        cell_id: &farseer_core::CellId,
        run_id: farseer_core::RunId,
        observation: &farseer_core::WindowObservation,
        ts: i64,
    ) -> Result<bool, StoreError> {
        Store::observe_window(self, cell_id, run_id, observation, ts)
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
    /// Every window the runner reported, when it reports more than one.
    ///
    /// `window` above is Claude Code's single one. `30 codex app server` found a
    /// runner reporting a five-hour **and** a weekly, so this carries them out
    /// whole and the layer that knows the account fills in the two fields the
    /// adapter deliberately left empty.
    pub windows: Vec<farseer_core::WindowObservation>,
    /// What the runner said about the session, if anything.
    ///
    /// Boxed for the same reason `window` is, and it grew a third field before
    /// it needed to be: `ManagerError::Cancelled` carries a whole report, so a
    /// rarely-present observation must not widen every `Result` this crate
    /// returns.
    ///
    /// `RunRow.model` existed from the beginning and was **always empty**,
    /// because nothing read the one line where a runner names its model.
    pub session: Option<Box<farseer_runner::claude_code::SessionInfo>>,
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
    ///
    /// Not Claude's any more. `31 manager delegation reach` found this was
    /// built for one runner and skipped for the rest, so a manager on any other
    /// runner never learned it had a roster - and one asked to delegate did the
    /// work itself and described it as delegation. Every runner that has
    /// somewhere to put it now gets it: pi and omp on the argv, the Codex
    /// app-server as `developerInstructions`, Claude Code as before.
    pub append_system_prompt: Option<String>,
    /// Which subscription this run spends, as the **operator declared it** in
    /// runner config.
    ///
    /// Passed in rather than derived here: `27 quota accounting` is explicit
    /// that the account is declared and never inferred, and this crate has no
    /// business reading config. Absent means the caller did not say, and a
    /// window observed without one is dropped rather than filed under a guess.
    pub account: Option<String>,
    /// The model the operator pinned for this runner in `runners.toml`, or
    /// `None` to leave the runner on its own configuration.
    ///
    /// Passed in for the same reason as [`Self::account`]: this crate reads no
    /// config, and `30 codex app server` settled that farseer never invents a
    /// value the operator did not write down.
    pub model: Option<String>,
    /// The effort the operator pinned, in the runner's own vocabulary.
    pub effort: Option<String>,
    /// Extra environment for the runner process, added to what farseer
    /// inherited.
    ///
    /// A manager on a runner with no MCP client gets its delegation endpoint
    /// and token here, because `31 manager delegation reach`'s fix has to hand
    /// a credential to an extension without putting it on the argv - visible to
    /// every process listing - or in the prompt, where the model would read it.
    pub runner_env: Vec<(String, String)>,
    /// Absolute paths to runner extensions farseer itself supplies.
    ///
    /// Separate from [`Self::skills`] because they are different grants: a
    /// skill is instructions, an extension is code that registers tools. The
    /// only one today is the pi/omp delegation extension of `31 manager
    /// delegation reach`, and the list is explicit so a run never loads one
    /// farseer did not hand it.
    pub extensions: Vec<PathBuf>,
    /// Absolute paths to the skill directories this run may load, resolved by
    /// the caller from the cell's declared skill names.
    ///
    /// Paths rather than names, for the same reason [`Self::account`] is a
    /// string rather than a lookup: this crate reads no config and knows no
    /// cell layout. Empty means the run loads **no** skills, which is the
    /// deliberate default - `32 harness capability floor` found every harness
    /// discovering the operator's own installed skills, and a run bounded by
    /// `12 autonomy and deny list` should not silently inherit them.
    pub skills: Vec<PathBuf>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            actor: Actor::Operator,
            role: RunRole::Worker,
            manager_cell: None,
            claude_mcp_config: None,
            append_system_prompt: None,
            account: None,
            model: None,
            effort: None,
            runner_env: Vec::new(),
            extensions: Vec::new(),
            skills: Vec::new(),
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
    /// An ACP agent **does not exit when the turn ends** - the session stays
    /// open for the next prompt - so what ends the read loop depends on whether
    /// anything intends to send one:
    ///
    /// - a **worker** has one goal, so the loop ends at the terminal signal.
    ///   Waiting for EOF instead waits forever, which is how
    ///   `29 harness protocol`'s first live run hung.
    /// - a **manager** is a conversation, so the loop stays open exactly as a
    ///   live Claude Code manager's does, and ends when the process does.
    ///
    /// The same distinction the native runners draw with `live_input`, drawn
    /// here by what the session is *for* rather than by a flag on the argv.
    Acp { manager: bool },
    /// The Codex app-server, per `30 codex app server`.
    ///
    /// Conversational for the same reason as [`Self::Acp`] and by the same
    /// rules - a thread outlives its turn - but a different handshake:
    /// `initialize`, then an `initialized` **notification** the server will not
    /// proceed without, then `thread/start`, then the goal as `turn/start`.
    CodexAppServer { manager: bool },
    /// pi's headless RPC mode, per [`farseer_runner::pi`].
    ///
    /// Conversational like the two above and ended by the same rule, but with
    /// **no handshake at all**: pi's first line is the goal. The one line
    /// farseer writes before it is `get_state`, which is a question rather than
    /// a negotiation - farseer would run identically without it, and asks only
    /// so `10 runner inventory`'s observed-never-advertised rule has something
    /// to observe.
    PiRpc { manager: bool },
}

impl Channel {
    fn stdin_mode(self) -> StdinMode {
        match self {
            // A goal or a handshake has to reach it.
            Self::Steered(_)
            | Self::Acp { .. }
            | Self::CodexAppServer { .. }
            | Self::PiRpc { .. } => StdinMode::Live,
            Self::OneShot => StdinMode::Closed,
        }
    }

    /// Whether the read loop stops at the terminal signal instead of at end of
    /// stream.
    fn ends_at_terminal(self) -> bool {
        matches!(
            self,
            Self::Acp { manager: false }
                | Self::CodexAppServer { manager: false }
                | Self::PiRpc { manager: false }
        )
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
    /// Set once the ACP handshake has run: the session a steer is addressed to,
    /// and what the agent said about itself while opening it. `None` for every
    /// other channel, and for an ACP run that has not been bootstrapped yet.
    acp_opened: Option<farseer_runner::acp::SessionOpened>,
    /// The next unused JSON-RPC request id, shared with any [`SteerHandle`] this
    /// worker hands out so two writers never collide on one.
    acp_next_id: Arc<AtomicI64>,
    /// The Codex app-server thread this run is talking to, once started, and
    /// what that runner is configured to bring to it.
    codex_thread: Option<farseer_runner::codex_app_server::ThreadOpened>,
    /// What the operator pinned in `runners.toml`, for the protocols that carry
    /// it in a frame rather than on the argv. `None` leaves the runner on its
    /// own configuration, per `30 codex app server`.
    pinned_model: Option<String>,
    pinned_effort: Option<String>,
    /// Manager identity and roster, for the protocols that carry it in a frame
    /// rather than on the argv. See [`RunOptions::append_system_prompt`].
    identity: Option<String>,
}

/// A cloneable handle that writes a steer message into a run's live process,
/// verified 2026-08-23 against `claude_code::steer_frame`'s wire shape.
/// `None` from [`StartedWorker::steer_handle`] rather than an instance of
/// this means the runner has no steering path at all - refuse the request
/// rather than writing a line nothing reads.
#[derive(Clone)]
pub struct SteerHandle {
    stdin: StdinHandle,
    wire: SteerWire,
}

/// How a steer message becomes a line on the wire.
///
/// A native steer is a pure function of the text. An **ACP steer is not**: it is
/// another `session/prompt`, which needs the session the handshake opened and a
/// request id nobody else has used. That is why this is an enum rather than the
/// `fn(&str) -> String` it started as - the second case cannot be expressed as
/// one, and pretending otherwise is how a handle that writes into nothing gets
/// built.
#[derive(Clone)]
enum SteerWire {
    Native(fn(&str) -> String),
    Acp {
        session: String,
        /// Shared with the process's own bootstrap, so a steer never reuses the
        /// id the goal was sent under.
        next_id: Arc<AtomicI64>,
    },
}

impl SteerHandle {
    pub fn steer(&self, message: &str) -> std::io::Result<()> {
        let line = match &self.wire {
            SteerWire::Native(frame) => frame(message),
            SteerWire::Acp { session, next_id } => farseer_runner::acp::prompt_frame(
                next_id.fetch_add(1, Ordering::Relaxed),
                session,
                message,
            ),
        };
        self.stdin.write_line(&line)
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
        env: &[(String, String)],
        thresholds: LivenessThresholds,
        parse: fn(&str) -> Result<Vec<RunnerSignal>, ParseError>,
        channel: Channel,
    ) -> Result<Self, ManagerError> {
        Ok(Self {
            proc: SupervisedProcess::spawn(exe, args, cwd, env, channel.stdin_mode())?,
            activity: Arc::new(Mutex::new(ActivityClock::started_at(0))),
            monotonic_start: Instant::now(),
            thresholds,
            parse,
            channel,
            acp_opened: None,
            acp_next_id: Arc::new(AtomicI64::new(1)),
            codex_thread: None,
            pinned_model: None,
            pinned_effort: None,
            identity: None,
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
            Channel::Steered(frame) => Some(SteerHandle {
                stdin,
                wire: SteerWire::Native(frame),
            }),
            // pi addresses a steer to nothing - it lands in whatever turn is
            // running - so the native wire carries it with no session id and no
            // handshake to wait for. `steer` rather than `follow_up`, because
            // `20 worker control channel` is about reaching a run **in flight**
            // and pi queues a follow-up until the turn ends.
            Channel::PiRpc { manager: true } => Some(SteerHandle {
                stdin,
                wire: SteerWire::Native(farseer_runner::pi::steer_frame),
            }),
            // Only after the handshake: the session id is what a steer is
            // addressed to, and `start_worker` bootstraps before any caller can
            // reach this. A worker gets nothing - `20 worker control channel`
            // made steering the exception, and a worker has one goal.
            Channel::Acp { manager: true } => self.acp_opened.as_ref().map(|opened| SteerHandle {
                stdin,
                wire: SteerWire::Acp {
                    session: opened.session_id.clone(),
                    next_id: Arc::clone(&self.acp_next_id),
                },
            }),
            // `turn/steer` takes an `expectedTurnId`, which farseer does not
            // track yet - and `30 codex app server` records that using it at all
            // is a **correction to `20 worker control channel`**, whose
            // turn-boundary finding this would be the first evidence against.
            // That is a decision, not a wiring detail.
            Channel::CodexAppServer { .. }
            | Channel::Acp { manager: false }
            | Channel::PiRpc { manager: false }
            | Channel::OneShot => None,
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
            Channel::Acp { .. } => {
                let mut next_id = self.acp_next_id.load(Ordering::Relaxed);
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
                let goal_id = next_id;
                // Bumped before the goal is sent, so a steer arriving the
                // instant the handle becomes reachable cannot reuse this id.
                self.acp_next_id.store(goal_id + 1, Ordering::Relaxed);
                farseer_runner::acp_drive::prompt_on(
                    &self.proc,
                    &opened.session_id,
                    goal_id,
                    goal,
                )?;
                self.acp_opened = Some(opened);
            }
            Channel::PiRpc { .. } => {
                // No handshake: the goal is the first thing pi needs. The
                // `get_state` ahead of it is a question whose answer becomes a
                // `Session` signal in the read loop - asked rather than assumed,
                // so the model and effort on the operator's surface are pi's
                // claim about itself rather than farseer's launch flags read
                // back as if they were an observation.
                self.proc
                    .write_line(&farseer_runner::pi::get_state_frame())?;
                self.proc
                    .write_line(&farseer_runner::pi::prompt_frame(goal))?;
            }
            Channel::CodexAppServer { .. } => {
                let mut ids = farseer_runner::jsonrpc::Ids::starting_at(
                    self.acp_next_id.load(Ordering::Relaxed) - 1,
                );
                let mut discard = |_: Result<Vec<RunnerSignal>, ParseError>| {};
                let identity = self.identity.clone();
                let opened = farseer_runner::codex_app_server::handshake(
                    &mut self.proc,
                    cwd,
                    // `12 autonomy and deny list` decides reach before the run.
                    // `10 runner inventory` measured that this flag did not stop
                    // a write on this machine, so it is a request and the
                    // worktree is still the guarantee.
                    CODEX_SANDBOX,
                    identity.as_deref(),
                    &mut ids,
                    &mut discard,
                )?;
                let goal_id = ids.next();
                self.acp_next_id.store(goal_id + 1, Ordering::Relaxed);
                self.proc
                    .write_line(&farseer_runner::codex_app_server::turn_start_frame(
                        goal_id,
                        &opened.thread_id,
                        goal,
                        // Only what the operator pinned in `runners.toml`.
                        // `30 codex app server` decided farseer never sends a
                        // value of its own - a farseer that did would silently
                        // downgrade every run they had configured up - and two
                        // `None`s here still mean exactly that. What changed is
                        // that the operator now has somewhere to say otherwise.
                        self.pinned_model.as_deref(),
                        self.pinned_effort.as_deref(),
                    ))?;
                self.codex_thread = Some(opened);
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
        account: Option<&str>,
        // When the row this run already wrote says it started, so an update
        // mid-run does not move it.
        started_ts: i64,
    ) -> Result<RunReport, ManagerError> {
        let mut report = None;
        let mut output = None;
        // Fragments of an answer still being written. Emptied into one
        // `manager_answered` when the turn ends - see `RunnerSignal::OutputChunk`.
        let mut chunks = String::new();
        // Spend from legs that ended without being the end. See `LegSpend`.
        let mut carried_cost: Option<i64> = None;
        let mut carried_tokens: Option<i64> = None;
        let mut window = None;
        let mut windows: Vec<farseer_core::WindowObservation> = Vec::new();
        let account = account.map(str::to_string);
        let mut session: Option<farseer_runner::claude_code::SessionInfo> = None;
        let mut store_err = None;
        let cancel_on_store_failure = self.proc.cancel_token();
        let activity = Arc::clone(&self.activity);
        let monotonic_start = self.monotonic_start;

        // What the ACP handshake learned, replayed into the read loop as though
        // the agent had announced it mid-stream - which is how Claude Code and
        // Codex report the same facts. The handshake happens in `bootstrap`,
        // before any store is in reach, so this is the first moment it can be
        // recorded rather than a second way of recording it.
        let codex_thread =
            self.codex_thread
                .take()
                .map(|opened| farseer_runner::claude_code::SessionInfo {
                    // Observed stays observed: the app-server names no model for
                    // the turn, and the configured one is a hint rather than a
                    // report of what ran. Different fields for that reason.
                    model: None,
                    provider: None,
                    configured: opened.configured,
                    session_id: Some(opened.thread_id),
                });
        let opened = self.acp_opened.take().map(|opened| {
            farseer_runner::claude_code::SessionInfo {
                // Not from `session/set_model`, which `29 harness protocol`
                // found unstable upstream and broken in shipped clients - from
                // the agent's own `configOptions`. `opencode acp` names a model
                // there and no provider; `goose acp` does the reverse.
                model: opened.model().map(str::to_string),
                provider: opened.provider().map(str::to_string),
                // ACP exposes settings but no notion of a default farseer could
                // separate from a current value, so there is no hint to give.
                configured: None,
                session_id: Some(opened.session_id),
            }
        });
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
                    // Activity, and nothing more, until the turn ends.
                    RunnerSignal::OutputChunk(text) => chunks.push_str(&text),
                    // A leg that spent money and was not the end. Held here
                    // and added at the terminal one, because a run report that
                    // carries only the final leg under-counts every omp run
                    // that used a background job - see `32 harness capability
                    // floor` and `11 analytics questions`.
                    RunnerSignal::LegSpend {
                        cost_usd_micros,
                        tokens,
                    } => {
                        if let Some(micros) = cost_usd_micros {
                            carried_cost = Some(carried_cost.unwrap_or(0) + micros);
                        }
                        if let Some(count) = tokens {
                            carried_tokens = Some(carried_tokens.unwrap_or(0) + count);
                        }
                    }
                    RunnerSignal::Finished(f) => {
                        if !chunks.trim().is_empty() {
                            let event = NewEvent::new(
                                contract.cell_id.clone(),
                                contract.run_id,
                                EventKind::new(EventKind::MANAGER_ANSWERED),
                                progress_actor,
                                now_ms(),
                                serde_json::json!({ "text": chunks }),
                            );
                            if let Err(e) = sink.append(&event) {
                                store_err = Some(e);
                                cancel_on_store_failure.cancel();
                                return;
                            }
                            output = Some(std::mem::take(&mut chunks));
                        }
                        // `None + None` stays `None`: a runner that reports
                        // no spend must not be made to look like it reported
                        // zero, which is `10 runner inventory`'s observed-
                        // never-advertised rule applied to money.
                        let total = |leg: Option<i64>, carried: Option<i64>| match (leg, carried) {
                            (None, None) => None,
                            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
                        };
                        report = Some(RunReport {
                            outcome: f.outcome,
                            cost_usd_micros: total(f.cost_usd_micros, carried_cost),
                            tokens: total(f.tokens, carried_tokens),
                            result: output.clone(),
                            // Attached after the stream ends: `10 runner
                            // inventory` observed `rate_limit_event` arriving
                            // around the terminal result, not before it.
                            window: None,
                            windows: Vec::new(),
                            session: None,
                        });
                    }
                    // `27 quota accounting`: observed on every successful run,
                    // carried out on the report, and appended by the layer that
                    // knows which account it belongs to - on change only.
                    RunnerSignal::RateLimit(info) => window = Some(Box::new(info)),
                    // The latest snapshot wins rather than accumulating: each
                    // notification restates every window, so keeping them all
                    // would be the repetition `27 quota accounting` section 4
                    // built append-on-change to avoid.
                    RunnerSignal::Windows(reported) => {
                        // Recorded now rather than at the end of the run. A
                        // manager stays open for as long as the operator is
                        // talking to it, so waiting would leave the quota
                        // surface empty during exactly the period being spent.
                        //
                        // The store's own on-change guard makes this idempotent,
                        // which is what lets the end-of-run path stay as it is.
                        for observation in &reported {
                            let Some(account) = account.as_ref() else {
                                // Nobody said which subscription this is. `27`
                                // declares accounts and never infers them, so an
                                // unattributed window is dropped rather than
                                // filed under a guess.
                                break;
                            };
                            let mut observation = observation.clone();
                            observation.account = account.clone();
                            observation.runner = contract.runner.clone();
                            if let Err(e) = sink.observe_window(
                                &contract.cell_id,
                                contract.run_id,
                                &observation,
                                now_ms(),
                            ) {
                                store_err = Some(e);
                                cancel_on_store_failure.cancel();
                                return;
                            }
                        }
                        windows = reported;
                    }
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
                                "provider": info.provider,
                                "runner": contract.runner,
                                // Hints, named as hints on the wire so a reader
                                // cannot mistake them for what the turn used.
                                "configured_model": info.configured.as_ref().and_then(|c| c.model.clone()),
                                "configured_effort": info.configured.as_ref().and_then(|c| c.effort.clone()),
                                "configured_from": info.configured.as_ref().and_then(|c| c.from.clone()),
                            }),
                        );
                        if let Err(e) = sink.append(&event) {
                            store_err = Some(e);
                            cancel_on_store_failure.cancel();
                            return;
                        }
                        // The row too, not only the event. `finished_row` fills
                        // the model from this same session at the end of the
                        // run, which is the third time that shape has been
                        // wrong for the same reason: **a manager never ends**,
                        // so a fact recorded only at the end is a fact the
                        // operator never sees while it matters. Same correction
                        // `28 operator surface` made for the context window and
                        // `27 quota accounting` made for the window itself.
                        if let Some(model) = info.model.clone() {
                            let row = row(
                                contract,
                                None,
                                0,
                                0,
                                started_ts,
                                None,
                                model,
                            );
                            if let Err(e) = sink.upsert_run(&row) {
                                store_err = Some(e);
                                cancel_on_store_failure.cancel();
                                return;
                            }
                        }
                        session = Some(info);
                    }
                }
            }
        };

        if let Some(info) = opened.or(codex_thread) {
            on_line(Ok(vec![RunnerSignal::Session(info)]));
        }

        if ends_at_terminal {
            // A conversational runner stays alive after the work is done, so
            // end of stream never comes. `29 harness protocol`'s first live run
            // waited for it anyway and hung.
            while let Some(line) = self.proc.read_line()? {
                let parsed = (self.parse)(&line);
                let ended = parsed.as_ref().is_ok_and(|signals| {
                    signals
                        .iter()
                        .any(|signal| matches!(signal, RunnerSignal::Finished(_)))
                });
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
                report.windows = std::mem::take(&mut windows);
                report.session = session.clone().map(Box::new);
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
                windows,
                session: session.map(Box::new),
            })),
            None => Err(ManagerError::NoResult),
        }
    }
}

/// What farseer can actually do with a runner, as measured rather than claimed.
///
/// Every field is a **proven** capability: something farseer has driven against
/// the real binary and left a test behind for. A missing one is not a defect in
/// the runner - it is a thing the operator should know before putting that
/// runner in front of a cell, because it changes what the surface can offer.
///
/// Nothing is hidden or refused on the strength of this. `13 harness build kit`
/// found the inventory is a **menu rather than a survey**, and a menu that
/// silently drops entries teaches the operator less than one that says why an
/// entry is dimmer than its neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Control {
    /// Farseer can put words into a run that is already going, per
    /// `20 worker control channel`.
    pub steer: bool,
    /// The runner reports its subscription window, so `27 quota accounting` can
    /// see exhaustion coming rather than discovering it by failing.
    pub quota: bool,
    /// Where a cost figure comes from, when there is one.
    ///
    /// `27 quota accounting` refused a **derived** percentage where a reported
    /// one existed, and this is the same distinction for money. Two runners
    /// report a dollar figure and they do not mean the same thing: Claude Code
    /// passes on what the provider charged, while pi and omp price every
    /// message from their own per-model table. On a subscription nobody is
    /// charged per token at all, so pi's number is **what this would have cost
    /// at list price** - which is worth reporting, and worth never being
    /// summed together with money that actually moved.
    pub cost: CostBasis,
    /// The runner names its context window, so "how full is this" has a
    /// denominator - see `29 harness protocol`.
    pub context: bool,
    /// The runner says when it compacted, so `02 record scope` can record that
    /// a result was produced from a summary rather than from the whole thread.
    pub compaction: bool,
}

/// What farseer has proven it can do with each runner.
///
/// Cancellation is absent because it is farseer's, not the runner's: the Job
/// Object kill in `03 spike job objects` works on anything with a process id,
/// and no runner has to agree to it.
/// Where a runner's cost figure comes from. See [`Control::cost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostBasis {
    /// The runner says nothing about money. Most of the inventory.
    Silent,
    /// The provider told the runner what it charged, and the runner passed it
    /// on. Money that moved.
    Reported,
    /// The runner multiplied its own token counts by its own price table.
    /// Accurate as an API list price and **not** what a subscription was
    /// billed - which makes it the number that says what the subscription
    /// saved, as long as nothing presents it as spend.
    ListPrice,
}

impl CostBasis {
    /// How to describe this to an operator, in six words or fewer.
    pub fn note(self) -> Option<&'static str> {
        match self {
            Self::Silent => None,
            Self::Reported => Some("as charged by the provider"),
            Self::ListPrice => Some("at list price, not billed"),
        }
    }
}

pub fn control_of(runner: &str) -> Control {
    match runner {
        // `10 runner inventory`: quota and cost both arrive in band, and
        // `system/compact_boundary` names a compaction. No context window - it
        // reports tokens with nothing to divide them by.
        "claude-code" => Control {
            cost: CostBasis::Reported,
            steer: true,
            quota: true,
            context: false,
            compaction: true,
        },
        // `30 codex app server`: two quota windows with the provider's own
        // percentage, a real `modelContextWindow`, and `thread/compacted`.
        // Steering needs an `expectedTurnId` farseer does not track yet, and
        // `30` records using it at all as a correction to `20`.
        "codex-app-server" => Control {
            cost: CostBasis::Silent,
            steer: false,
            quota: true,
            context: true,
            compaction: true,
        },
        // `29 harness protocol`: agy is one-shot and says so at both ends -
        // `-p` runs a prompt and exits, and `--continue` starts a new process
        // the way Codex and cursor-agent do. It names its model and its tokens
        // and nothing else, which is the honest floor rather than a gap.
        "agy" => Control {
            cost: CostBasis::Silent,
            steer: false,
            quota: false,
            context: false,
            compaction: false,
        },
        // `29 harness protocol`: pi steers natively, prices its own messages,
        // and brackets a compaction. No quota: pi calls whichever provider the
        // operator configured, and a subscription window is the provider's fact
        // rather than pi's. No context window either - `get_state` knows the
        // denominator but the event stream never sends it, and half an answer is
        // the thing `10 runner inventory`'s rule exists to refuse.
        runner if PI_RUNNERS.iter().any(|(name, _)| *name == runner) => Control {
            // pi prices its own messages from its own table. Real, and not
            // what a ChatGPT subscription was billed.
            cost: CostBasis::ListPrice,
            steer: true,
            quota: false,
            context: false,
            compaction: true,
        },
        // `29 harness protocol`: an ACP agent names a context window and steers
        // as a manager, and ACP has no quota concept and no compaction boundary
        // at all - the trade is the protocol's, not the agent's.
        runner if ACP_RUNNERS.iter().any(|(name, _, _)| *name == runner) => Control {
            cost: CostBasis::Silent,
            steer: true,
            quota: false,
            context: true,
            compaction: false,
        },
        // Everything else is one-shot and silent about all four:
        // `codex exec`, `cursor-agent`, `goose`. They run, they answer, they exit.
        _ => Control {
            cost: CostBasis::Silent,
            steer: false,
            quota: false,
            context: false,
            compaction: false,
        },
    }
}

/// What sandbox farseer asks the Codex app-server for.
///
/// A request rather than a guarantee: `10 runner inventory` measured Codex's own
/// `--sandbox read-only` failing to prevent a write on this machine, so the
/// worktree remains the boundary and this is defence in depth.
pub const CODEX_SANDBOX: &str = "read-only";

/// The ACP runners, as `name -> (executable, subcommand)`.
///
/// Both were verified installed and speaking ACP on 2026-08-26. The list is
/// short on purpose: an entry here is a claim that farseer has **seen this
/// agent's output**, which is `10 runner inventory`'s rule, not a claim that
/// ACP agents in general work.
/// The runners speaking pi's RPC mode, as `name -> executable`.
///
/// `omp` is a superset of `pi` that speaks the **same protocol verbatim** - the
/// 2026-08-27 probe drove it with pi's own frames and got pi's own events back,
/// plus a `ready` handshake and `extension_ui_request` widget calls that farseer
/// ignores. So it shares [`farseer_runner::pi`] rather than getting a copy.
///
/// They are still two runners, for `29 harness protocol`'s reason: omp bundles
/// **task agents**, which is the subagent capability pi does not have, and a
/// single name would hide the difference the operator is choosing between.
pub const PI_RUNNERS: [(&str, &str); 2] = [("pi", "pi"), ("omp", "omp")];

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
                        append_system_prompt: options.append_system_prompt.as_deref(),
                        edits_granted: options
                            .manager_cell
                            .as_ref()
                            .is_some_and(|cell| cell.has_shell_grant()),
                    },
                ),
                cwd,
                &options.runner_env,
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
                &options.runner_env,
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
                &options.runner_env,
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
                &options.runner_env,
                thresholds,
                farseer_runner::goose::parse_line,
                // `-r/--resume` restarts into a new process rather than
                // continuing this one, and no `--input-format`-style flag
                // exists, per this crate's own 2026-08-24 probe.
                Channel::OneShot,
            )
        }
        // The richest face of a runner farseer already drove, per
        // `30 codex app server`. A separate runner from `codex` for the reason
        // `29 harness protocol` set: same binary, different faces, and they
        // report different things - this one names a context window, a
        // compaction boundary and two quota windows.
        "codex-app-server" => {
            let exe =
                resolve("codex").ok_or_else(|| ManagerError::ExecutableNotFound("codex".into()))?;
            StartedWorker::spawn(
                &exe,
                &["app-server".to_string()],
                cwd,
                &options.runner_env,
                thresholds,
                farseer_runner::codex_app_server::parse_line,
                Channel::CodexAppServer {
                    manager: options.role == RunRole::Manager,
                },
            )
        }
        // pi's RPC mode, per `29 harness protocol` and [`farseer_runner::pi`].
        // The only runner farseer launches with a model on the argv, because it
        // is the only one whose model the operator can pin in `runners.toml`
        // and have farseer pass without overriding a value they set elsewhere.
        // agy, per `29 harness protocol`. One-shot, so the goal goes on the
        // argv and stdin closes at spawn - the same shape as `codex exec`.
        "agy" => {
            let exe =
                resolve("agy").ok_or_else(|| ManagerError::ExecutableNotFound("agy".into()))?;
            StartedWorker::spawn(
                &exe,
                &farseer_runner::agy::build_args(&contract.goal, options.model.as_deref()),
                cwd,
                &options.runner_env,
                thresholds,
                farseer_runner::agy::parse_line,
                Channel::OneShot,
            )
        }
        // pi and omp, per `29 harness protocol` and [`farseer_runner::pi`].
        // The only runners farseer launches with a model on the argv, because
        // they are the only ones whose model the operator can pin without
        // overriding a value they set elsewhere.
        other if PI_RUNNERS.iter().any(|(name, _)| *name == other) => {
            let exe =
                resolve(other).ok_or_else(|| ManagerError::ExecutableNotFound(other.into()))?;
            StartedWorker::spawn(
                &exe,
                &farseer_runner::pi::build_args(
                    other,
                    options.model.as_deref(),
                    options.effort.as_deref(),
                    &options.skills,
                    &options.extensions,
                    options.append_system_prompt.as_deref(),
                ),
                cwd,
                &options.runner_env,
                thresholds,
                farseer_runner::pi::parse_line,
                Channel::PiRpc {
                    manager: options.role == RunRole::Manager,
                },
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
                    &options.runner_env,
                    thresholds,
                    farseer_runner::acp::parse_line,
                    Channel::Acp {
                        manager: options.role == RunRole::Manager,
                    },
                )
            }
            None => return Err(ManagerError::UnsupportedRunner(other.to_string())),
        },
    }?;
    let mut started = started;
    // Set before the handshake rather than passed through it: a protocol that
    // carries the model in a frame (the Codex app-server) and one that carries
    // it on the argv (pi) need the same value at different moments, and the
    // operator declared it once.
    started.pinned_model = options.model.clone();
    started.pinned_effort = options.effort.clone();
    started.identity = options.append_system_prompt.clone();
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
        started.run_to_completion(
            sink,
            contract,
            progress_actor,
            &mut now_ms,
            options.account.as_deref(),
            started_ts,
        )
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
            &[],
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
            .run_to_completion(&store, &contract, Actor::Worker, || 1, None, 1)
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
            .run_to_completion(&store, &contract, Actor::Worker, || 1, None, 1)
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
            windows: Vec::new(),
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
            windows: Vec::new(),
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
            windows: Vec::new(),
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
            windows: Vec::new(),
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

    /// omp's shape, as signals: a background-job leg that spends and does not
    /// end, then a terminal leg carrying only its own.
    fn two_leg_fixture(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
        Ok(match line {
            "leg" => vec![RunnerSignal::LegSpend {
                cost_usd_micros: Some(4000),
                tokens: Some(900),
            }],
            "done" => vec![RunnerSignal::Finished(
                farseer_runner::claude_code::FinishedSignal {
                    outcome: Outcome::Ok,
                    cost_usd_micros: Some(1000),
                    tokens: Some(100),
                },
            )],
            _ => Vec::new(),
        })
    }

    /// The run report used to carry the **final leg only**, so an omp run that
    /// delegated to a background job reported a fraction of what it spent -
    /// which `11 analytics questions` reads, and a budget draws down against.
    #[test]
    fn a_run_report_carries_every_leg_that_spent_not_only_the_last() {
        let store = Store::open_in_memory().unwrap();
        let contract = contract();
        let dir = tempfile::tempdir().unwrap();
        let lines = dir.path().join("legs.txt");
        std::fs::write(&lines, "leg\r\ndone\r\n").unwrap();

        let started = StartedWorker::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &[
                "/d".into(),
                "/c".into(),
                "type".into(),
                lines.to_string_lossy().into_owned(),
            ],
            &std::env::current_dir().unwrap(),
            &[],
            LivenessThresholds::default(),
            two_leg_fixture,
            Channel::OneShot,
        )
        .unwrap();

        let report = started
            .run_to_completion(&store, &contract, Actor::Worker, || 1, None, 1)
            .expect("the run completes");
        assert_eq!(report.cost_usd_micros, Some(5000));
        assert_eq!(report.tokens, Some(1000));
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
            &[],
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

        let result = started.run_to_completion(&store, &contract, Actor::Worker, || 1, None, 1);
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
            &[],
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
        let result = started.run_to_completion(&store, &contract, Actor::Worker, || 1, None, 1);
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
            &[],
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
            .run_to_completion(&store, &contract, Actor::Worker, || 1, None, 1)
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
        assert!(matches!(
            Channel::Acp { manager: false }.stdin_mode(),
            StdinMode::Live
        ));

        // Only a conversational runner stops early, because only it stays alive
        // after the work is done - and only when it is a worker, since a manager
        // is meant to be spoken to again.
        assert!(Channel::Acp { manager: false }.ends_at_terminal());
        assert!(!Channel::Acp { manager: true }.ends_at_terminal());
        assert!(!Channel::OneShot.ends_at_terminal());
        assert!(!Channel::Steered(farseer_runner::claude_code::steer_frame).ends_at_terminal());
    }

    #[test]
    fn what_farseer_can_do_with_a_runner_is_recorded_per_runner_not_assumed() {
        // The two faces of one binary differ, which is the whole reason they are
        // separate runners.
        assert!(!control_of("codex").context);
        assert!(control_of("codex-app-server").context);
        assert!(control_of("codex-app-server").quota);

        // Every ACP agent gets the protocol's trade rather than its own: a
        // context window, no quota. `29 harness protocol` measured that on two
        // different agents.
        for (name, _, _) in ACP_RUNNERS {
            let control = control_of(name);
            assert!(control.context, "{name}");
            assert!(!control.quota, "{name}");
        }

        // An unknown runner is assumed to do nothing, which is the safe
        // direction: it under-promises rather than offering a verb that stalls.
        let unknown = control_of("something-nobody-has-driven");
        assert!(!unknown.steer && !unknown.quota && !unknown.context && !unknown.compaction);
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
    fn an_acp_run_on_goose_reaches_the_record_with_a_context_window() {
        one_acp_run("goose-acp");
    }

    /// The second agent, and the reason `ACP_RUNNERS` claims farseer has *seen*
    /// each entry's output rather than that ACP agents work in general.
    ///
    /// `opencode acp` advertises **no modes**, streams a separate
    /// `agent_thought_chunk`, and names a **model** where goose names a
    /// **provider** - three differences a one-agent adapter would have called
    /// the protocol.
    #[test]
    #[ignore = "spawns a real `opencode acp` and spends a subscription"]
    fn an_acp_run_on_opencode_reaches_the_record_with_a_context_window() {
        one_acp_run("opencode-acp");
    }

    /// The third conversational runner, and the first one that is not ACP.
    ///
    /// `30 codex app server` found `codex exec --json` is the cut-down face of a
    /// runner farseer already drove: this one names a **real**
    /// `modelContextWindow`, where the native adapter has no denominator at all.
    #[test]
    #[ignore = "spawns a real `codex app-server` and spends a subscription"]
    fn a_codex_app_server_run_reaches_the_record_with_a_context_window() {
        let report = one_acp_run("codex-app-server");
        // `10 runner inventory` measured this runner as reporting no window at
        // all. It reports two, with the provider's own percentage on each.
        assert_eq!(
            report.windows.len(),
            2,
            "a five-hour and a weekly: {:?}",
            report.windows
        );
        assert!(
            report.windows.iter().all(|w| w.used_percent.is_some()),
            "{:?}",
            report.windows
        );
        // Left blank on purpose: `27 quota accounting` declares the account.
        assert!(report.windows.iter().all(|w| w.account.is_empty()));
    }

    /// Live: the fifth protocol, and the second runner able to report money.
    ///
    /// Not `one_acp_run`, because pi answers a different set of questions: it
    /// reports **cost** where the conversational runners report a context
    /// window, and reports no window at all. Reusing the shared helper would
    /// have meant loosening the assertion that makes the helper worth having -
    /// `29 harness protocol`'s point is that runners differ, and a test that
    /// hides the difference proves nothing.
    ///
    /// Run with: `cargo test -p farseer-manager a_pi_run -- --ignored --nocapture`
    #[test]
    #[ignore = "spawns a real `pi --mode rpc` and spends a subscription"]
    fn a_pi_run_reaches_the_record_with_what_it_cost() {
        let store = Store::open_in_memory().unwrap();
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "Reply with exactly: pi online.".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "pi".into(),
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
            // What the operator pinned in `runners.toml`, passed the way the API
            // passes it. Cheap and shallow on purpose: this test proves the
            // wiring, not the model.
            &RunOptions {
                model: Some("openai-codex/gpt-5.6-luna".into()),
                effort: Some("low".into()),
                ..RunOptions::default()
            },
            || {
                tick += 1;
                tick
            },
            |_, _, _| {},
        )
        .expect("the run completes rather than waiting for an EOF that never comes");

        assert_eq!(report.outcome, Outcome::Ok);
        // The claim `10 runner inventory` made about only one runner. pi prices
        // every message itself, so this is the second.
        assert!(
            report.cost_usd_micros.is_some_and(|micros| micros > 0),
            "pi reports what it spent: {report:?}"
        );
        assert!(report.tokens.is_some_and(|tokens| tokens > 0), "{report:?}");
        // No quota and no denominator, and that is the honest answer rather
        // than an omission - see `control_of("pi")`.
        assert!(report.windows.is_empty(), "{:?}", report.windows);

        let events = store
            .scan(0, 200, &farseer_store::ScanFilter::run(run_id))
            .unwrap();
        let kinds: Vec<&str> = events.iter().map(|event| event.kind.as_str()).collect();
        assert!(kinds.contains(&EventKind::MANAGER_ANSWERED), "{kinds:?}");

        // `get_state` is asked rather than assumed: this is pi describing
        // itself, which is why the effort on the operator's surface is an
        // observation and not farseer reading its own launch flags back.
        let session = events
            .iter()
            .find(|event| event.kind.as_str() == EventKind::SESSION_STARTED)
            .expect("pi answers `get_state` before the goal goes in");
        eprintln!("session_started: {}", session.payload);
        assert_eq!(
            session.payload.get("configured_effort").and_then(|v| v.as_str()),
            Some("low"),
            "the effort the operator pinned, as pi reports it back"
        );
        assert_eq!(
            session.payload.get("model").and_then(|v| v.as_str()),
            Some("gpt-5.6-luna")
        );
        assert_eq!(
            session.payload.get("provider").and_then(|v| v.as_str()),
            Some("openai-codex")
        );
    }

    /// Live: `omp` reached through pi's adapter, which is the claim
    /// `PI_RUNNERS` makes - one protocol, two binaries.
    ///
    /// Run with: `cargo test -p farseer-manager an_omp_run -- --ignored --nocapture`
    #[test]
    #[ignore = "spawns a real `omp --mode rpc` and spends a subscription"]
    fn an_omp_run_reaches_the_record_through_pis_adapter() {
        let report = one_pi_run("omp", Some("gpt-5.6-luna"), Some("low"));
        assert!(report.cost_usd_micros.is_some_and(|micros| micros > 0));
    }

    /// Live: `agy` on Google's own CLI, the sixth event vocabulary.
    ///
    /// Run with: `cargo test -p farseer-manager an_agy_run -- --ignored --nocapture`
    #[test]
    #[ignore = "spawns a real `agy -p` and spends a subscription"]
    fn an_agy_run_reaches_the_record_with_tokens_and_no_money() {
        let report = one_pi_run("agy", Some("gemini-3.7-flash-low"), None);
        // `10 runner inventory`'s floor: tokens, no currency. Asserted rather
        // than assumed, because a runner that started reporting cost would be
        // a fact this test should notice.
        assert!(report.tokens.is_some_and(|tokens| tokens > 0), "{report:?}");
        assert_eq!(report.cost_usd_micros, None, "{report:?}");
    }

    /// Live: what the runner wrote is what the record holds, byte for byte.
    ///
    /// Written after a false alarm worth keeping the guard from: a manager's
    /// typographic apostrophe *appeared* to reach the record double-encoded, and
    /// the corruption turned out to be in the shell one-liner doing the
    /// checking - `curl | python` decodes stdin as cp1252 on Windows. farseer
    /// was correct throughout.
    ///
    /// The test stays because the failure it was chasing is real elsewhere and
    /// silent when it happens: a record that mangles what a runner said cannot
    /// be trusted to quote it, and both ends render as *something* in a
    /// terminal. Two runner families are covered so a regression names its own
    /// side of the spawn - pi is bun, the Codex app-server is a Rust binary.
    ///
    /// It is also a standing reminder that a verification tool is part of the
    /// system under test.
    ///
    /// Run with: `cargo test -p farseer-manager text_reaches -- --ignored --nocapture`
    #[test]
    #[ignore = "spawns real runners and spends a subscription"]
    fn text_reaches_the_record_as_the_runner_wrote_it() {
        for (runner, model) in [
            ("pi", Some("openai-codex/gpt-5.6-luna")),
            ("codex-app-server", None),
        ] {
            let store = Store::open_in_memory().unwrap();
            let spec = WorkerContractSpec {
                run_id: RunId::new(),
                task_id: TaskId::new(),
                cell_id: CellId::new("zero"),
                goal: "Reply with exactly this line and nothing else, using a real typographic \
                       apostrophe (U+2019): it can{APOS}t work"
                    .into(),
                workspace: WorkspaceStrategy::Worktree,
                runner: runner.into(),
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
                &RunOptions {
                    model: model.map(str::to_string),
                    effort: Some("low".into()),
                    ..RunOptions::default()
                },
                || {
                    tick += 1;
                    tick
                },
                |_, _, _| {},
            )
            .expect("the run completes");
            assert_eq!(report.outcome, Outcome::Ok, "{runner}: {report:?}");

            let events = store
                .scan(0, 200, &farseer_store::ScanFilter::run(run_id))
                .unwrap();
            let answer = events
                .iter()
                .find(|e| e.kind.as_str() == EventKind::MANAGER_ANSWERED)
                .and_then(|e| e.payload.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string();
            eprintln!("{runner}: {:?}", answer.as_bytes());
            // The first character of the cp1252 misreading of any UTF-8
            // punctuation. Cheap, and specific enough not to fire on prose.
            assert!(
                !answer.contains('\u{e2}'),
                "{runner} reached the record double-encoded - see `34 record mojibake`: {answer:?}"
            );
        }
    }

    fn one_pi_run(runner: &str, model: Option<&str>, effort: Option<&str>) -> RunReport {
        let store = Store::open_in_memory().unwrap();
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: format!("Reply with exactly: {runner} online."),
            workspace: WorkspaceStrategy::Worktree,
            runner: runner.into(),
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
            &RunOptions {
                model: model.map(str::to_string),
                effort: effort.map(str::to_string),
                ..RunOptions::default()
            },
            || {
                tick += 1;
                tick
            },
            |_, _, _| {},
        )
        .expect("the run completes rather than waiting for an EOF that never comes");

        assert_eq!(report.outcome, Outcome::Ok, "{report:?}");
        let events = store
            .scan(0, 200, &farseer_store::ScanFilter::run(run_id))
            .unwrap();
        let kinds: Vec<&str> = events.iter().map(|event| event.kind.as_str()).collect();
        assert!(kinds.contains(&EventKind::MANAGER_ANSWERED), "{kinds:?}");
        let session = events
            .iter()
            .find(|event| event.kind.as_str() == EventKind::SESSION_STARTED)
            .expect("every runner here names the session it opened");
        eprintln!("{runner} session_started: {}", session.payload);
        if let Some(model) = model {
            // The model actually used, which is what `10 runner inventory`
            // asks for - not the flag farseer sent.
            let reported = session.payload.get("model").and_then(|v| v.as_str());
            assert!(
                reported.is_some_and(|m| model.ends_with(m) || m == model),
                "pinned {model}, ran {reported:?}"
            );
        }
        report
    }

    fn one_acp_run(runner: &str) -> RunReport {
        let store = Store::open_in_memory().unwrap();
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "Say hello in one short sentence.".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: runner.into(),
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
        // The handshake's own answer about itself, replayed into the record.
        let session = events
            .iter()
            .find(|event| event.kind.as_str() == EventKind::SESSION_STARTED)
            .expect("an ACP run names the session it opened");
        eprintln!("session_started: {}", session.payload);
        assert!(
            session
                .payload
                .get("session_id")
                .is_some_and(|v| !v.is_null()),
            "{}",
            session.payload
        );
        report
    }

    /// Live: an ACP **manager** answers, is steered, and answers again on the
    /// same session - which is the whole reason `Channel::Acp` carries a role.
    ///
    /// Claude Code is deliberately not involved, per the operator's standing
    /// request that farseer not compete with their interactive session.
    ///
    /// Run with: `cargo test -p farseer-manager steering_an_acp -- --ignored --nocapture`
    #[test]
    #[ignore = "spawns a real `goose acp` and spends a subscription on two turns"]
    fn steering_an_acp_manager_reaches_the_same_session() {
        // On disk rather than in memory: the watching thread needs its own
        // connection, because `09 store decision`'s single writer is a
        // `rusqlite::Connection` and one cannot be shared across threads.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("record.db");
        let store = Store::open(&db).unwrap();
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

        let said = |store: &Store| -> Vec<String> {
            store
                .scan(0, 200, &farseer_store::ScanFilter::run(run_id))
                .unwrap()
                .iter()
                .filter(|event| event.kind.as_str() == EventKind::MANAGER_ANSWERED)
                .filter_map(|event| {
                    event
                        .payload
                        .get("text")
                        .and_then(|text| text.as_str())
                        .map(str::to_string)
                })
                .collect()
        };
        let answers = |store: &Store| said(store).len();
        let wait_for = |store: &Store, count: usize| {
            let deadline = Instant::now() + std::time::Duration::from_secs(90);
            while answers(store) < count {
                assert!(Instant::now() < deadline, "waited 90s for answer {count}");
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        };

        let mut tick = 100i64;
        let watched = db.clone();
        let result = std::thread::scope(|scope| {
            run_worker(
                &store,
                &contract,
                &std::env::current_dir().unwrap(),
                LivenessThresholds::default(),
                &RunOptions {
                    role: RunRole::Manager,
                    ..RunOptions::default()
                },
                || {
                    tick += 1;
                    tick
                },
                |cancel, _, steer| {
                    let steer = steer.expect("a manager on ACP is addressable");
                    scope.spawn(move || {
                        let reader = Store::open(&watched).unwrap();
                        wait_for(&reader, 1);
                        // The session is still open, which is the claim.
                        steer
                            .steer("Now say goodbye in one short sentence.")
                            .expect("the steer reaches the live session");
                        wait_for(&reader, 2);
                        cancel.cancel();
                    });
                },
            )
        });

        // Cancelled by the test, so the report comes back through the cancelled
        // path - the run did not fail and did not end on its own.
        assert!(
            matches!(result, Err(ManagerError::Cancelled(_))),
            "expected a cancelled manager, got {result:?}"
        );
        let said = said(&store);
        // Printed so a live run leaves evidence of what the agent actually said.
        eprintln!("turns: {said:#?}");
        // Exactly two, not two-or-more: an answer is a **turn**, and an ACP
        // agent streams one a fragment at a time. Before the chunks were
        // assembled this assertion passed on a single "Hello" + "!" and the
        // steer was never being tested at all.
        assert_eq!(
            said.len(),
            2,
            "one answer per turn, assembled from fragments: {said:?}"
        );
        assert_ne!(
            said[0], said[1],
            "the second turn is a reply to the steer, not an echo"
        );
    }
}
