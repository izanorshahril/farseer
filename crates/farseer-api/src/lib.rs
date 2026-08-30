//! Farseer's local API: **bespoke HTTP plus SSE on `127.0.0.1`.**
//!
//! `16 local api surface` weighed making ACP the substrate, as berd does, and rejected it: roughly
//! a fifth of farseer's surface maps to an ACP verb, and **a protocol that
//! covers a fifth of the surface is not the transport**. ACP belongs on top of
//! this as a server adapter, exposing one cell's manager conversation and
//! nothing else.
//!
//! Two rules from `16 local api surface` shape everything below:
//!
//! - **Every stream connection takes a cursor.** Attach-to-a-running-worker and
//!   replay-a-dead-session are the same call with a different cursor, so they can
//!   never drift into two different answers.
//! - **A slow client never slows a worker.** The record is the durable answer;
//!   the stream is a tail on it.
//!
//! Versioning: `/v1/` in the path, **additive only** within a major. New fields
//! may appear; existing fields never change meaning and never vanish; clients
//! must ignore unknown fields.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use serde::{Deserialize, Serialize};

use farseer_core::RunnerConfig;
use farseer_core::policy::Budget;
use farseer_core::run::{WorkerContract, WorkerContractSpec, WorkspaceStrategy};
use farseer_core::{CellDefinition, CellId, LivenessThresholds, NewEvent, RunId, Seq, TaskId};
use farseer_manager::{
    LivenessHandle, MANAGER_CELL_FIELD, RUN_ROLE_FIELD, RunOptions, RunRole, RunSink, SteerHandle,
};
use farseer_runner::spawn::CancelToken;
use farseer_store::{RunRow, ScanFilter, Store, StoreError, UI_STATE_CAP_BYTES};

mod mcp;
mod notify;
pub mod security;

pub use security::{RuntimeToken, runtime_file_path, write_runtime_file};

/// How often the stream looks for new events.
///
/// Polling the record rather than fanning out from an in-process bus is what
/// makes "a slow client never slows a worker" structural instead of a rule
/// someone has to remember. `09 store decision` measured the read side at p99 478us while a
/// writer appends continuously, so this costs a worker nothing.
const STREAM_POLL: Duration = Duration::from_millis(250);

/// Events per stream tick. A bounded read, so one busy run cannot starve the
/// connection of memory.
const STREAM_BATCH: usize = 256;

pub struct AppState {
    store: Mutex<Store>,
    cells: Mutex<BTreeMap<CellId, CellDefinition>>,
    cells_dir: PathBuf,
    token: RuntimeToken,
    thresholds: LivenessThresholds,
    /// Where a run's workspace is created - a plain directory under here for
    /// `WorkspaceStrategy::PlainDirectory`, a `git worktree` under here for
    /// `Worktree`.
    runs_dir: PathBuf,
    /// The git repository a `Worktree`-strategy cell's runs are worktrees
    /// *of*. `13 harness build kit` deliberately keeps no git flag on `CellDefinition`, so this
    /// has to come from somewhere else - the runtime's own working directory
    /// is the only unambiguous repo available without inventing a field. In
    /// practice this is the farseer checkout itself, which is exactly what
    /// cell zero - farseer's own builder harness - is for.
    repo_root: PathBuf,
    /// Machine-wide runner facts, per `27 quota accounting` section 3: which
    /// account a runner signs in with, declared by the operator and never
    /// inferred. Loaded once at startup, because it describes the machine rather
    /// than the work - unlike cell definitions, which `16 local api surface`
    /// gives a reload verb.
    runner_config: RunnerConfig,
    /// In-flight runs, keyed by run id. A run removes its own entry when it
    /// finishes, successfully or not - so a lookup miss here means either
    /// "already finished" or "farseer restarted since", and the run row is
    /// what still answers which.
    runs: Mutex<HashMap<RunId, RunHandle>>,
    /// Active manager identities accepted by `delegate_to_worker`.
    managers: Mutex<HashMap<RunId, ManagerContext>>,
    /// Cancellation requested after a run id is returned but before its
    /// process exposes a live [`CancelToken`].
    pending_cancellations: Mutex<HashMap<RunId, Arc<AtomicBool>>>,
    /// In-flight delegated workers per cell, enforcing the definition's cap.
    worker_counts: Mutex<HashMap<CellId, u32>>,
    /// Set exactly once after `serve` binds, including an OS-selected port.
    base_url: OnceLock<String>,
    /// The last snapshot [`poll_windows`] took, or empty if nothing has polled.
    ///
    /// Cached rather than fetched per request for two reasons, and the second is
    /// the load-bearing one: a live `omp usage` takes seconds, and a handler
    /// that shells out per request makes an operator's quota view cost a process
    /// launch. Empty is the honest resting state - farseer reports what it
    /// observed, and before the first poll it has observed nothing.
    polled_windows: Mutex<Vec<farseer_core::WindowObservation>>,
}

/// What `farseer_manager::run_worker`'s `on_started` callback hands back for
/// one in-flight run: a way to end it, and a way to ask how it's doing.
struct RunHandle {
    cancel: CancelToken,
    liveness: LivenessHandle,
    /// `None` when this run's runner has no steering path - Codex today,
    /// per `farseer_manager::start_worker`.
    steer: Option<SteerHandle>,
}

#[derive(Clone)]
struct ManagerContext {
    contract: WorkerContract,
    cell: CellDefinition,
    /// A per-manager capability used both as the MCP bearer and as the identity bound to every manager-scoped tool call.
    manager_token: RuntimeToken,
    /// Serialized across tool calls so two concurrent delegations cannot both
    /// observe and spend the same remaining pool.
    remaining_budget: Arc<Mutex<Budget>>,
    child_runs: Arc<Mutex<HashSet<RunId>>>,
    cancel_requested: Arc<AtomicBool>,
}

struct WorkerPermit {
    state: Arc<AppState>,
    cell_id: CellId,
}

struct SecretFileGuard(Option<PathBuf>);

impl Drop for SecretFileGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        let mut counts = self
            .state
            .worker_counts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let remove = counts.get_mut(&self.cell_id).is_some_and(|count| {
            *count = count.saturating_sub(1);
            *count == 0
        });
        if remove {
            counts.remove(&self.cell_id);
        }
    }
}

impl AppState {
    /// Take the store lock, surviving a poisoning.
    ///
    /// A panic in one handler must not turn every later request into a panic:
    /// the store is a `Connection`, not an invariant a half-finished handler can
    /// leave inconsistent, because every write is its own statement or
    /// transaction.
    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cells(&self) -> std::sync::MutexGuard<'_, BTreeMap<CellId, CellDefinition>> {
        self.cells.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn runs(&self) -> std::sync::MutexGuard<'_, HashMap<RunId, RunHandle>> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn managers(&self) -> std::sync::MutexGuard<'_, HashMap<RunId, ManagerContext>> {
        self.managers.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn manager(&self, run_id: RunId) -> Option<ManagerContext> {
        self.managers().get(&run_id).cloned()
    }

    /// The manager this bearer belongs to, if it belongs to one.
    ///
    /// The guard already scans for exactly this to let a manager-scoped request
    /// through; `31 manager delegation reach` found the identity was resolved
    /// here and thrown away, leaving the model to re-state it from its prompt.
    fn manager_by_token(&self, presented: &str) -> Option<ManagerContext> {
        self.managers()
            .values()
            .find(|manager| manager.manager_token.matches(presented))
            .cloned()
    }

    fn manager_token_matches(&self, presented: &str) -> bool {
        self.managers()
            .values()
            .any(|manager| manager.manager_token.matches(presented))
    }

    fn acquire_worker(self: &Arc<Self>, cell_id: &CellId, worker_cap: u32) -> Option<WorkerPermit> {
        let mut counts = self.worker_counts.lock().unwrap_or_else(|e| e.into_inner());
        let count = counts.entry(cell_id.clone()).or_default();
        if *count >= worker_cap {
            return None;
        }
        *count += 1;
        Some(WorkerPermit {
            state: Arc::clone(self),
            cell_id: cell_id.clone(),
        })
    }

    fn set_mcp_endpoint(&self, port: u16) {
        let _ = self.base_url.set(format!("http://127.0.0.1:{port}"));
    }

    /// The runtime's own loopback origin, once the listener has bound.
    ///
    /// Stored as the base rather than as one endpoint because two manager faces
    /// hang off it now - `/v1/mcp` and `/v1/manager` - and a second `OnceLock`
    /// would be a second copy of one fact.
    fn base_url(&self) -> Option<String> {
        self.base_url.get().cloned()
    }

    fn mcp_endpoint(&self) -> Option<String> {
        self.base_url.get().map(|base| format!("{base}/v1/mcp"))
    }

    pub fn new(
        store: Store,
        cells_dir: impl Into<PathBuf>,
        token: RuntimeToken,
        runs_dir: impl Into<PathBuf>,
        repo_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            store: Mutex::new(store),
            cells: Mutex::new(BTreeMap::new()),
            cells_dir: cells_dir.into(),
            token,
            thresholds: LivenessThresholds::default(),
            polled_windows: Mutex::new(Vec::new()),
            runs_dir: runs_dir.into(),
            repo_root: repo_root.into(),
            runner_config: RunnerConfig::default(),
            runs: Mutex::new(HashMap::new()),
            managers: Mutex::new(HashMap::new()),
            pending_cancellations: Mutex::new(HashMap::new()),
            worker_counts: Mutex::new(HashMap::new()),
            base_url: OnceLock::new(),
        }
    }

    /// Re-read every definition from disk.
    ///
    /// `16 local api surface` gives the API read, validate and reload, and **no edit path**: an
    /// edit API would make farseer responsible for merge conflicts and skew
    /// against the operator's own editor, in exchange for nothing.
    /// Reloading makes the files on disk the truth: a cell whose own file is
    /// broken **disappears** until it parses again. `17 cell lifecycle` pins the definition
    /// version per run, so work already executing is unaffected.
    /// An absent or unreadable config is an empty one: declaring accounts
    /// improves accounting, and `27 quota accounting` never made it a
    /// precondition for running anything.
    pub fn with_runner_config(mut self, config: RunnerConfig) -> Self {
        self.runner_config = config;
        self
    }

    /// Close every run row a previous process left marked running, recording
    /// why. Returns how many.
    ///
    /// Called once at startup, before anything reads the fleet. See
    /// [`farseer_store::Store::reap_orphaned_runs`] for why these cannot simply
    /// be cancelled: `cancel` asks the in-memory handle, and an orphan has none,
    /// so every attempt answers `404` while the row stays `running` forever.
    pub fn reap_orphaned_runs(&self) -> Result<usize, StoreError> {
        let now = now_ms();
        let store = self.store();
        let orphans = store.reap_orphaned_runs(now)?;
        for row in &orphans {
            // `02 record scope`: the record says **why**, so a reaped run is
            // distinguishable from one that failed on its own. `05 run state
            // model` gets no new outcome for this - it is `Failed` because
            // something broke and nobody chose it.
            store.append(&farseer_core::NewEvent::new(
                row.cell_id.clone(),
                row.run_id,
                farseer_core::EventKind::new(farseer_core::EventKind::RUN_FINISHED),
                farseer_core::Actor::Operator,
                now,
                serde_json::json!({
                    "outcome": "failed",
                    "reason": "the farseer process that started this run is gone",
                }),
            ))?;
        }
        Ok(orphans.len())
    }

    pub(crate) fn repo_root(&self) -> &std::path::Path {
        &self.repo_root
    }

    pub fn runner_config(&self) -> &RunnerConfig {
        &self.runner_config
    }

    pub fn reload(&self) -> ReloadReport {
        let mut report = ReloadReport::default();
        let entries = match std::fs::read_dir(&self.cells_dir) {
            Ok(entries) => entries,
            Err(e) => {
                report.errors.push(ReloadError {
                    file: self.cells_dir.display().to_string(),
                    message: e.to_string(),
                });
                return report;
            }
        };

        let mut loaded = BTreeMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let file = path.display().to_string();
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| CellDefinition::load(&text).map_err(|e| e.to_string()))
            {
                Ok((definition, advisories)) => {
                    report.loaded.push(definition.cell_id.to_string());
                    report
                        .advisories
                        .extend(advisories.iter().map(|a| ReloadError {
                            file: file.clone(),
                            message: a.to_string(),
                        }));
                    loaded.insert(definition.cell_id.clone(), definition);
                }
                Err(message) => report.errors.push(ReloadError { file, message }),
            }
        }

        *self.cells() = loaded;
        report
    }
}

/// `farseer-manager` never holds a `Store` across a whole run - see that
/// crate's own doc comment for why - so `AppState` locks and releases the
/// store mutex for each individual write instead of handing over one
/// long-lived borrow.
impl RunSink for AppState {
    fn append(&self, event: &NewEvent) -> Result<Seq, StoreError> {
        self.store().append(event)
    }

    fn upsert_run(&self, row: &RunRow) -> Result<(), StoreError> {
        self.store().upsert_run(row)
    }

    fn observe_window(
        &self,
        cell_id: &CellId,
        run_id: RunId,
        observation: &farseer_core::WindowObservation,
        ts: i64,
    ) -> Result<bool, StoreError> {
        self.store()
            .observe_window(cell_id, run_id, observation, ts)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ReloadReport {
    pub loaded: Vec<String>,
    pub advisories: Vec<ReloadError>,
    pub errors: Vec<ReloadError>,
}

#[derive(Debug, Serialize)]
pub struct ReloadError {
    pub file: String,
    pub message: String,
}

/// Everything under `/v1`, behind the loopback and token guard.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/cells", get(list_cells))
        .route("/v1/cells/{cell_id}", get(get_cell))
        .route("/v1/cells/reload", post(reload_cells))
        .route("/v1/cells/{cell_id}/instruct", post(instruct_cell))
        .route("/v1/manager/delegate/worker", post(delegate_to_worker))
        .route("/v1/manager/delegate/cell", post(delegate_to_cell))
        .route("/v1/events", get(read_events))
        .route("/v1/stream", get(stream_events))
        .route("/v1/runs", get(list_runs))
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/cancel", post(cancel_run))
        .route("/v1/runs/{run_id}/steer", post(steer_run))
        .route("/v1/runs/{run_id}/rerun", post(rerun_run))
        .route("/v1/runs/{run_id}/rescope", post(rescope_run))
        .route("/v1/ui-state/{key}", get(get_ui_state).put(put_ui_state))
        .route("/v1/skills", get(skills))
        .route("/v1/quota", get(quota))
        .route("/v1/quota/refresh", post(refresh_quota))
        .route("/v1/analytics/cost", get(analytics_cost))
        .route("/v1/analytics/intervention", get(analytics_intervention))
        .route("/v1/analytics/rework", get(analytics_rework))
        .route("/v1/analytics/lessons", get(analytics_lessons))
        .nest_service("/v1/mcp", mcp::service(state.clone()))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Bind loopback only, write the runtime file, and serve until cancelled.
pub async fn serve(state: Arc<AppState>, port: u16) -> std::io::Result<()> {
    // `16 local api surface`: **bind `127.0.0.1` only.** Not `0.0.0.0`, not a hostname that might
    // resolve to something routable.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let bound = listener.local_addr()?.port();
    state.set_mcp_endpoint(bound);
    write_runtime_file(&runtime_file_path(), bound, &state.token)?;
    // `35 notification plane`: off unless the operator set a URL, and never in
    // the path of anything - a failed notification must not fail a run.
    notify::spawn(Arc::clone(&state));
    spawn_window_poll(Arc::clone(&state));
    axum::serve(listener, router(state)).await
}

async fn guard(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
    next: middleware::Next,
) -> Response {
    let headers = request.headers();
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .or_else(|| request.uri().host());
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());

    if !security::is_origin_allowed(host, origin) {
        return ApiError::Forbidden("request did not come from loopback").into_response();
    }
    let is_manager_request = is_manager_scoped(request.uri().path());
    if !presented_token(headers).is_some_and(|token| {
        state.token.matches(token) || (is_manager_request && state.manager_token_matches(token))
    }) {
        return ApiError::Unauthorized.into_response();
    }
    next.run(request).await
}

/// The routes a **manager's** bearer opens, as opposed to the operator's.
///
/// Two entries because a delegation can be asked for in two ways and they are
/// the same grant: `/v1/mcp` is the MCP face a Claude Code manager reaches
/// through its generated config, and `/v1/manager` is the plain-JSON face for
/// the runners that have no MCP client at all. Everything else on the router
/// stays the operator's, which is what
/// `a_manager_bearer_cannot_call_operator_routes` pins.
fn is_manager_scoped(path: &str) -> bool {
    path.starts_with("/v1/mcp") || path.starts_with("/v1/manager/")
}

fn presented_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

#[derive(Debug, thiserror::Error)]
enum ApiError {
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("a valid bearer token is required")]
    Unauthorized,
    #[error("no {0} by that id")]
    NotFound(&'static str),
    #[error("{0}")]
    BadRequest(&'static str),
    #[error("{0}")]
    Policy(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("could not prepare a workspace for this run: {0}")]
    Workspace(String),
    #[error("record holds an unreadable {0}")]
    Corrupt(&'static str),
    #[error("writing the steer message failed: {0}")]
    Steer(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) | Self::Policy(_) => StatusCode::BAD_REQUEST,
            // `24 ui state persistence`: over 1 MiB per key, the answer is `413`.
            Self::Store(StoreError::UiStateTooLarge { .. }) => StatusCode::PAYLOAD_TOO_LARGE,
            // `24 ui state persistence`: the key is capped too, and an overlong
            // one arrives in the URL, so `414` names what was actually too long.
            Self::Store(StoreError::UiStateKeyTooLong { .. }) => StatusCode::URI_TOO_LONG,
            Self::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Workspace(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Corrupt(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Steer(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(serde_json::json!({ "error": self.to_string() })),
        )
            .into_response()
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "api_version": "v1",
        "runtime_version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn list_cells(State(state): State<Arc<AppState>>) -> Json<Vec<CellSummary>> {
    let cells = state.cells();
    Json(
        cells
            .values()
            .map(|c| CellSummary {
                cell_id: c.cell_id.to_string(),
                name: c.name.clone(),
                description: c.description.clone(),
                version: c.version.clone(),
                roster_size: c.roster.len(),
            })
            .collect(),
    )
}

#[derive(Debug, Serialize)]
pub struct CellSummary {
    pub cell_id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub roster_size: usize,
}

async fn get_cell(
    State(state): State<Arc<AppState>>,
    UrlPath(cell_id): UrlPath<String>,
) -> ApiResult<Json<CellDefinition>> {
    let cells = state.cells();
    cells
        .get(&CellId::new(cell_id))
        .cloned()
        .map(Json)
        .ok_or(ApiError::NotFound("cell"))
}

async fn reload_cells(State(state): State<Arc<AppState>>) -> Json<ReloadReport> {
    Json(state.reload())
}

#[derive(Debug, Deserialize)]
pub struct InstructBody {
    pub goal: String,
}

#[derive(Debug, Serialize)]
pub struct InstructResponse {
    pub run_id: String,
}

/// Delegation for a runner that cannot speak MCP, on exactly the terms one that can gets.
///
/// `31 manager delegation reach` found the roster reachable from Claude Code and
/// from nowhere else, and the fix could not be "give every runner an MCP client"
/// because most of them do not have one and are not going to. The verbs are the
/// same functions the MCP tools call, [`mcp::FarseerMcp::delegate_to_worker_json`],
/// so the roster resolution, the worker cap, the budget draw-down and the
/// cancellation link are one implementation rather than two that drift.
///
/// Authorization is the manager's own token, presented as the bearer. The body
/// may repeat it and does not have to: `31 manager delegation reach` moved
/// identity onto the credential the transport already proved, after a model
/// mistyped the one it was asked to copy. A manager bearer opens this route and
/// no operator route.
async fn delegate_to_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<mcp::DelegateToWorkerArgs>,
) -> ApiResult<Json<serde_json::Value>> {
    let bearer = presented_token(&headers).map(str::to_string);
    tokio::task::block_in_place(|| {
        mcp::FarseerMcp::new(state).delegate_to_worker_json(body, bearer.as_deref())
    })
    .map(Json)
    .map_err(mcp_error)
}

/// The fire-and-forget half. See [`delegate_to_worker`].
async fn delegate_to_cell(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<mcp::DelegateToCellArgs>,
) -> ApiResult<Json<serde_json::Value>> {
    let bearer = presented_token(&headers).map(str::to_string);
    mcp::FarseerMcp::new(state)
        .delegate_to_cell_json(body, bearer.as_deref())
        .map(Json)
        .map_err(mcp_error)
}

/// An MCP refusal is a policy refusal on this face: the caller asked for
/// something the roster, the cap or the budget does not allow, which is a `400`
/// and not a runtime fault.
fn mcp_error(error: rmcp::ErrorData) -> ApiError {
    ApiError::Policy(error.message.to_string())
}

/// [What is the local API surface?] makes an instruction fire-and-forget: this returns `202` with a `run_id`, and the record carries the run.
///
/// A Claude Code manager receives a generated strict MCP config after the listener's real port is known.
/// The live manager may call `delegate_to_worker`, which preserves this task while a named pinned-roster worker executes a child contract synchronously.
/// It may also call `delegate_to_cell`, which preserves the task while a granted cell runs its own manager - fire-and-forget, per `06 cell transport`.
/// Managers using another native runner still execute the goal directly because no equivalent MCP launch shape has been verified for those CLIs.
///
/// [What is the local API surface?]: ../../../.scratch/farseer/issues/16-local-api-surface.md
async fn instruct_cell(
    State(state): State<Arc<AppState>>,
    UrlPath(cell_id): UrlPath<String>,
    Json(body): Json<InstructBody>,
) -> ApiResult<(StatusCode, Json<InstructResponse>)> {
    if body.goal.trim().is_empty() {
        return Err(ApiError::BadRequest("goal must not be empty"));
    }
    let cell = state
        .cells()
        .get(&CellId::new(cell_id))
        .cloned()
        .ok_or(ApiError::NotFound("cell"))?;
    check_tool_level(&cell.manager.runner, cell.manager.tools)?;
    let contract = WorkerContract::seal(WorkerContractSpec {
        run_id: RunId::new(),
        task_id: TaskId::new(),
        cell_id: cell.cell_id.clone(),
        goal: body.goal,
        workspace: cell.workspace_strategy,
        runner: cell.manager.runner.clone(),
        tool_grants: cell.tool_grants(),
        tool_level: cell.manager.tools,
        autonomy_ceiling: cell.policy.autonomy_ceiling,
        // `23 prototype loose ends`: the task starts with the owning cell's
        // pool; every delegated worker narrows and draws down from this root.
        budget: cell.budget,
        definition_of_done: String::new(),
    });
    let run_id = spawn_run(&state, contract, RunRole::Manager, cell)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(InstructResponse {
            run_id: run_id.to_string(),
        }),
    ))
}

/// `12 autonomy and deny list`: every currently implemented native LLM runner has shell-equivalent reach, so launching one without an explicit shell-capable roster grant would silently widen authority.
pub(crate) fn ensure_runner_authority(cell: &CellDefinition, runner: &str) -> ApiResult<()> {
    if matches!(runner, "claude-code" | "codex" | "cursor-agent" | "goose")
        && !cell.has_shell_grant()
    {
        return Err(ApiError::Policy(format!(
            "runner `{runner}` exposes shell-equivalent reach, but cell `{}` grants no shell-capable tool",
            cell.cell_id
        )));
    }
    Ok(())
}

/// A bounded dimension must stop spend before it happens, not merely report an overrun afterward.
pub(crate) fn unenforceable_budget_dimension(
    _runner: &str,
    budget: Budget,
) -> Option<&'static str> {
    if budget.tokens.is_some() {
        return Some("tokens");
    }
    if budget.wall_secs.is_some() {
        return Some("wall-clock");
    }
    if budget.usd_micros.is_some() {
        // `10 runner inventory`: Claude Code 2.1.233 accepted a one-micro-dollar
        // cap, reported $0.131195, then failed with `error_max_budget_usd`.
        // Every current native runner therefore lacks pre-spend enforcement.
        return Some("currency");
    }
    None
}

/// `04 spike workspace teardown`: a `Worktree` cell gets a real git worktree off `repo_root`, while a `PlainDirectory` cell gets exactly that.
/// Shared by root and delegated runs so a workspace failure surfaces before anything is spawned.
pub(crate) fn create_workspace(
    state: &AppState,
    strategy: WorkspaceStrategy,
    run_id: RunId,
) -> ApiResult<(PathBuf, Option<PathBuf>)> {
    let name = run_id.to_string();
    match strategy {
        WorkspaceStrategy::Worktree => {
            let cwd = farseer_runner::workspace::create_worktree(
                &state.repo_root,
                &state.runs_dir,
                &name,
            )
            .map_err(|e| ApiError::Workspace(e.to_string()))?;
            Ok((cwd, Some(state.repo_root.clone())))
        }
        WorkspaceStrategy::PlainDirectory => {
            let cwd = state.runs_dir.join(&name);
            std::fs::create_dir_all(&cwd).map_err(|e| ApiError::Workspace(e.to_string()))?;
            Ok((cwd, None))
        }
    }
}

pub(crate) fn execute_run(
    state: &Arc<AppState>,
    contract: &WorkerContract,
    cwd: &Path,
    options: &RunOptions,
    cancel_requested: Option<&AtomicBool>,
) -> Result<farseer_manager::RunReport, farseer_manager::ManagerError> {
    let run_id = contract.run_id;
    let result = farseer_manager::run_worker(
        state.as_ref(),
        contract,
        cwd,
        state.thresholds,
        options,
        now_ms,
        |cancel, liveness, steer| {
            state.runs().insert(
                run_id,
                RunHandle {
                    cancel: cancel.clone(),
                    liveness,
                    steer,
                },
            );
            if cancel_requested.is_some_and(|requested| requested.load(Ordering::Acquire)) {
                cancel.cancel();
            }
        },
    );
    state.runs().remove(&run_id);
    result
}

fn manager_run_options(
    state: &AppState,
    contract: &WorkerContract,
    cell: &CellDefinition,
    manager_token: &RuntimeToken,
) -> ApiResult<RunOptions> {
    let mut options = RunOptions {
        actor: farseer_core::Actor::Operator,
        role: RunRole::Manager,
        manager_cell: Some(cell.clone()),
        claude_mcp_config: None,
        append_system_prompt: None,
        runner_env: Vec::new(),
        mcp: None,
        extensions: Vec::new(),
        // Declared by the operator in runner config, per `27 quota accounting`,
        // and passed down rather than derived: the manager crate reads no
        // config, and an undeclared runner is its own account.
        account: Some(state.runner_config().account_for(&contract.runner)),
        usd_micros_per_mtok: state.runner_config().price_for(&contract.runner),
        // The manager's own declared skills, resolved to directories.
        skills: skill_paths(state, &contract.runner, &cell.manager.skills)?,
        // What the operator pinned, or nothing at all. `30 codex app
        // server`: farseer passes a model or an effort only when a
        // person wrote one down, so an unpinned runner keeps whatever
        // its own config says.
        model: state
            .runner_config()
            .launch_of(&contract.runner)
            .0
            .map(str::to_string),
        effort: state
            .runner_config()
            .launch_of(&contract.runner)
            .1
            .map(str::to_string),
    };
    // `22 cell addressing` section 3: the roster is the grant, so the prompt
    // names exactly what is callable. A manager that has to guess will guess.
    let workers = cell
        .roster
        .iter()
        .filter_map(|entry| match entry {
            farseer_core::RosterEntry::Worker { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ");
    let callable_cells = cell
        .roster
        .iter()
        .filter_map(|entry| match entry {
            farseer_core::RosterEntry::Cell { name, peer, .. } if !peer => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ");
    let workers = if workers.is_empty() { "none" } else { &workers };
    let callable_cells = if callable_cells.is_empty() {
        "none"
    } else {
        &callable_cells
    };

    // Whether this run can actually reach the roster, as opposed to merely
    // having one - and if so, over what.
    //
    // `31 manager delegation reach` opened on the first case and closes on the
    // third: the roster was reachable from Claude Code and from nowhere else,
    // because delegation had only ever been offered as an MCP endpoint and most
    // runners have no MCP client. There is no one protocol to add. What
    // generalises is the **verb**, so each runner reaches the same two
    // functions over whatever it does speak.
    let reach = if state.base_url().is_none() {
        // Before the listener binds there is no endpoint to hand anyone.
        Reach::None
    } else if contract.runner == "claude-code" {
        Reach::Mcp
    } else if farseer_manager::PI_RUNNERS
        .iter()
        .any(|(name, _)| *name == contract.runner)
        && delegate_extension(state).is_some()
    {
        Reach::PiExtension
    } else if contract.runner == "codex-app-server" {
        Reach::CodexAppServer
    } else if farseer_manager::ACP_RUNNERS
        .iter()
        .any(|(name, _, _)| *name == contract.runner)
    {
        Reach::Acp
    } else {
        Reach::None
    };

    let mut prompt = match reach {
        // Codex shares Claude Code's wording because it shares the transport,
        // and that is the point rather than an accident: **an MCP client calls
        // the tool itself**, so it has to pass the two credentials the tool
        // authorizes on. The extension route does not, which is why pi's prompt
        // carries none - see the note on `Reach::PiExtension`.
        //
        // Found by running it. The first version of this reused pi's wording and
        // the manager answered `Unable to delegate: manager authorization token
        // unavailable.` - it could see the tools and had nothing to present.
        // No credentials, on any transport. `31 manager delegation reach` moved
        // identity onto the bearer the request already carries, after a
        // `goose-acp` manager reached the tool and **mistyped** the
        // 64-character token it had been asked to copy - failing as an
        // authorization error when it was a transcription error. A model never
        // given a secret cannot quote it, leak it, or get it wrong.
        Reach::Mcp | Reach::CodexAppServer | Reach::Acp => format!(
            "You are the manager for farseer cell `{}`. The roster workers available to              delegate to are: {}. The cells you may call are: {}.              Use the `delegate_to_worker` tool: name a roster worker, give it a precise              sub-goal, and relay the returned result. The worker sees nothing of              this conversation, so the sub-goal must stand alone.              Calling a cell is different: delegate_to_cell is fire-and-forget, it returns a              call_id rather than an answer, and the callee owns its own workspace, runner              and tools. Anything not named above is not callable, and asking again will not              make it callable. The task remains yours to own.",
            cell.cell_id, workers, callable_cells,
        ),
        // No credentials in this one. The extension holds them and injects them
        // itself, so the model states a worker and a goal and never sees the
        // token that authorizes the call - which is strictly better than the
        // MCP shape above, where the manager has to carry its own bearer.
        Reach::PiExtension => format!(
            "You are the manager for farseer cell `{}`. The roster workers available to \
             delegate to are: {}. The cells you may call are: {}. \
             Use the `delegate_to_worker` tool: name a roster worker, give it a precise \
             sub-goal, and relay the returned result. The worker sees nothing of this \
             conversation, so the sub-goal must stand alone. \
             `delegate_to_cell` is different: it is fire-and-forget, it returns a call_id \
             rather than an answer, and the callee owns its own workspace, runner and tools. \
             Anything not named above is not callable, and asking again will not make it \
             callable. The task remains yours to own.",
            cell.cell_id, workers, callable_cells,
        ),
        // The honest limit, stated in the one place the manager will read it.
        //
        // `31 manager delegation reach` records what the silence cost: a
        // manager asked to spawn a sub-agent ran a command itself and reported
        // the wait as a delegation, because an agent asked to delegate with no
        // delegation tool will do the nearest available thing and describe it
        // as compliance. The last sentence is the one that matters - it is
        // cheaper to be told than to have the record hold a false claim about
        // farseer's own execution.
        Reach::None => format!(
            "You are the manager for farseer cell `{}`, running on the `{}` runner. \
             This cell's roster names these workers: {}. It names these callable cells: {}. \
             You CANNOT reach any of them: farseer has no delegation channel for this runner, \
             so there is no tool you can call to hand work to a worker or to another cell, and \
             none will appear during this run. \
             Do the work yourself, within this workspace. \
             If a task genuinely requires a worker you cannot reach, say so plainly and stop. \
             Never state or imply that you delegated, spawned, dispatched or handed off \
             anything - you did not, and farseer records what actually ran.",
            cell.cell_id, contract.runner, workers, callable_cells,
        ),
    };
    if !cell.manager.prompt.trim().is_empty() {
        prompt.push_str("\n\nCell manager instructions:\n");
        prompt.push_str(&cell.manager.prompt);
    }
    options.append_system_prompt = Some(prompt);

    match reach {
        Reach::Mcp => {
            let config_path = security::manager_config_path(&contract.run_id.to_string());
            let config = serde_json::json!({
                "mcpServers": {
                    "farseer": {
                        "type": "http",
                        "url": state.mcp_endpoint().expect("reach implies a bound listener"),
                        "headers": {
                            "Authorization": format!("Bearer {}", manager_token.as_str()),
                        },
                    },
                },
            });
            let bytes = serde_json::to_vec_pretty(&config)
                .map_err(|e| ApiError::Workspace(format!("serializing manager MCP config: {e}")))?;
            security::write_user_only_file(&config_path, &bytes)
                .map_err(|e| ApiError::Workspace(format!("writing manager MCP config: {e}")))?;
            options.claude_mcp_config = Some(config_path);
        }
        // The credential goes in the environment, which is why
        // `RunOptions::runner_env` exists: not on the argv, where every process
        // listing on this machine can read it, and not in the prompt, where the
        // model itself would.
        Reach::PiExtension => {
            options.extensions = vec![delegate_extension(state).expect("reach implies the file")];
            options.runner_env = vec![
                (
                    "FARSEER_ENDPOINT".to_string(),
                    state.base_url().expect("reach implies a bound listener"),
                ),
                (
                    "FARSEER_MANAGER_RUN_ID".to_string(),
                    contract.run_id.to_string(),
                ),
                (
                    "FARSEER_MANAGER_TOKEN".to_string(),
                    manager_token.as_str().to_string(),
                ),
            ];
        }
        // The same two verbs as `Reach::Mcp`, over the same MCP face, with the
        // credential moved off the model's page and into the environment - the
        // improvement `31 manager delegation reach` made for pi, arriving here
        // for free because Codex resolves the variable in its own process.
        //
        // No config file: `Reach::Mcp` writes one because Claude Code reads MCP
        // servers from disk, and a file is a secret with a lifetime to manage.
        // This route has neither.
        // ACP takes a literal header rather than the name of a variable, so
        // the bearer is in the frame - a step down from the other two, recorded
        // in `31 manager delegation reach` rather than smoothed over. The frame
        // goes down a pipe to a child farseer spawned, and `02 record scope`
        // scrubs what reaches the log.
        Reach::Acp => {
            options.mcp = Some(farseer_manager::McpReach::BearerInline {
                url: state
                    .mcp_endpoint()
                    .expect("reach implies a bound listener"),
                bearer: manager_token.as_str().to_string(),
            });
        }
        Reach::CodexAppServer => {
            options.mcp = Some(farseer_manager::McpReach::BearerFromEnv {
                url: state
                    .mcp_endpoint()
                    .expect("reach implies a bound listener"),
                var: "FARSEER_MANAGER_TOKEN".to_string(),
            });
            options.runner_env = vec![(
                "FARSEER_MANAGER_TOKEN".to_string(),
                manager_token.as_str().to_string(),
            )];
        }
        Reach::None => {}
    }
    Ok(options)
}

/// How a manager on this runner asks for a delegation, if it can at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// farseer's MCP face, through a generated config. Claude Code.
    Mcp,
    /// farseer's plain-JSON manager face, through an extension registering the
    /// two verbs as ordinary tools. pi and omp.
    PiExtension,
    /// farseer's MCP face over ACP's own `session/new` `mcpServers`.
    ///
    /// `29 harness protocol` found ACP's `fs/*` and `terminal/*` are served **by
    /// the client** and incompatible with a runner-owned worktree, so farseer
    /// declines both - which made it easy to conclude ACP had nothing to offer a
    /// manager. `mcpServers` is a different mechanism: farseer serves no tools,
    /// it names an address the agent dials itself. goose 1.47.0 advertises
    /// `mcpCapabilities.http` and reports what it reached.
    Acp,
    /// farseer's MCP face again, but configured **in the handshake** rather than
    /// in a file on disk: `codex-app-server` takes `config.mcp_servers` on
    /// `thread/start`, and takes the bearer as the *name of an environment
    /// variable* it resolves in its own process. `31 manager delegation reach`
    /// predicted this route; `30 codex app server` named the field.
    CodexAppServer,
    /// Nothing. The manager is told so rather than left to improvise.
    None,
}

/// The delegation extension farseer ships for pi and omp, if it is on disk.
///
/// Checked rather than assumed, per `10 runner inventory`'s observed-never-
/// advertised rule: a manager promised a `delegate_to_worker` tool that failed
/// to load would be back to the fabrication this ticket opened on, so a missing
/// file downgrades the run to an honest [`Reach::None`] instead.
fn delegate_extension(state: &AppState) -> Option<PathBuf> {
    let path = state
        .repo_root()
        .join("extensions")
        .join("pi")
        .join("farseer-delegate.ts");
    path.is_file().then_some(path)
}

/// Create a workspace synchronously, run on a blocking task, then tear down.
pub(crate) fn spawn_run(
    state: &Arc<AppState>,
    contract: WorkerContract,
    role: RunRole,
    pinned_cell: CellDefinition,
) -> ApiResult<RunId> {
    ensure_runner_authority(&pinned_cell, &contract.runner)?;
    if let Some(dimension) = unenforceable_budget_dimension(&contract.runner, contract.budget) {
        return Err(ApiError::Policy(format!(
            "{} runner `{}` cannot enforce the cell's {dimension} budget before spending",
            role.as_record_str(),
            contract.runner
        )));
    }
    let worker_permit = if role == RunRole::Worker {
        Some(
            state
                .acquire_worker(&pinned_cell.cell_id, pinned_cell.policy.worker_cap)
                .ok_or_else(|| {
                    ApiError::Policy(format!(
                        "cell `{}` is already at its worker cap of {}",
                        pinned_cell.cell_id, pinned_cell.policy.worker_cap
                    ))
                })?,
        )
    } else {
        None
    };
    let run_id = contract.run_id;
    let (cwd, repo_for_teardown) = create_workspace(state, contract.workspace, run_id)?;
    let cancel_requested = Arc::new(AtomicBool::new(false));
    let manager_context = (role == RunRole::Manager).then(|| ManagerContext {
        manager_token: RuntimeToken::generate(),
        remaining_budget: Arc::new(Mutex::new(contract.budget)),
        child_runs: Arc::new(Mutex::new(HashSet::new())),
        cancel_requested: Arc::clone(&cancel_requested),
        contract: contract.clone(),
        cell: pinned_cell.clone(),
    });
    let options = if let Some(context) = &manager_context {
        manager_run_options(state, &contract, &context.cell, &context.manager_token)
    } else {
        Ok(RunOptions {
            actor: farseer_core::Actor::Operator,
            role,
            manager_cell: Some(pinned_cell),
            claude_mcp_config: None,
            append_system_prompt: None,
            runner_env: Vec::new(),
            mcp: None,
            extensions: Vec::new(),
            account: Some(state.runner_config().account_for(&contract.runner)),
            usd_micros_per_mtok: state.runner_config().price_for(&contract.runner),
            // A run reached without a manager context - a rerun, or a
            // direct worker - carries no declared skills rather than
            // inheriting the cell manager's.
            skills: Vec::new(),
            // What the operator pinned, or nothing at all. `30 codex app
            // server`: farseer passes a model or an effort only when a
            // person wrote one down, so an unpinned runner keeps whatever
            // its own config says.
            model: state
                .runner_config()
                .launch_of(&contract.runner)
                .0
                .map(str::to_string),
            effort: state
                .runner_config()
                .launch_of(&contract.runner)
                .1
                .map(str::to_string),
        })
    };
    let options = match options {
        Ok(options) => options,
        Err(error) => {
            let _ = std::fs::remove_file(security::manager_config_path(&run_id.to_string()));
            let _ =
                farseer_runner::workspace::teardown_workspace(&cwd, repo_for_teardown.as_deref());
            return Err(error);
        }
    };
    let config_path = options.claude_mcp_config.clone();
    if let Some(context) = manager_context.as_ref() {
        // Register before returning the run id so an immediate cancel or the
        // manager's first MCP call cannot race process startup and see 404/401.
        state.managers().insert(run_id, context.clone());
    }
    state
        .pending_cancellations
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(run_id, Arc::clone(&cancel_requested));
    let background_state = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let _config_guard = SecretFileGuard(config_path);
        let _worker_permit = worker_permit;
        let result = execute_run(
            &background_state,
            &contract,
            &cwd,
            &options,
            Some(cancel_requested.as_ref()),
        );
        observe_window(&background_state, &contract, &result);
        background_state.managers().remove(&run_id);
        background_state
            .pending_cancellations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&run_id);
        drop(result);

        if let Err(e) =
            farseer_runner::workspace::teardown_workspace(&cwd, repo_for_teardown.as_deref())
        {
            eprintln!("workspace teardown for run {run_id} did not complete: {e}");
        }
    });

    Ok(run_id)
}

/// Record a window transition the run happened to observe, per `27 quota accounting`.
///
/// The run is the only thing that sees `rate_limit_event`, and the runtime is the
/// only thing that knows which **account** the runner signs in with, so the two
/// halves meet here. Appended on change only, so the repetition `10 runner
/// inventory` measured - one report per successful run, identical across
/// concurrent runs on one account - never reaches the log.
///
/// A cancelled run still carries what it observed: the window it saw was real.
fn observe_window(
    state: &Arc<AppState>,
    contract: &WorkerContract,
    result: &Result<farseer_manager::RunReport, farseer_manager::ManagerError>,
) {
    let report = match result {
        Ok(report) => Some(report),
        Err(farseer_manager::ManagerError::Cancelled(report)) => Some(report),
        Err(_) => None,
    };
    let account = state.runner_config().account_for(&contract.runner);

    // Claude Code's single window, in its own field names, and every window a
    // runner reported wholesale - `30 codex app server` found one reporting two.
    // Both paths converge here because this is the layer that knows the account:
    // `27 quota accounting` declares it in runner config and never infers it,
    // which is exactly why the adapters leave it blank.
    let mut observations: Vec<farseer_core::WindowObservation> = Vec::new();
    if let Some(info) = report.and_then(|report| report.window.as_ref()) {
        observations.push(farseer_core::WindowObservation {
            account: account.clone(),
            runner: contract.runner.clone(),
            availability: info.availability(),
            rate_limit_type: info.rate_limit_type.clone(),
            is_using_overage: info.is_using_overage,
            // Claude Code states no percentage headless - `10 runner inventory`
            // measured that - and farseer never computes one.
            used_percent: None,
            window_duration_mins: None,
            // A run's own window names a runner, not a provider. Same rule as
            // the adapters: transcribe what was said, leave the rest absent.
            provider: None,
            label: None,
        });
    }
    observations.extend(report.into_iter().flat_map(|report| {
        report.windows.iter().cloned().map(|mut observation| {
            observation.account = account.clone();
            observation.runner = contract.runner.clone();
            observation
        })
    }));
    if observations.is_empty() {
        return;
    }

    let store = state.store();
    for observation in &observations {
        if let Err(error) =
            store.observe_window(&contract.cell_id, contract.run_id, observation, now_ms())
        {
            // The record is the product, but a window transition is an
            // observation about the machine rather than about the run, so
            // losing one must not fail a run that already succeeded.
            eprintln!(
                "window observation for run {} was not recorded: {error}",
                contract.run_id
            );
        }
    }
}

/// `05 run state model` cancellation ends the selected run as `cancelled`, never `failed`.
/// Cancelling a manager also marks its ownership context and cancels every active delegated child, including a child racing with process startup.
async fn cancel_run(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<StatusCode> {
    let run_id: RunId = run_id.parse().map_err(|_| ApiError::NotFound("run"))?;
    let pending = state
        .pending_cancellations
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&run_id)
        .cloned();
    if let Some(requested) = &pending {
        requested.store(true, Ordering::Release);
    }
    let manager = state.manager(run_id);
    let child_runs = manager
        .as_ref()
        .map(|context| {
            context.cancel_requested.store(true, Ordering::Release);
            context
                .child_runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .copied()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut tokens = {
        let runs = state.runs();
        std::iter::once(run_id)
            .chain(child_runs)
            .filter_map(|id| runs.get(&id).map(|handle| handle.cancel.clone()))
            .collect::<Vec<_>>()
    };
    if pending.is_none() && manager.is_none() && tokens.is_empty() {
        return Err(ApiError::NotFound("run"));
    }
    for token in tokens.drain(..) {
        token.cancel();
    }
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Deserialize)]
pub struct SteerBody {
    pub message: String,
}

/// `05 run state model`'s **steer**: a follow-up message into a run's live process, on the
/// frame `claude_code::steer_frame`'s 2026-08-23 probe verified.
/// `400` when the run's runner has no steering path (Codex today) rather
/// than writing a line nothing reads; `404` when the run is unknown or
/// already finished, same as `cancel`.
async fn steer_run(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
    Json(body): Json<SteerBody>,
) -> ApiResult<StatusCode> {
    if body.message.trim().is_empty() {
        return Err(ApiError::BadRequest("message must not be empty"));
    }
    let run_id: RunId = run_id.parse().map_err(|_| ApiError::NotFound("run"))?;
    let handle = state
        .runs()
        .get(&run_id)
        .ok_or(ApiError::NotFound("run"))?
        .steer
        .clone();
    match handle {
        Some(steer) => {
            steer
                .steer(&body.message)
                .map_err(|e| ApiError::Steer(e.to_string()))?;
            Ok(StatusCode::ACCEPTED)
        }
        None => Err(ApiError::BadRequest(
            "this run's runner has no steering path",
        )),
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct RescopeBody {
    /// The field being changed. `05 run state model`: re-scope is a new run because a
    /// contract field changed; a run's own goal is the one an operator can
    /// actually reach here today. Omit to leave it as it was - that is
    /// `rerun`, not `rescope`, so this endpoint refuses an unchanged goal.
    pub goal: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RespawnResponse {
    pub run_id: String,
    pub parent_run_id: String,
}

/// Reconstructs a past run's sealed contract, role, and pinned manager definition from its `run_queued` event.
/// `05 run state model` makes contract immutability the reason re-run and re-scope have one durable answer.
struct OriginalRun {
    spec: WorkerContractSpec,
    role: RunRole,
    manager_cell: Option<CellDefinition>,
}

fn original_run(state: &AppState, run_id: RunId) -> ApiResult<OriginalRun> {
    let events = {
        let store = state.store();
        store.scan(0, 5_000, &ScanFilter::run(run_id))?
    };
    let queued = events
        .iter()
        .find(|e| e.kind.as_str() == farseer_core::EventKind::RUN_QUEUED)
        .ok_or(ApiError::NotFound("run"))?;
    let spec: WorkerContractSpec = serde_json::from_value(queued.payload.clone())
        .map_err(|_| ApiError::Corrupt("run_queued event"))?;
    let role = match queued
        .payload
        .get(RUN_ROLE_FIELD)
        .and_then(serde_json::Value::as_str)
    {
        Some(value) => RunRole::from_record_str(value).ok_or(ApiError::Corrupt("run role"))?,
        // Before run roles were recorded, every operator-queued run was the
        // manager entry point; manager-authored worker runs did not exist yet.
        None if queued.actor == farseer_core::Actor::Operator => RunRole::Manager,
        None => RunRole::Worker,
    };
    let manager_cell = queued
        .payload
        .get(MANAGER_CELL_FIELD)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| ApiError::Corrupt("manager cell snapshot"))?;
    Ok(OriginalRun {
        spec,
        role,
        manager_cell,
    })
}

/// `05 run state model`'s **re-run**: same contract, fresh run, fresh workspace. `16 local api surface`:
/// operator-initiated re-run leaves an event behind - here, the
/// `rescoped_from` edge `11 analytics questions`'s rework-depth query already walks, so a chain
/// of re-runs reads exactly like a chain of re-scopes to that analytics query.
async fn rerun_run(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<(StatusCode, Json<RespawnResponse>)> {
    let parent_run_id: RunId = run_id.parse().map_err(|_| ApiError::NotFound("run"))?;
    let mut original = original_run(&state, parent_run_id)?;
    original.spec.run_id = RunId::new();
    respawn(&state, original, parent_run_id).await
}

/// `05 run state model`'s **re-scope**: a new run against the same task, with a changed
/// contract field. Only `goal` is reachable here today - `tool_grants`,
/// `autonomy_ceiling` and the rest come from the cell definition at
/// `instruct` time, not from anything an operator can override per run yet.
async fn rescope_run(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
    Json(body): Json<RescopeBody>,
) -> ApiResult<(StatusCode, Json<RespawnResponse>)> {
    let parent_run_id: RunId = run_id.parse().map_err(|_| ApiError::NotFound("run"))?;
    let mut original = original_run(&state, parent_run_id)?;
    let Some(goal) = body.goal else {
        return Err(ApiError::BadRequest(
            "rescope needs a changed field - pass goal, or use rerun to repeat the same contract",
        ));
    };
    if goal.trim().is_empty() {
        return Err(ApiError::BadRequest("goal must not be empty"));
    }
    if goal == original.spec.goal {
        return Err(ApiError::BadRequest(
            "goal is unchanged - use rerun, not rescope, to repeat the same contract",
        ));
    }
    original.spec.run_id = RunId::new();
    original.spec.goal = goal;
    respawn(&state, original, parent_run_id).await
}

async fn respawn(
    state: &Arc<AppState>,
    original: OriginalRun,
    parent_run_id: RunId,
) -> ApiResult<(StatusCode, Json<RespawnResponse>)> {
    let pinned_cell = original.manager_cell.ok_or(ApiError::BadRequest(
        "this legacy run has no pinned cell definition and cannot be rerun safely",
    ))?;
    let run_id = original.spec.run_id;
    state.store().record_rescope(run_id, parent_run_id)?;
    let run_id = spawn_run(
        state,
        WorkerContract::seal(original.spec),
        original.role,
        pinned_cell,
    )?;
    Ok((
        StatusCode::ACCEPTED,
        Json(RespawnResponse {
            run_id: run_id.to_string(),
            parent_run_id: parent_run_id.to_string(),
        }),
    ))
}

#[derive(Debug, Default, Deserialize)]
pub struct StreamQuery {
    /// Scope, applied server-side. `16 local api surface` rejected a firehose the client filters.
    pub cell: Option<String>,
    pub run: Option<String>,
    /// The cursor. Omit for live only.
    pub since: Option<Seq>,
    pub limit: Option<usize>,
    /// Read the **last** this-many events instead of the first, for a surface
    /// opening cold with no cursor to resume from. Ignored when `since` is
    /// given, because a caller holding a cursor is resuming and a tail would
    /// skip the gap it is resuming across. See `Store::scan_tail`.
    pub tail: Option<usize>,
}

impl StreamQuery {
    /// An unparseable run id is an error, never a silently wider query: a
    /// filter that fails open would hand a client every other run in the log
    /// while it believed it was reading one.
    fn filter(&self) -> ApiResult<ScanFilter> {
        let run_id = match &self.run {
            Some(run) => Some(
                run.parse::<RunId>()
                    .map_err(|_| ApiError::BadRequest("run must be a uuid"))?,
            ),
            None => None,
        };
        Ok(ScanFilter {
            cell_id: self.cell.as_ref().map(|c| CellId::new(c.clone())),
            run_id,
        })
    }
}

async fn read_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StreamQuery>,
) -> ApiResult<Json<Vec<farseer_core::Event>>> {
    let store = state.store();
    let filter = query.filter()?;
    let events = match query.tail {
        Some(tail) if query.since.is_none() => store.scan_tail(tail.min(5_000), &filter)?,
        _ => store.scan(
            query.since.unwrap_or(0),
            query.limit.unwrap_or(500).min(5_000),
            &filter,
        )?,
    };
    Ok(Json(events))
}

/// The one stream endpoint.
///
/// SSE carries `Last-Event-ID` in the protocol itself, so reconnect-with-cursor
/// is free rather than something farseer invents.
async fn stream_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> ApiResult<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<SseEvent, std::convert::Infallible>>>,
> {
    let filter = query.filter()?;
    let resume = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<Seq>().ok());
    let mut cursor = match query.since.or(resume) {
        Some(seq) => seq,
        // Live only: start at the end of the log rather than replaying it.
        None => state.store().latest_seq().unwrap_or(0),
    };

    // A bounded channel *is* `16 local api surface`'s bounded per-connection buffer. When the
    // client stops reading, the poller below blocks on `send` - and the poller
    // is not a worker, so the worker keeps going.
    let (tx, rx) = tokio::sync::mpsc::channel(STREAM_BATCH);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(STREAM_POLL);
        loop {
            ticker.tick().await;
            let batch = match state.store().scan(cursor, STREAM_BATCH, &filter) {
                Ok(batch) => batch,
                Err(_) => continue,
            };
            // A tick with nothing to say still probes the channel, so a client
            // that hung up during a quiet stretch is noticed rather than polled
            // at forever.
            if batch.is_empty() {
                if tx.send(Ok(SseEvent::default().comment(""))).await.is_err() {
                    return;
                }
                continue;
            }
            for event in batch {
                cursor = event.seq;
                let sse = SseEvent::default()
                    .id(event.seq.to_string())
                    .event(event.kind.to_string())
                    .json_data(&event)
                    .unwrap_or_else(|_| SseEvent::default().comment("unserialisable event"));
                if tx.send(Ok(sse)).await.is_err() {
                    return; // the client hung up
                }
            }
        }
    });

    Ok(Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx)).keep_alive(KeepAlive::default()))
}

/// A run, on all three axes.
///
/// `16 local api surface`: **liveness is derived, never stored**, so there is no write path for it
/// and any client that caches it must recompute rather than trust a snapshot.
/// This is the one place where a naive CRUD shape would be actively wrong.
#[derive(Debug, Serialize)]
pub struct RunView {
    pub run_id: String,
    pub task_id: String,
    pub cell_id: String,
    pub runner: String,
    pub model: String,
    pub lifecycle: String,
    pub outcome: Option<String>,
    pub usd_micros: u64,
    pub tokens: u64,
    pub operator_touched: bool,
    pub started_ts: i64,
    pub finished_ts: Option<i64>,
    pub liveness_stalled_secs: u64,
    pub liveness_likely_hung_secs: u64,
    /// `18 hang detection prior art`/`05 run state model`'s watchdog state - `"live"`, `"stalled"` or `"likely_hung"` -
    /// or `None` when there is nothing in memory to ask: the run already
    /// finished, or farseer restarted since it started. `17 cell lifecycle` chose no orphan
    /// survival over run survival, so a restart losing this is the same
    /// trade already made everywhere else, not a new gap.
    pub liveness: Option<String>,
    /// What this run is *for*, in one line, derived from the goal it was queued
    /// with.
    ///
    /// Derived rather than stored, and read from the record rather than from a
    /// column: `run_queued` already carries the whole sealed contract, so the
    /// answer exists and asking the operator to also name a run would be asking
    /// them to repeat themselves. `None` when the record has no `run_queued` -
    /// which is a corrupt run rather than an untitled one, and says so by
    /// staying absent.
    pub title: Option<String>,
    /// `manager` or `worker`, as recorded when the run was queued.
    ///
    /// The difference an operator most wants on a fleet view - a manager is a
    /// conversation they can still steer, a worker is a job - and it was in the
    /// record all along while no surface showed it.
    pub role: Option<String>,
    /// Why a finished run finished the way it did, when the record says so.
    ///
    /// `17 cell lifecycle` chose no orphan survival, so a run whose farseer
    /// process is gone is reaped and marked `failed` - correct, because the run
    /// *had started* and its outcome is genuinely unknown. On a fleet view it
    /// is also indistinguishable from a run that tried and could not do the
    /// work, and a screen of `failed` after a restart reads as breakage.
    ///
    /// **Derived, not a new outcome.** Adding a fifth outcome word would change
    /// what `11 analytics questions` counts, to fix a presentation problem;
    /// `26 routing policy` refused a new outcome on the same grounds. The reason
    /// is already in `run_finished`, and this is the same move `title` makes
    /// with `run_queued` - read the record rather than add a column.
    pub finished_reason: Option<String>,
}

/// The fleet, newest first.
///
/// `16 local api surface` is additive-only, so this is a new operation rather
/// than a change to an existing one. It exists because a surface that can show
/// one run by id cannot show **which runs there are**, and `28 operator
/// surface`'s verb table is defined over a run line the operator can see.
async fn list_runs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListRunsQuery>,
) -> ApiResult<Json<Vec<RunView>>> {
    let limit = query.limit.unwrap_or(50).min(500);
    let rows = {
        let store = state.store();
        store.recent_runs(limit)?
    };
    Ok(Json(
        rows.into_iter().map(|row| run_view(&state, row)).collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ListRunsQuery {
    pub limit: Option<usize>,
}

async fn get_run(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<Json<RunView>> {
    let run_id: RunId = run_id.parse().map_err(|_| ApiError::NotFound("run"))?;
    let row = {
        let store = state.store();
        store.run(run_id)?
    }
    .ok_or(ApiError::NotFound("run"))?;

    Ok(Json(run_view(&state, row)))
}

/// One row, on all three axes.
///
/// Shared by the list and the single read so the two can never disagree about
/// what a run is - and `05 run state model` keeps liveness **derived** here,
/// asked of the live handle rather than read from the row.
/// The one-line name of a run, from the goal it was sealed with.
///
/// First non-empty line, clipped on a word boundary. Clipped here rather than in
/// each surface so two widgets cannot come to disagree about what a run is
/// called - `14 vocabulary lock`'s argument, applied to a derived string.
fn title_of(goal: &str) -> Option<String> {
    let line = goal.lines().map(str::trim).find(|line| !line.is_empty())?;
    const LIMIT: usize = 64;
    if line.chars().count() <= LIMIT {
        return Some(line.to_string());
    }
    let clipped: String = line.chars().take(LIMIT).collect();
    let cut = clipped.rfind(char::is_whitespace).unwrap_or(clipped.len());
    Some(format!("{}...", clipped[..cut].trim_end()))
}

/// The goal and role a run was queued with, read from its first recorded event.
///
/// One indexed row per run - `events_run(run_id, seq)` makes `LIMIT 1` cheap -
/// rather than the full scan `original_run` does, because a list of fifty runs
/// must not cost fifty full scans.
fn queued_facts(state: &Arc<AppState>, run_id: RunId) -> (Option<String>, Option<String>) {
    let store = state.store();
    let Ok(events) = store.scan(0, 1, &ScanFilter::run(run_id)) else {
        return (None, None);
    };
    let Some(queued) = events
        .first()
        .filter(|e| e.kind.as_str() == farseer_core::EventKind::RUN_QUEUED)
    else {
        return (None, None);
    };
    (
        queued
            .payload
            .get("goal")
            .and_then(serde_json::Value::as_str)
            .and_then(title_of),
        queued
            .payload
            .get(RUN_ROLE_FIELD)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    )
}

/// Where a cell's skills live: `<repo>/skills/<name>/SKILL.md`.
///
/// One layout, not a search path. `32 harness capability floor` found every
/// harness discovering skills from the operator's home directory, so a run got
/// whatever happened to be installed there; a cell that names a skill should get
/// **that** skill, from a place that is in the repository and reviewable.
pub(crate) fn skill_dir(repo_root: &std::path::Path, name: &str) -> PathBuf {
    repo_root.join("skills").join(name)
}

/// Resolve declared skill names to directories, refusing any that is not there.
///
/// Refused rather than skipped: a run that quietly drops a skill produces a
/// worse answer for a reason nobody can see, and `13 harness build kit` is
/// explicit that failing after the operator has committed is the bad direction.
/// A name with a path separator in it is refused too - a skill is a name in the
/// cell's own vocabulary, never a way to reach an arbitrary directory.
/// Resolve declared skill names to directories, for a runner that can load them.
///
/// The runner is checked here rather than at launch because dropping a declared
/// skill is invisible from the outside: the run works, the answer is worse, and
/// nothing in the record says why. `32 harness capability floor` made a cell
/// declare its skills so a run would stop inheriting whatever was installed on
/// the machine; a runner that silently ignores the declaration puts the run
/// straight back to not knowing what it loaded.
///
/// Same call `31 manager delegation reach` made about delegation, and the same
/// call [`mcp::FarseerMcp::delegate_to_worker_json`] already makes about a skill
/// that is not in the repository: refuse where a person can see it.
fn skill_paths(state: &AppState, runner: &str, names: &[String]) -> ApiResult<Vec<PathBuf>> {
    if !names.is_empty() && !runner_loads_skills(runner) {
        return Err(ApiError::Policy(format!(
            "runner `{runner}` cannot be given a skill by name, and this cell declares {names:?}"
        )));
    }
    names
        .iter()
        .map(|name| {
            if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
                return Err(ApiError::BadRequest("a skill is a name, not a path"));
            }
            let dir = skill_dir(&state.repo_root, name);
            if dir.join("SKILL.md").is_file() {
                Ok(dir)
            } else {
                Err(ApiError::NotFound("skill"))
            }
        })
        .collect()
}

/// The first runner in the author's order whose subscription window is not spent.
///
/// `26 routing policy` put the ordered list on the roster entry because **the
/// author asserts equivalence and farseer never infers it**. This is the other
/// half: farseer picks, and it picks by the one signal it gets for free.
///
/// Three states, per `26` section 2, and the third is the common one - a runner
/// farseer has never seen a window for is `unknown`, which behaves as
/// **available until proven otherwise**. The router tries, and a failure is what
/// teaches it. Most runners will never report a window, so `unknown` is
/// permanent rather than a gap to close.
///
/// `None` means every candidate is spent, which the caller turns into `26`
/// section 3's `runner_exhausted` rather than a fifth outcome.
fn first_available_runner(state: &AppState, candidates: &[String]) -> Option<String> {
    const EXHAUSTED: &str = "exhausted_until";

    let config = state.runner_config();
    let mut exhausted: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut on_overage: std::collections::HashSet<String> = std::collections::HashSet::new();
    for row in state
        .store()
        .windows(|account| config.runners_on(account))
        .unwrap_or_default()
    {
        let runners = config.runners_on(&row.account);
        if row.status == EXHAUSTED {
            exhausted.extend(runners.iter().cloned());
        }
        if row.is_using_overage {
            on_overage.extend(runners);
        }
    }
    let eligible = || candidates.iter().filter(|r| !exhausted.contains(*r));
    // `26 routing policy` section 4, and the shape is the whole point: a
    // subscription run costs **nothing marginal** until its window turns over,
    // and the only way to continue past that is a pay-per-token key that costs
    // real money. So the cost curve is not a slope a router shaves margins off.
    // It is **a cliff at the moment of exhaustion**, and `is_using_overage` is
    // the provider saying which side of it a runner is on.
    //
    // Observed, not estimated: farseer does not price a run in advance to decide
    // this, because the one number it would need is the one nobody has yet.
    // Preference only - a runner in overage still runs when it is all there is,
    // since `26` is explicit that budget pressure may **reorder within the
    // author's list** and never introduce or remove a runner the author named.
    eligible()
        .find(|runner| !on_overage.contains(*runner))
        .or_else(|| eligible().next())
        .cloned()
}

/// Refuse a tool level farseer cannot actually impose on this runner.
///
/// The third application of one rule, and the one `36 tool grant enforcement`
/// exists because of: a cell that opts down to `read` and gets a shell anyway
/// has been told nothing, and the record would say it was allowed less than it
/// was. `31` refuses a delegation a manager cannot make and `32` refuses a
/// skill a runner cannot load; this refuses a grant a runner cannot honour.
///
/// `ToolLevel::Shell` never refuses, because it asks for nothing to be imposed.
fn check_tool_level(runner: &str, level: farseer_core::ToolLevel) -> ApiResult<()> {
    if level != farseer_core::ToolLevel::Shell && !farseer_runner::pi::takes_tool_allowlist(runner)
    {
        return Err(ApiError::Policy(format!(
            "runner `{runner}` cannot be held to a tool allowlist, and this cell asks for `{}`",
            level.as_str()
        )));
    }
    Ok(())
}

/// Whether farseer has a way to hand this runner a named skill.
///
/// One arm today, and stated as a list rather than a negation so a new runner
/// is silent until somebody probes it - `10 runner inventory`'s observed-never-
/// advertised rule. omp is the instructive absence: it speaks pi's protocol
/// verbatim and shares pi's adapter, and its `--skills` is a glob filter over
/// what it discovered rather than a loader, so with discovery denied there is
/// no argv at all that gives it a directory.
fn runner_loads_skills(runner: &str) -> bool {
    farseer_runner::pi::loads_skills_by_path(runner)
}

/// Why the run ended, from its own last event.
///
/// The mirror of [`queued_facts`], which reads a run's first event - and it is
/// the same one query, pointed at the other end of the run.
fn finished_reason(state: &Arc<AppState>, run_id: RunId) -> Option<String> {
    let store = state.store();
    let events = store.scan_tail(1, &ScanFilter::run(run_id)).ok()?;
    let last = events.first()?;
    if last.kind.as_str() != farseer_core::EventKind::RUN_FINISHED {
        return None;
    }
    last.payload
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn run_view(state: &Arc<AppState>, row: RunRow) -> RunView {
    let run_id = row.run_id;
    let (title, role) = queued_facts(state, run_id);
    #[allow(clippy::let_and_return)]
    let view = RunView {
        run_id: run_id.to_string(),
        task_id: row.task_id.to_string(),
        cell_id: row.cell_id.to_string(),
        runner: row.runner,
        model: row.model,
        lifecycle: match (&row.outcome, row.finished_ts) {
            (Some(_), _) => "finished",
            (None, _) => "running",
        }
        .to_string(),
        outcome: row.outcome,
        usd_micros: row.usd_micros,
        tokens: row.tokens,
        operator_touched: row.operator_touched,
        started_ts: row.started_ts,
        finished_ts: row.finished_ts,
        liveness_stalled_secs: state.thresholds.stalled_secs,
        liveness_likely_hung_secs: state.thresholds.likely_hung_secs,
        liveness: state
            .runs()
            .get(&run_id)
            .map(|h| liveness_str(h.liveness.liveness()).to_string()),
        title,
        role,
        finished_reason: finished_reason(state, run_id),
    };
    view
}

#[cfg(test)]
mod title_tests {
    use super::title_of;

    #[test]
    fn a_short_goal_is_its_own_title() {
        assert_eq!(
            title_of("Reply with exactly: pi online.").as_deref(),
            Some("Reply with exactly: pi online.")
        );
    }

    /// A manager prompt is a paragraph. The first line is what the operator
    /// actually asked for; the rest is how.
    #[test]
    fn a_multi_line_goal_is_named_by_its_first_real_line() {
        assert_eq!(
            title_of(
                "

  Add a quota widget  
and make it live.
"
            )
            .as_deref(),
            Some("Add a quota widget")
        );
    }

    #[test]
    fn a_long_line_is_clipped_on_a_word_rather_than_mid_word() {
        let title = title_of(
            "Investigate why the manager reported a delegation that never happened and fix it",
        )
        .unwrap();
        assert!(title.ends_with("..."), "{title}");
        assert!(title.chars().count() <= 67, "{title}");
        assert!(!title.contains("happene."), "clipped mid-word: {title}");
    }

    /// An empty goal has no title rather than an empty one: `10 runner
    /// inventory`'s rule, that an absent answer stays absent.
    #[test]
    fn a_goal_with_nothing_in_it_has_no_title() {
        assert_eq!(
            title_of(
                "   
  
"
            ),
            None
        );
    }
}

fn liveness_str(liveness: farseer_core::Liveness) -> &'static str {
    match liveness {
        farseer_core::Liveness::Live => "live",
        farseer_core::Liveness::Stalled => "stalled",
        farseer_core::Liveness::LikelyHung => "likely_hung",
    }
}

async fn get_ui_state(
    State(state): State<Arc<AppState>>,
    UrlPath(key): UrlPath<String>,
) -> ApiResult<Response> {
    let blob = {
        let store = state.store();
        store.ui_state(&key)?
    };
    match blob {
        Some(blob) => {
            Ok(([(header::CONTENT_TYPE, "application/octet-stream")], blob).into_response())
        }
        None => Err(ApiError::NotFound("ui state")),
    }
}

/// `24 ui state persistence`: farseer stores an opaque blob and **never parses it**. No validation,
/// no schema, no reference-following. This is deliberately the only part of the
/// API that returns something farseer does not understand.
async fn put_ui_state(
    State(state): State<Arc<AppState>>,
    UrlPath(key): UrlPath<String>,
    body: axum::body::Bytes,
) -> ApiResult<StatusCode> {
    if body.len() > UI_STATE_CAP_BYTES {
        return Err(ApiError::Store(StoreError::UiStateTooLarge {
            key,
            size: body.len(),
            cap: UI_STATE_CAP_BYTES,
        }));
    }
    let store = state.store();
    store.put_ui_state(&key, &body, now_ms())?;
    Ok(StatusCode::NO_CONTENT)
}

/// `27 quota accounting`'s utilisation surface.
///
/// **Never a percentage.** `10 runner inventory` proved `used_percentage` reaches
/// only a status line that does not fire headless, and farseer's own spend is a
/// lower bound on the window - it would be most wrong exactly near exhaustion.
/// So this answers the question the operator actually has, which `27 quota
/// accounting` identified as "what has the fleet spent and which runners are
/// spending it", rather than the one nobody can answer honestly.
///
/// Accounting is keyed by **account** and display by **runner**, deliberately.
/// The skills a cell may declare, and who declares each.
///
/// `32 harness capability floor`'s third consequence asked for the **menu** to
/// show skills, on the grounds that they are the part of a harness an operator
/// has actually customised and the part farseer was most blind to.
///
/// **`32`'s own fix made that menu the wrong one.** Since discovery was denied,
/// a run reaches only what its cell names, and a cell may name only a directory
/// under the repository's `skills/`. Listing the twenty-odd skills pi discovers
/// on this machine would be a menu of things no cell can order - the failure
/// `13 harness build kit` names as the worst direction, an author committing to
/// something and finding out afterwards.
///
/// So the menu is the repository, and `also_declared_by` is the part an author
/// actually asks: is anything using this, and what does it do there.
async fn skills(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let cells = state.cells();
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let dir = state.repo_root().join("skills");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        let declared_by: Vec<String> = cells
            .values()
            .filter(|cell| {
                cell.manager.skills.contains(&name)
                    || cell.roster.iter().any(|entry| match entry {
                        farseer_core::RosterEntry::Worker { skills, .. } => skills.contains(&name),
                        _ => false,
                    })
            })
            .map(|cell| cell.cell_id.as_str().to_string())
            .collect();
        rows.push(serde_json::json!({ "name": name, "declared_by": declared_by }));
    }
    Ok(Json(serde_json::json!({ "skills": rows })))
}

async fn quota(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let config = state.runner_config();
    let windows = {
        let store = state.store();
        store.windows(|account| config.runners_on(account))?
    };
    let mut rows: Vec<serde_json::Value> = windows
        .into_iter()
        // A window keyed by a runner's own name, when that runner now declares
        // an account, is a **superseded reading and not a second subscription**.
        //
        // `27 quota accounting` keys a window by account and says the account is
        // declared by the operator, never inferred - so an undeclared runner is
        // keyed by its own name. The moment `runners.toml` gains
        // `[codex-app-server] account = "chatgpt"`, every observation already in
        // the record keeps the old key, and the widget showed one Codex
        // subscription twice, with two percentages a day apart.
        //
        // Filtered rather than deleted: `02 record scope` makes the record
        // append-only, and what those rows said was true when they were written.
        .filter(|window| config.account_for(&window.account) == window.account)
        .map(|window| {
            let runners = config.runners_on(&window.account);
            let mut value = serde_json::to_value(&window).unwrap_or_default();
            if let Some(object) = value.as_object_mut() {
                object.insert("runners".into(), serde_json::json!(runners));
                object.insert("source".into(), serde_json::json!("record"));
            }
            value
        })
        .collect();
    let polled = state
        .polled_windows
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    rows.extend(polled.into_iter().map(|window| {
        let mut value = serde_json::to_value(&window).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("runners".into(), serde_json::json!([window.runner]));
            object.insert("source".into(), serde_json::json!("omp usage"));
        }
        value
    }));
    // The surface must never offer a source the runtime cannot read, which is
    // `13 harness build kit`'s menu rule applied to a toggle: the widget lists
    // what is here and marks which one is on, rather than hard-coding a list
    // that goes stale the day a second source lands.
    Ok(Json(serde_json::json!({
        "windows": rows,
        "sources": farseer_core::runners::USAGE_SOURCES,
        "source": config.usage_source(),
    })))
}

/// Read every window omp can see **now**, then answer like [`quota`].
///
/// The background poll runs every five minutes because windows move on the
/// scale of hours, and that is the right default for something farseer does on
/// its own. It is the wrong answer for an operator who has just switched
/// accounts and is looking at the widget: the number they want is one process
/// launch away and they have to wait up to five minutes for it.
///
/// **A POST, and not a query parameter on the read.** This launches somebody
/// else's binary, which is not a read however it is spelled - and `16 local api
/// surface` would then have a GET with a side effect, which is the kind of
/// thing that gets retried by a proxy.
///
/// **Refuses when the poll is off** rather than quietly answering the cached
/// numbers. `13 harness build kit` made the poll a menu item an operator picks
/// (`usage_poll = true` in `runners.toml`), and a button that appears to do
/// something farseer was never given permission to do is the failure mode this
/// repository keeps naming: silence around a missing capability.
async fn refresh_quota(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    if state.runner_config().usage_source().is_none() {
        return Err(ApiError::BadRequest(
            "no usage source is configured; set `source = \"omp\"` under [usage] in runners.toml",
        ));
    }
    let windows = tokio::task::spawn_blocking(poll_windows)
        .await
        .unwrap_or_default();
    // Only overwrite with something, for the same reason the loop does: a failed
    // poll must leave the last good snapshot rather than blank the view.
    if !windows.is_empty() {
        *state
            .polled_windows
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = windows;
    }
    quota(State(state)).await
}

/// Refresh [`AppState::polled_windows`] on a slow loop, for as long as farseer runs.
///
/// Started from `serve`, so nothing polls in a test: a unit test that shells out
/// to a binary on the developer's machine is not testing farseer.
///
/// **Opt-in, and off by default.** Starting farseer used to launch `omp` whether
/// or not the operator had asked for a cross-provider quota reading, which is
/// farseer running somebody else's binary on its own initiative. `13 harness
/// build kit` made the inventory a menu an author picks from, and a poll nobody
/// chose is the opposite of that. Turn it on per runner in `runners.toml`:
///
/// ```toml
/// [omp]
/// usage_poll = true
/// ```
fn spawn_window_poll(state: Arc<AppState>) {
    if !state.runner_config().polls_usage("omp") {
        return;
    }
    tokio::spawn(async move {
        // Windows move on the scale of hours. Anything faster would be a
        // process launch nobody asked for.
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            ticker.tick().await;
            let windows = tokio::task::block_in_place(poll_windows);
            // Only overwrite with something. A failed poll leaves the last good
            // snapshot in place rather than blanking the operator's view.
            if !windows.is_empty() {
                *state
                    .polled_windows
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = windows;
            }
        }
    });
}

/// Every window omp is logged into, asked for now rather than remembered.
///
/// **A read, never an append.** `27 quota accounting` made the record's windows
/// a log of *transitions observed by a run*; this is a snapshot belonging to no
/// run and no cell, so writing it would need a cell id the record has no honest
/// value for. `27` also made current state **derived**, and a live poll is the
/// most derived thing there is.
///
/// This is farseer's only view of a window while nothing is running - every
/// other one arrives on a run's own stream, so an idle farseer learned nothing.
/// It is also the only reading of a Google quota at all, which `33 google quota`
/// closed as impossible against `agy`; see [`farseer_runner::omp_usage`].
///
/// Best-effort: omp absent, not logged in, or slow yields nothing, because a
/// missing side-channel must not fail the operator's quota view.
fn poll_windows() -> Vec<farseer_core::WindowObservation> {
    let Some(exe) = farseer_runner::resolve::resolve("omp") else {
        return Vec::new();
    };
    let mut command = std::process::Command::new(exe);
    command.args(farseer_runner::omp_usage::build_args());
    // **`CREATE_NO_WINDOW`, for the reason `03 spike job objects` named it a
    // root cause.** Without it this opened a console the operator could see and
    // close - and closing it delivered `CTRL_CLOSE` to farseer's own process
    // group, taking the daemon and the desktop shell down with it. A side
    // channel that improves a view was killing the product.
    //
    // Every child farseer spawns goes through `SupervisedProcess`, which has
    // always set this. This one did not, because it was written as "just run a
    // command" rather than as spawning a process, and that distinction does not
    // exist on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    }
    command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| farseer_runner::omp_usage::parse(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

macro_rules! analytics_route {
    ($name:ident, $method:ident, $row:ty) => {
        async fn $name(State(state): State<Arc<AppState>>) -> ApiResult<Json<Vec<$row>>> {
            let store = state.store();
            Ok(Json(store.$method()?))
        }
    };
}

analytics_route!(
    analytics_cost,
    cost_by_runner_and_model,
    farseer_store::CostRow
);
analytics_route!(
    analytics_intervention,
    intervention_rate_by_cell,
    farseer_store::InterventionRow
);
analytics_route!(analytics_rework, rework_depth, farseer_store::ReworkRow);
analytics_route!(
    analytics_lessons,
    lessons_against_outcome,
    farseer_store::LessonRow
);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Load definitions from a directory without standing up a server. Used by the
/// CLI's `validate`.
pub fn validate_dir(dir: &Path) -> ReloadReport {
    let state = AppState::new(
        Store::open_in_memory().expect("in-memory store"),
        dir,
        RuntimeToken::generate(),
        std::env::temp_dir(),
        std::env::temp_dir(),
    );
    state.reload()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use farseer_core::{Actor, NewEvent};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    const CELL: &str = r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "worktree"

[manager]
runner = "claude-code"

[[roster]]
kind = "tool"
name = "shell"
irreversibility = "reversible"
grants_shell = true
"#;

    struct Harness {
        router: Router,
        token: RuntimeToken,
        state: Arc<AppState>,
        _dir: tempfile::TempDir,
        _runs_dir: tempfile::TempDir,
        _repo: tempfile::TempDir,
    }

    /// A repo with one commit, exactly as `farseer-runner`'s own `workspace`
    /// tests need one - `git worktree add` needs a valid ref to detach at.
    fn git_repo_with_a_commit() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.email", "test@example.invalid"]);
        git(&["config", "user.name", "test"]);
        std::fs::write(dir.path().join("README.md"), "fixture\n").unwrap();
        git(&["add", "README.md"]);
        git(&["commit", "--quiet", "-m", "initial"]);
        dir
    }

    /// `22 cell addressing` section 1: a callable cell is a third roster entry
    /// kind, so granting one is an edit to the caller's definition.
    const CELL_THAT_MAY_CALL: &str = r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "worktree"

[manager]
runner = "claude-code"

[[roster]]
kind = "tool"
name = "shell"
irreversibility = "reversible"
grants_shell = true

[[roster]]
kind = "cell"
name = "social"
cell_id = "social"
max_autonomy_ceiling = "undoable"

[[roster]]
kind = "cell"
name = "abroad"
cell_id = "abroad"
max_autonomy_ceiling = "irreversible"
peer = true

[[roster]]
kind = "cell"
name = "ghost"
cell_id = "ghost"
max_autonomy_ceiling = "reversible"
"#;

    const CALLEE_CELL: &str = r#"
cell_id = "social"
name = "Social"
workspace_strategy = "plain_directory"

[manager]
runner = "claude-code"

[[roster]]
kind = "worker"
name = "writer"
runner = "codex"
"#;

    /// A callee whose manager runner does not exist, so the call is accepted and
    /// spawned and the callee's run then fails on its own. That is exactly the
    /// seam a fire-and-forget call needs to prove without spending a real one:
    /// acceptance is the caller's half, and the outcome is the callee's.
    const UNRUNNABLE_CALLEE_CELL: &str = r#"
cell_id = "social"
name = "Social"
workspace_strategy = "plain_directory"

[manager]
runner = "not-a-real-runner"
"#;

    /// The live cross-cell callee. Goose rather than Claude Code: the callee
    /// needs no MCP face of its own, and goose on this machine delegates
    /// through the already-authenticated `codex` CLI, so the probe spends no
    /// new credential.
    const LIVE_CALLEE_CELL: &str = r#"
cell_id = "social"
name = "Social"
workspace_strategy = "plain_directory"

[manager]
runner = "goose"

[[roster]]
kind = "tool"
name = "shell"
irreversibility = "reversible"
grants_shell = true
"#;

    const CELL_WITH_A_WORKER: &str = r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "worktree"

[manager]
runner = "claude-code"

[[roster]]
kind = "worker"
name = "coder"
runner = "goose"

[[roster]]
kind = "tool"
name = "shell"
irreversibility = "reversible"
grants_shell = true
"#;

    fn harness() -> Harness {
        harness_with_cell(CELL)
    }

    fn register_manager(h: &Harness) -> (RunId, TaskId, String) {
        let cell = h.state.cells().get(&CellId::new("zero")).cloned().unwrap();
        let run_id = RunId::new();
        let task_id = TaskId::new();
        let contract = WorkerContract::seal(WorkerContractSpec {
            run_id,
            task_id,
            cell_id: cell.cell_id.clone(),
            goal: "manage the task".into(),
            workspace: cell.workspace_strategy,
            runner: cell.manager.runner.clone(),
            tool_grants: cell.tool_grants(),
            tool_level: Default::default(),
            autonomy_ceiling: cell.policy.autonomy_ceiling,
            budget: cell.budget,
            definition_of_done: String::new(),
        });
        let manager_token = RuntimeToken::generate();
        let token_text = manager_token.as_str().to_string();
        h.state.managers().insert(
            run_id,
            ManagerContext {
                manager_token,
                remaining_budget: Arc::new(Mutex::new(contract.budget)),
                child_runs: Arc::new(Mutex::new(HashSet::new())),
                cancel_requested: Arc::new(AtomicBool::new(false)),
                contract,
                cell,
            },
        );
        (run_id, task_id, token_text)
    }

    fn harness_with_cell(cell_toml: &str) -> Harness {
        harness_with_cells(&[("zero", cell_toml)])
    }

    fn harness_with_cells(cells: &[(&str, &str)]) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        for (name, toml) in cells {
            std::fs::write(dir.path().join(format!("{name}.toml")), toml).unwrap();
        }
        let runs_dir = tempfile::tempdir().unwrap();
        let repo = git_repo_with_a_commit();
        let token = RuntimeToken::generate();
        let state = Arc::new(AppState::new(
            Store::open_in_memory().unwrap(),
            dir.path(),
            token.clone(),
            runs_dir.path(),
            repo.path(),
        ));
        state.reload();
        Harness {
            router: router(state.clone()),
            token,
            state,
            _dir: dir,
            _runs_dir: runs_dir,
            _repo: repo,
        }
    }

    impl Harness {
        fn request(&self, method: &str, uri: &str) -> Request<Body> {
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::HOST, "127.0.0.1:9000")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", self.token.as_str()),
                )
                .body(Body::empty())
                .unwrap()
        }

        async fn send(&self, request: Request<Body>) -> (StatusCode, serde_json::Value) {
            let response = self.router.clone().oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            (status, body)
        }

        async fn get(&self, uri: &str) -> (StatusCode, serde_json::Value) {
            self.send(self.request("GET", uri)).await
        }

        async fn post(
            &self,
            uri: &str,
            body: serde_json::Value,
        ) -> (StatusCode, serde_json::Value) {
            let mut request = self.request("POST", uri);
            *request.body_mut() = Body::from(body.to_string());
            request
                .headers_mut()
                .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
            self.send(request).await
        }

        async fn wait_for_finished(
            &self,
            run_id: &str,
            timeout: std::time::Duration,
        ) -> serde_json::Value {
            let deadline = std::time::Instant::now() + timeout;
            loop {
                let (status, row) = self.get(&format!("/v1/runs/{run_id}")).await;
                if status == StatusCode::OK && row["lifecycle"] == "finished" {
                    return row;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the run never reached a terminal state"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }

        /// Cancels immediately, then polls until the run row reads `finished`.
        async fn cancel_and_wait_for_finished(&self, run_id: &str) -> serde_json::Value {
            self.post(&format!("/v1/runs/{run_id}/cancel"), json!({}))
                .await;
            self.wait_for_finished(run_id, std::time::Duration::from_secs(30))
                .await
        }
    }

    #[tokio::test]
    async fn a_request_without_a_token_is_refused() {
        let h = harness();
        let request = Request::builder()
            .uri("/v1/health")
            .header(header::HOST, "127.0.0.1:9000")
            .body(Body::empty())
            .unwrap();
        assert_eq!(h.send(request).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_wrong_token_is_refused() {
        let h = harness();
        let request = Request::builder()
            .uri("/v1/health")
            .header(header::HOST, "127.0.0.1:9000")
            .header(header::AUTHORIZATION, "Bearer nope")
            .body(Body::empty())
            .unwrap();
        assert_eq!(h.send(request).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_manager_bearer_cannot_call_operator_routes() {
        let h = harness();
        let (_, _, manager_token) = register_manager(&h);
        let request = Request::builder()
            .uri("/v1/cells")
            .header(header::HOST, "127.0.0.1:9000")
            .header(header::AUTHORIZATION, format!("Bearer {manager_token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(h.send(request).await.0, StatusCode::UNAUTHORIZED);
    }

    // Multi-threaded because the worker handler steps out of the async pool
    // with `block_in_place` - a delegation blocks for a real worker lifetime.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_manager_bearer_reaches_the_delegation_face_it_was_issued_for() {
        // The other half of `a_manager_bearer_cannot_call_operator_routes`.
        // `31 manager delegation reach` added a second manager-scoped face for
        // the runners with no MCP client, so the guard now has a prefix set
        // rather than one path - and a set is the kind of thing that quietly
        // grows. This pins that the new prefix is open to a manager and the
        // operator's routes are not, in the same breath.
        let h = harness();
        let (manager_run_id, _, manager_token) = register_manager(&h);
        let request = Request::builder()
            .method("POST")
            .uri("/v1/manager/delegate/worker")
            .header(header::HOST, "127.0.0.1:9000")
            .header(header::AUTHORIZATION, format!("Bearer {manager_token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "manager_run_id": manager_run_id.to_string(),
                    "manager_token": manager_token,
                    "worker": "not-in-the-roster",
                    "goal": "anything",
                })
                .to_string(),
            ))
            .unwrap();
        // A refusal by the roster, not by the guard: the call was authorized
        // and then answered on its merits, which is the whole distinction.
        let (status, body) = h.send(request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.to_string().contains("pinned roster"), "{body}");
    }

    #[tokio::test]
    async fn a_rebound_host_is_refused_before_the_token_is_even_checked() {
        // The CVE-2025-9074 shape: the browser would attach the token for the
        // attacker, so the token cannot be the thing that saves us here.
        let h = harness();
        let request = Request::builder()
            .uri("/v1/health")
            .header(header::HOST, "evil.example.com")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", h.token.as_str()),
            )
            .body(Body::empty())
            .unwrap();
        assert_eq!(h.send(request).await.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_cross_site_origin_is_refused() {
        let h = harness();
        let mut request = h.request("GET", "/v1/health");
        request
            .headers_mut()
            .insert(header::ORIGIN, "https://evil.example.com".parse().unwrap());
        assert_eq!(h.send(request).await.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_good_request_reaches_the_handler() {
        let h = harness();
        let (status, body) = h.get("/v1/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["api_version"], "v1");
    }

    #[tokio::test]
    async fn cells_are_listed_and_fetched_but_never_edited() {
        let h = harness();
        let (_, list) = h.get("/v1/cells").await;
        assert_eq!(list[0]["cell_id"], "zero");

        let (status, cell) = h.get("/v1/cells/zero").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cell["manager"]["runner"], "claude-code");

        assert_eq!(h.get("/v1/cells/missing").await.0, StatusCode::NOT_FOUND);

        // There is no edit path, per `16 local api surface` section 6.
        let put = h.request("PUT", "/v1/cells/zero");
        assert_eq!(h.send(put).await.0, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn a_broken_definition_is_reported_by_reload_rather_than_crashing_the_runtime() {
        let h = harness();
        std::fs::write(h._dir.path().join("broken.toml"), "cell_id = ").unwrap();
        let (status, report) = h.send(h.request("POST", "/v1/cells/reload")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(report["loaded"], json!(["zero"]));
        assert_eq!(report["errors"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_record_is_read_by_cursor_and_scoped_server_side() {
        let h = harness();
        let run = RunId::new();
        {
            let store = h.state.store.lock().unwrap();
            for i in 0..3 {
                store
                    .append(&NewEvent::new(
                        CellId::new("zero"),
                        run,
                        "tool_result",
                        Actor::Worker,
                        i,
                        json!({"i": i}),
                    ))
                    .unwrap();
            }
            store
                .append(&NewEvent::new(
                    CellId::new("social"),
                    RunId::new(),
                    "tool_result",
                    Actor::Worker,
                    9,
                    json!({}),
                ))
                .unwrap();
        }

        let (_, all) = h.get("/v1/events").await;
        assert_eq!(all.as_array().unwrap().len(), 4);

        let (_, scoped) = h.get("/v1/events?cell=zero").await;
        assert_eq!(scoped.as_array().unwrap().len(), 3);

        let (_, after) = h.get("/v1/events?since=2").await;
        assert_eq!(after[0]["seq"], 3, "since is exclusive");
    }

    #[tokio::test]
    async fn a_malformed_run_id_is_refused_rather_than_widening_the_query() {
        let h = harness();
        {
            let store = h.state.store();
            store
                .append(&NewEvent::new(
                    CellId::new("zero"),
                    RunId::new(),
                    "tool_result",
                    Actor::Worker,
                    1,
                    json!({}),
                ))
                .unwrap();
        }
        let (status, _) = h.get("/v1/events?run=not-a-uuid").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "a typo must not return every other run's events"
        );
        assert_eq!(
            h.get("/v1/stream?run=not-a-uuid").await.0,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn ui_state_round_trips_as_an_opaque_blob() {
        let h = harness();
        let body = br#"{"widgets":[{"id":"trades"}]}"#;
        let put = Request::builder()
            .method("PUT")
            .uri("/v1/ui-state/command-center")
            .header(header::HOST, "127.0.0.1:9000")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", h.token.as_str()),
            )
            .body(Body::from(&body[..]))
            .unwrap();
        assert_eq!(h.send(put).await.0, StatusCode::NO_CONTENT);

        let response = h
            .router
            .clone()
            .oneshot(h.request("GET", "/v1/ui-state/command-center"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&bytes[..], &body[..]);
    }

    #[tokio::test]
    async fn an_unwritten_ui_key_is_a_404_and_an_oversized_one_is_a_413() {
        let h = harness();
        assert_eq!(h.get("/v1/ui-state/never").await.0, StatusCode::NOT_FOUND);

        let oversized = vec![0u8; UI_STATE_CAP_BYTES + 1];
        let put = Request::builder()
            .method("PUT")
            .uri("/v1/ui-state/big")
            .header(header::HOST, "127.0.0.1:9000")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", h.token.as_str()),
            )
            .body(Body::from(oversized))
            .unwrap();
        assert_eq!(h.send(put).await.0, StatusCode::PAYLOAD_TOO_LARGE);

        let long_key = "k".repeat(farseer_store::UI_STATE_KEY_CAP_BYTES + 1);
        let put = Request::builder()
            .method("PUT")
            .uri(format!("/v1/ui-state/{long_key}"))
            .header(header::HOST, "127.0.0.1:9000")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", h.token.as_str()),
            )
            .body(Body::from("x"))
            .unwrap();
        assert_eq!(h.send(put).await.0, StatusCode::URI_TOO_LONG);
    }

    #[tokio::test]
    async fn every_analytics_query_answers_on_an_empty_record() {
        let h = harness();
        for path in [
            "/v1/analytics/cost",
            "/v1/analytics/intervention",
            "/v1/analytics/rework",
            "/v1/analytics/lessons",
        ] {
            let (status, body) = h.get(path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(body, json!([]), "{path}");
        }
    }

    #[tokio::test]
    async fn a_missing_run_is_a_404_rather_than_a_500() {
        let h = harness();
        assert_eq!(h.get("/v1/runs/not-a-uuid").await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            h.get(&format!("/v1/runs/{}", RunId::new())).await.0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn instructing_an_unknown_cell_is_a_404() {
        let h = harness();
        let (status, _) = h
            .post("/v1/cells/nope/instruct", json!({ "goal": "do the thing" }))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_empty_goal_is_refused_rather_than_spawning_nothing() {
        let h = harness();
        let (status, _) = h
            .post("/v1/cells/zero/instruct", json!({ "goal": "   " }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_native_manager_without_an_explicit_shell_grant_is_refused_before_workspace_creation()
    {
        let h = harness_with_cell(
            r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "worktree"

[manager]
runner = "claude-code"
"#,
        );
        let (status, body) = h
            .post("/v1/cells/zero/instruct", json!({ "goal": "do the thing" }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            std::fs::read_dir(&h.state.runs_dir)
                .unwrap()
                .next()
                .is_none(),
            "authority refusal must happen before creating a workspace"
        );
    }

    #[tokio::test]
    async fn a_currency_budget_is_refused_before_claude_can_overspend_it() {
        let h = harness_with_cell(
            r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "worktree"
budget = { usd_micros = 1 }

[manager]
runner = "claude-code"

[[roster]]
kind = "tool"
name = "shell"
irreversibility = "reversible"
grants_shell = true
"#,
        );
        let (status, body) = h
            .post("/v1/cells/zero/instruct", json!({ "goal": "do the thing" }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains("currency"),
            "the refusal must name the dimension Claude cannot enforce: {body}"
        );
        assert!(
            std::fs::read_dir(&h.state.runs_dir)
                .unwrap()
                .next()
                .is_none(),
            "budget refusal must happen before creating a workspace"
        );
    }

    #[test]
    fn manager_mcp_config_is_outside_the_git_worktree() {
        let h = harness_with_cell(CELL_WITH_A_WORKER);
        h.state.set_mcp_endpoint(8787);
        let (run_id, _, manager_token) = register_manager(&h);
        let manager = h.state.manager(run_id).unwrap();

        let options = manager_run_options(
            &h.state,
            &manager.contract,
            &manager.cell,
            &manager.manager_token,
        )
        .unwrap();
        let config_path = options.claude_mcp_config.unwrap();
        assert!(
            config_path.starts_with(
                security::runtime_file_path()
                    .parent()
                    .unwrap()
                    .join("manager-configs")
            )
        );
        assert!(!config_path.starts_with(h._repo.path()));
        let config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["farseer"]["type"], "http");
        assert_eq!(
            config["mcpServers"]["farseer"]["headers"]["Authorization"],
            format!("Bearer {manager_token}")
        );
        assert!(
            !std::fs::read_to_string(&config_path)
                .unwrap()
                .contains(h.token.as_str()),
            "a manager config must not disclose the process-wide operator token"
        );
        std::fs::remove_file(config_path).unwrap();
    }

    #[test]
    fn the_cell_worker_cap_is_shared_across_manager_runs() {
        let h = harness();
        let cell_id = CellId::new("zero");
        let first = h.state.acquire_worker(&cell_id, 1).unwrap();
        assert!(h.state.acquire_worker(&cell_id, 1).is_none());
        drop(first);
        assert!(h.state.acquire_worker(&cell_id, 1).is_some());
    }

    #[tokio::test]
    async fn cancelling_a_manager_also_cancels_its_active_delegated_worker() {
        let h = harness_with_cell(CELL_WITH_A_WORKER);
        let (manager_run_id, _, _) = register_manager(&h);
        let child_run_id = RunId::new();
        h.state
            .manager(manager_run_id)
            .unwrap()
            .child_runs
            .lock()
            .unwrap()
            .insert(child_run_id);

        let spawn = || {
            farseer_manager::StartedWorker::spawn(
                std::path::Path::new(r"C:\Windows\System32\cmd.exe"),
                &["/c".into(), "ping -n 30 127.0.0.1 >nul".into()],
                &std::env::current_dir().unwrap(),
                &[],
                LivenessThresholds::default(),
                farseer_runner::claude_code::parse_line,
                farseer_manager::Channel::OneShot,
            )
            .unwrap()
        };
        let manager_worker = spawn();
        let child_worker = spawn();
        let manager_token = manager_worker.cancel_token();
        let child_token = child_worker.cancel_token();
        h.state.runs().insert(
            manager_run_id,
            RunHandle {
                cancel: manager_token.clone(),
                liveness: manager_worker.liveness_handle(),
                steer: None,
            },
        );
        h.state.runs().insert(
            child_run_id,
            RunHandle {
                cancel: child_token.clone(),
                liveness: child_worker.liveness_handle(),
                steer: None,
            },
        );

        assert_eq!(
            h.post(&format!("/v1/runs/{manager_run_id}/cancel"), json!({}))
                .await
                .0,
            StatusCode::ACCEPTED
        );
        assert!(manager_token.was_cancelled());
        assert!(child_token.was_cancelled());
    }

    #[tokio::test]
    async fn cancelling_an_unknown_run_is_a_404() {
        let h = harness();
        assert_eq!(
            h.post(&format!("/v1/runs/{}/cancel", RunId::new()), json!({}))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            h.post("/v1/runs/not-a-uuid/cancel", json!({})).await.0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn cancellation_is_accepted_before_the_process_exposes_a_live_handle() {
        let h = harness();
        let run_id = RunId::new();
        let requested = Arc::new(AtomicBool::new(false));
        h.state
            .pending_cancellations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(run_id, Arc::clone(&requested));

        let (status, body) = h
            .post(&format!("/v1/runs/{run_id}/cancel"), json!({}))
            .await;

        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert!(
            requested.load(Ordering::Acquire),
            "the request must be waiting for the process startup callback"
        );
    }

    #[tokio::test]
    async fn steering_an_unknown_run_is_a_404() {
        let h = harness();
        assert_eq!(
            h.post(
                &format!("/v1/runs/{}/steer", RunId::new()),
                json!({ "message": "keep going" })
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real headless `claude` process - see the note on \
                `steering_a_claude_code_run_reaches_its_live_stdin`"]
    async fn steering_with_an_empty_message_is_a_400() {
        let h = harness();
        let (_, body) = h
            .post(
                "/v1/cells/zero/instruct",
                json!({ "goal": "reply with just the word ok" }),
            )
            .await;
        let run_id = body["run_id"].as_str().unwrap().to_string();

        assert_eq!(
            h.post(
                &format!("/v1/runs/{run_id}/steer"),
                json!({ "message": "   " })
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );

        h.cancel_and_wait_for_finished(&run_id).await;
    }

    #[tokio::test]
    #[ignore = "spawns a real headless `claude` process - slow (cold-start plugin \
                load cost per AGENTS.md) and excluded from `cargo test --workspace` \
                so the default run stays fast; run with `-- --ignored` to include it"]
    async fn steering_a_claude_code_run_reaches_its_live_stdin() {
        // `claude_code::steer_frame`'s 2026-08-23 probe verified the wire
        // format; this proves the seam - a later HTTP request reaching a
        // live process's stdin - actually exists end to end.
        let h = harness();
        let (_, body) = h
            .post(
                "/v1/cells/zero/instruct",
                json!({ "goal": "reply with just the word ok" }),
            )
            .await;
        let run_id = body["run_id"].as_str().unwrap().to_string();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let (status, _) = h
                .post(
                    &format!("/v1/runs/{run_id}/steer"),
                    json!({ "message": "never mind, just say done" }),
                )
                .await;
            if status == StatusCode::ACCEPTED {
                break;
            }
            // `on_started` has not populated the registry yet - same race
            // `a_running_run_reports_liveness_and_a_finished_one_reports_none`
            // documents for `liveness`.
            assert!(
                std::time::Instant::now() < deadline,
                "steer never became reachable for this run"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        h.cancel_and_wait_for_finished(&run_id).await;
    }

    /// One of several tests in this suite that spawn a real process - see the
    /// `#[ignore]` on each for why. The mechanics of spawning, reaping and
    /// mapping stream-json are already proven against `cmd.exe` fixtures in
    /// `farseer-runner` and `farseer-manager`'s own tests, so this only needs
    /// to prove the HTTP wiring around them: fire-and-forget returns a
    /// `run_id` before the run finishes, the record picks it up, and a
    /// workspace directory exists. Whether `claude` is actually installed on
    /// the machine running this test is deliberately not asserted either way
    /// - `10 runner inventory` confirms it is on the dev machine, but the row this test waits
    /// for reaches a terminal state either way: `ok`/`failed` if it ran,
    /// `failed` immediately via `ExecutableNotFound` if it did not.
    #[tokio::test]
    #[ignore = "spawns a real headless `claude` process - see the note on \
                `steering_a_claude_code_run_reaches_its_live_stdin`"]
    async fn instructing_a_cell_returns_a_run_id_that_becomes_a_real_queryable_run() {
        let h = harness();
        let (status, body) = h
            .post(
                "/v1/cells/zero/instruct",
                json!({ "goal": "reply with just the word ok" }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let run_id = body["run_id"].as_str().unwrap().to_string();

        let row = h.cancel_and_wait_for_finished(&run_id).await;
        assert_eq!(row["cell_id"], "zero");
        assert_eq!(row["runner"], "claude-code");

        // `04 spike workspace teardown`'s ordering constraint, proven end to end: the workspace this
        // run's `git worktree add` created is gone by the time this test's
        // own bounded wait ends - teardown ran, and it ran only after the
        // process that held the directory as its cwd had already exited.
        let workspace = h.state.runs_dir.join(&run_id);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while workspace.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "the workspace was never torn down"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    #[ignore = "spawns a real headless `claude` process - see the note on \
                `steering_a_claude_code_run_reaches_its_live_stdin`"]
    async fn a_running_run_reports_liveness_and_a_finished_one_reports_none() {
        let h = harness();
        let (_, body) = h
            .post(
                "/v1/cells/zero/instruct",
                json!({ "goal": "reply with just the word ok" }),
            )
            .await;
        let run_id = body["run_id"].as_str().unwrap().to_string();

        // The row is written before `start_worker` even resolves the
        // executable, and `on_started` - the callback that populates the
        // in-memory registry `liveness` reads from - only fires once a
        // process actually spawns. So a run can genuinely be `running` with
        // `liveness: null` for a moment before it becomes `"live"`, and it
        // can also race straight to `finished` before either is observed.
        // Poll until one of those three states is confirmed rather than
        // asserting against a single, timing-dependent snapshot.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let (_, row) = h.get(&format!("/v1/runs/{run_id}")).await;
            if row["liveness"] == "live" {
                break;
            }
            if row["lifecycle"] == "finished" {
                assert!(row["liveness"].is_null());
                return; // finished before ever being observed live - fine
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the run was never observed live nor finished: {row}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let row = h.cancel_and_wait_for_finished(&run_id).await;
        assert!(
            row["liveness"].is_null(),
            "a finished run has no in-memory liveness handle left: {row}"
        );
    }

    #[test]
    fn a_pre_manager_loop_run_remains_reconstructable_without_gaining_delegation() {
        let h = harness();
        let run_id = RunId::new();
        let spec = WorkerContractSpec {
            run_id,
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "legacy goal".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "claude-code".into(),
            tool_grants: Vec::new(),
            tool_level: Default::default(),
            autonomy_ceiling: farseer_core::policy::Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: String::new(),
        };
        h.state
            .store()
            .append(&NewEvent::new(
                CellId::new("zero"),
                run_id,
                farseer_core::EventKind::RUN_QUEUED,
                Actor::Operator,
                1,
                serde_json::to_value(&spec).unwrap(),
            ))
            .unwrap();

        let original = original_run(&h.state, run_id).unwrap();
        assert_eq!(original.role, RunRole::Manager);
        assert!(original.manager_cell.is_none());
        assert_eq!(original.spec.goal, "legacy goal");
    }

    #[tokio::test]
    async fn a_pinned_delegated_worker_can_be_rerun_by_the_operator() {
        let h = harness_with_cell(CELL_WITH_A_WORKER);
        let cell = h.state.cells().get(&CellId::new("zero")).cloned().unwrap();
        let run_id = RunId::new();
        let spec = WorkerContractSpec {
            run_id,
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "delegated goal".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "not-a-real-runner".into(),
            tool_grants: vec!["shell".into()],
            tool_level: Default::default(),
            autonomy_ceiling: farseer_core::policy::Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: String::new(),
        };
        let mut payload = serde_json::to_value(&spec).unwrap();
        payload.as_object_mut().unwrap().insert(
            RUN_ROLE_FIELD.into(),
            serde_json::Value::String(RunRole::Worker.as_record_str().into()),
        );
        payload.as_object_mut().unwrap().insert(
            MANAGER_CELL_FIELD.into(),
            serde_json::to_value(cell).unwrap(),
        );
        h.state
            .store()
            .append(&NewEvent::new(
                CellId::new("zero"),
                run_id,
                farseer_core::EventKind::RUN_QUEUED,
                Actor::Manager,
                1,
                payload,
            ))
            .unwrap();

        let (status, body) = h.post(&format!("/v1/runs/{run_id}/rerun"), json!({})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let rerun_id = body["run_id"].as_str().unwrap();
        let row = h
            .wait_for_finished(rerun_id, std::time::Duration::from_secs(10))
            .await;
        assert_eq!(row["outcome"], "failed", "{row}");

        let reconstructed = original_run(&h.state, rerun_id.parse().unwrap()).unwrap();
        assert_eq!(reconstructed.role, RunRole::Worker);
        assert!(
            reconstructed.manager_cell.is_some(),
            "the operator's rerun must retain the worker's pinned cell authority"
        );
    }

    #[tokio::test]
    async fn legacy_runs_without_a_pinned_definition_fail_closed_on_respawn() {
        let h = harness();
        for (role, actor, endpoint) in [
            (RunRole::Manager, Actor::Operator, "rerun"),
            (RunRole::Worker, Actor::Manager, "rescope"),
        ] {
            let run_id = RunId::new();
            let spec = WorkerContractSpec {
                run_id,
                task_id: TaskId::new(),
                cell_id: CellId::new("zero"),
                goal: "legacy goal".into(),
                workspace: WorkspaceStrategy::Worktree,
                runner: "not-a-real-runner".into(),
                tool_grants: Vec::new(),
                tool_level: Default::default(),
                autonomy_ceiling: farseer_core::policy::Irreversibility::Reversible,
                budget: Budget::default(),
                definition_of_done: String::new(),
            };
            let mut payload = serde_json::to_value(&spec).unwrap();
            payload.as_object_mut().unwrap().insert(
                RUN_ROLE_FIELD.into(),
                serde_json::Value::String(role.as_record_str().into()),
            );
            h.state
                .store()
                .append(&NewEvent::new(
                    CellId::new("zero"),
                    run_id,
                    farseer_core::EventKind::RUN_QUEUED,
                    actor,
                    1,
                    payload,
                ))
                .unwrap();

            let body = if endpoint == "rerun" {
                json!({})
            } else {
                json!({ "goal": "changed goal" })
            };
            let (status, response) = h.post(&format!("/v1/runs/{run_id}/{endpoint}"), body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
            assert!(
                response["error"]
                    .as_str()
                    .is_some_and(|error| error.contains("pinned")),
                "legacy authority must fail closed: {response}"
            );
        }
    }

    #[tokio::test]
    async fn a_worker_rerun_reacquires_the_pinned_cells_worker_permit() {
        let h = harness_with_cell(CELL_WITH_A_WORKER);
        let cell = h.state.cells().get(&CellId::new("zero")).cloned().unwrap();
        let run_id = RunId::new();
        let spec = WorkerContractSpec {
            run_id,
            task_id: TaskId::new(),
            cell_id: cell.cell_id.clone(),
            goal: "delegated goal".into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "not-a-real-runner".into(),
            tool_grants: vec!["shell".into()],
            tool_level: Default::default(),
            autonomy_ceiling: farseer_core::policy::Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: String::new(),
        };
        let mut payload = serde_json::to_value(&spec).unwrap();
        payload.as_object_mut().unwrap().insert(
            RUN_ROLE_FIELD.into(),
            serde_json::Value::String(RunRole::Worker.as_record_str().into()),
        );
        payload.as_object_mut().unwrap().insert(
            MANAGER_CELL_FIELD.into(),
            serde_json::to_value(&cell).unwrap(),
        );
        h.state
            .store()
            .append(&NewEvent::new(
                cell.cell_id.clone(),
                run_id,
                farseer_core::EventKind::RUN_QUEUED,
                Actor::Manager,
                1,
                payload,
            ))
            .unwrap();

        let permit = h
            .state
            .acquire_worker(&cell.cell_id, cell.policy.worker_cap)
            .unwrap();
        let mut remaining = Vec::new();
        for _ in 1..cell.policy.worker_cap {
            remaining.push(
                h.state
                    .acquire_worker(&cell.cell_id, cell.policy.worker_cap)
                    .unwrap(),
            );
        }

        let (status, body) = h.post(&format!("/v1/runs/{run_id}/rerun"), json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|error| error.contains("worker cap")),
            "the pinned cell-wide cap must be enforced: {body}"
        );
        drop(remaining);
        drop(permit);
    }

    #[tokio::test]
    async fn rerunning_an_unknown_run_is_a_404() {
        let h = harness();
        assert_eq!(
            h.post(&format!("/v1/runs/{}/rerun", RunId::new()), json!({}))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    #[ignore = "spawns a real headless `claude` process - see the note on \
                `steering_a_claude_code_run_reaches_its_live_stdin`"]
    async fn rescoping_without_a_goal_is_refused() {
        let h = harness();
        let (_, body) = h
            .post("/v1/cells/zero/instruct", json!({ "goal": "first goal" }))
            .await;
        let run_id = body["run_id"].as_str().unwrap();
        h.cancel_and_wait_for_finished(run_id).await;

        let (status, _) = h
            .post(&format!("/v1/runs/{run_id}/rescope"), json!({}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "spawns a real headless `claude` process - see the note on \
                `steering_a_claude_code_run_reaches_its_live_stdin`"]
    async fn rescoping_with_the_unchanged_goal_is_refused() {
        let h = harness();
        let (_, body) = h
            .post("/v1/cells/zero/instruct", json!({ "goal": "same goal" }))
            .await;
        let run_id = body["run_id"].as_str().unwrap();
        h.cancel_and_wait_for_finished(run_id).await;

        let (status, _) = h
            .post(
                &format!("/v1/runs/{run_id}/rescope"),
                json!({ "goal": "same goal" }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "spawns a real headless `claude` process - see the note on \
                `steering_a_claude_code_run_reaches_its_live_stdin`"]
    async fn rerun_and_rescope_start_a_fresh_run_linked_to_the_original() {
        let h = harness();
        let (_, body) = h
            .post(
                "/v1/cells/zero/instruct",
                json!({ "goal": "original goal" }),
            )
            .await;
        let original_id = body["run_id"].as_str().unwrap().to_string();
        h.cancel_and_wait_for_finished(&original_id).await;

        let (status, body) = h
            .post(&format!("/v1/runs/{original_id}/rerun"), json!({}))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let rerun_id = body["run_id"].as_str().unwrap().to_string();
        assert_ne!(rerun_id, original_id, "rerun mints a fresh run id");
        assert_eq!(body["parent_run_id"], original_id);
        h.cancel_and_wait_for_finished(&rerun_id).await;

        let (status, body) = h
            .post(
                &format!("/v1/runs/{original_id}/rescope"),
                json!({ "goal": "a genuinely different goal" }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let rescope_id = body["run_id"].as_str().unwrap().to_string();
        h.cancel_and_wait_for_finished(&rescope_id).await;

        // `11 analytics questions`'s rework-depth query walks the same `rescoped_from` edge both
        // verbs write - proof the link landed where analytics already reads
        // from, not a parallel record only this endpoint understands.
        let (_, rework) = h.get("/v1/analytics/rework").await;
        let depths: Vec<i64> = rework
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["depth"].as_i64().unwrap())
            .collect();
        assert!(
            depths.contains(&2),
            "two chains of depth 2 (original -> rerun, original -> rescope) should be visible: {rework}"
        );
    }

    /// Real `rmcp` client, real TCP, real MCP handshake - not a hand-rolled
    /// JSON-RPC request, per the project's own rule against guessing a
    /// wire-format fact. Covers the whole face `02 record scope` section 8 describes:
    /// write, read-back, the run-attributed `memory_consulted` edge, and the
    /// global tier's refusal.
    #[tokio::test]
    async fn the_mcp_face_writes_reads_back_and_refuses_the_global_tier() {
        use rmcp::ServiceExt;
        use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};
        use rmcp::transport::StreamableHttpClientTransport;
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

        let h = harness();
        let (manager_run_id, task_id, manager_token) = register_manager(&h);
        h.state
            .store()
            .upsert_run(&RunRow {
                run_id: manager_run_id,
                task_id,
                cell_id: CellId::new("zero"),
                runner: "claude-code".into(),
                model: String::new(),
                outcome: None,
                usd_micros: 0,
                tokens: 0,
                operator_touched: false,
                started_ts: 1,
                finished_ts: None,
            })
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = h.router.clone();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let transport = StreamableHttpClientTransport::<reqwest::Client>::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!(
                "http://127.0.0.1:{port}/v1/mcp"
            ))
            .auth_header(h.token.as_str().to_string()),
        );
        let client = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("farseer-api test client", "0.0.1"),
        )
        .serve(transport)
        .await
        .unwrap();

        let tools = client.list_tools(Default::default()).await.unwrap();
        let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"read_memory"));
        assert!(names.contains(&"write_memory"));

        let impersonation = client
            .call_tool(
                CallToolRequestParams::new("read_memory").with_arguments(
                    json!({
                        "manager_run_id": manager_run_id.to_string(),
                        "manager_token": "wrong"
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                ),
            )
            .await;
        assert!(
            format!("{impersonation:?}").contains("manager_token"),
            "a manager run id alone must not authorize memory: {impersonation:?}"
        );

        let write = client
            .call_tool(
                CallToolRequestParams::new("write_memory").with_arguments(
                    json!({
                        "manager_run_id": manager_run_id.to_string(),
                        "manager_token": manager_token,
                        "body": "prefer MSVC"
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                ),
            )
            .await
            .unwrap();
        assert_ne!(
            write.is_error,
            Some(true),
            "a plain cell-local write should not be refused: {write:?}"
        );

        let read = client
            .call_tool(
                CallToolRequestParams::new("read_memory").with_arguments(
                    json!({
                        "manager_run_id": manager_run_id.to_string(),
                        "manager_token": manager_token
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                ),
            )
            .await
            .unwrap();
        let read_text = format!("{read:?}");
        assert!(
            read_text.contains("prefer MSVC"),
            "the claim just written should read back: {read_text}"
        );

        // `02 record scope`'s "Carried from 11": a manager-scoped read marks each returned
        // claim consulted by the authenticated manager run - the `consulted`
        // edge `11 analytics questions`'s lessons-against-outcome query joins on.
        client
            .call_tool(
                CallToolRequestParams::new("read_memory").with_arguments(
                    json!({
                        "manager_run_id": manager_run_id.to_string(),
                        "manager_token": manager_token
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                ),
            )
            .await
            .unwrap();
        let (_, lessons) = h.get("/v1/analytics/lessons").await;
        assert!(
            lessons
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["body"] == "prefer MSVC" && row["consulted_by"].as_i64() == Some(1)),
            "the claim consulted through MCP should show up against this run: {lessons}"
        );

        // `25 memory lifecycle`: the global tier is gated on the operator, and this face does
        // not offer that promotion. `write_memory` returns `Err(McpError)`
        // for this, which the client surfaces as a JSON-RPC error from the
        // call itself rather than a successful `CallToolResult` with
        // `is_error: true`.
        let global_attempt = client
            .call_tool(
                CallToolRequestParams::new("write_memory").with_arguments(
                    json!({
                        "manager_run_id": manager_run_id.to_string(),
                        "manager_token": manager_token,
                        "body": "should not land",
                        "tier": "global"
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                ),
            )
            .await;
        assert!(
            global_attempt.is_err(),
            "writing the global tier through MCP must be refused: {global_attempt:?}"
        );

        client.cancel().await.unwrap();
    }

    /// `22 cell addressing`: an ungranted worker stays ungranted even when named.
    /// The default fixture has no roster, so refusal must happen before workspace or process creation.
    #[tokio::test]
    async fn delegating_to_a_worker_not_in_the_roster_is_refused() {
        use rmcp::ServiceExt;
        use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};
        use rmcp::transport::StreamableHttpClientTransport;
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

        let h = harness();
        let (manager_run_id, _, manager_token) = register_manager(&h);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = h.router.clone();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let transport = StreamableHttpClientTransport::<reqwest::Client>::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!(
                "http://127.0.0.1:{port}/v1/mcp"
            ))
            .auth_header(manager_token.clone()),
        );
        let client = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("farseer-api test client", "0.0.1"),
        )
        .serve(transport)
        .await
        .unwrap();

        // `31 manager delegation reach`, after a model mistyped its own token:
        // **the bearer is the credential, and a wrong argument beside it is
        // ignored rather than fatal.** This client presents the manager's real
        // bearer, so the call gets as far as the roster - which is the refusal
        // this test is actually about. A caller with no bearer is refused by the
        // guard before any tool body runs, which
        // `a_rebound_host_is_refused_before_the_token_is_even_checked` and
        // `an_operator_token_is_refused_on_a_manager_route` already cover.
        let mistyped = client
            .call_tool(
                CallToolRequestParams::new("delegate_to_worker").with_arguments(
                    json!({ "manager_run_id": manager_run_id.to_string(), "manager_token": "wrong", "worker": "nope", "goal": "anything" })
                        .as_object()
                        .cloned()
                        .unwrap(),
                ),
            )
            .await;
        assert!(
            format!("{mistyped:?}").contains("pinned roster"),
            "the transport proved this manager; a mistyped argument must not undo that: {mistyped:?}"
        );

        let attempt = client
            .call_tool(
                CallToolRequestParams::new("delegate_to_worker").with_arguments(
                    json!({ "manager_run_id": manager_run_id.to_string(), "manager_token": manager_token, "worker": "nope", "goal": "anything" })
                        .as_object()
                        .cloned()
                        .unwrap(),
                ),
            )
            .await;
        assert!(
            attempt.is_err(),
            "a worker absent from the roster must be refused before anything spawns: {attempt:?}"
        );

        client.cancel().await.unwrap();
    }

    /// Proves direct delegation end to end with a real worktree, a real Goose invocation, and the resulting run row.
    /// `10 runner inventory` records why Goose is the cheapest real worker on this machine: subscription reuse and no trust gate.
    /// Ignored because it still starts a real external process.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "spawns a real `goose` process - see the note on \
                `steering_a_claude_code_run_reaches_its_live_stdin`"]
    async fn delegating_to_a_roster_worker_runs_it_and_reports_back() {
        use rmcp::ServiceExt;
        use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};
        use rmcp::transport::StreamableHttpClientTransport;
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

        let h = harness_with_cell(CELL_WITH_A_WORKER);
        let (manager_run_id, task_id, manager_token) = register_manager(&h);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = h.router.clone();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let transport = StreamableHttpClientTransport::<reqwest::Client>::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!(
                "http://127.0.0.1:{port}/v1/mcp"
            ))
            .auth_header(h.token.as_str().to_string()),
        );
        let client = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("farseer-api test client", "0.0.1"),
        )
        .serve(transport)
        .await
        .unwrap();

        let result = client
            .call_tool(
                CallToolRequestParams::new("delegate_to_worker").with_arguments(
                    json!({ "manager_run_id": manager_run_id.to_string(), "manager_token": manager_token, "worker": "coder", "goal": "reply with a short confirmation" })
                        .as_object()
                        .cloned()
                        .unwrap(),
                ),
            )
            .await
            .unwrap();
        let text = format!("{result:?}");
        assert!(
            text.contains("\\\"outcome\\\":\\\"ok\\\"") || text.contains("\"outcome\":\"ok\""),
            "a successful delegated worker should report outcome ok: {text}"
        );
        assert!(
            text.contains("\\\"result\\\":\\\"") || text.contains("\"result\":\""),
            "the worker's non-null terminal text must be relayed: {text}"
        );
        assert!(
            text.contains(&task_id.to_string()),
            "the delegated run must stay on the manager's task: {text}"
        );

        client.cancel().await.unwrap();
    }

    /// The complete manager loop: HTTP instruction -> Claude manager -> farseer
    /// MCP -> Goose roster worker -> tool result -> the same Claude turn.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "spawns real `claude` and `goose` processes - slow and consumes \
                the configured subscriptions; run explicitly to verify the manager loop"]
    async fn instructing_a_manager_reaches_a_roster_worker_through_farseers_mcp_face() {
        let h = harness_with_cell(CELL_WITH_A_WORKER);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        h.state.set_mcp_endpoint(port);
        let router = h.router.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let (status, body) = h
            .post(
                "/v1/cells/zero/instruct",
                json!({
                    "goal": "Call the farseer delegate_to_worker tool exactly once. Ask the coder worker to reply with exactly worker-ok. Then return exactly the worker's result."
                }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let manager_run_id = body["run_id"].as_str().unwrap().to_string();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let delegated_run_id = loop {
            let events = h
                .state
                .store()
                .scan(0, 5_000, &ScanFilter::default())
                .unwrap();
            let delegated = events.iter().find(|event| {
                event.actor == Actor::Manager
                    && event.kind.as_str() == farseer_core::EventKind::RUN_QUEUED
            });
            let completed = delegated.and_then(|event| {
                h.state
                    .store()
                    .run(event.run_id)
                    .unwrap()
                    .filter(|row| row.outcome.as_deref() == Some("ok"))
                    .map(|_| event)
            });
            let relayed = completed.is_some_and(|delegated| {
                events.iter().any(|event| {
                    event.seq > delegated.seq
                        && event.actor == Actor::Manager
                        && event.run_id.to_string() == manager_run_id
                        && event.kind.as_str() == farseer_core::EventKind::TOOL_RESULT
                })
            });
            if let Some(delegated) = completed.filter(|_| relayed) {
                break delegated.run_id;
            }
            if std::time::Instant::now() >= deadline {
                let summary = events
                    .iter()
                    .map(|event| {
                        format!("{}:{}:{}", event.actor.as_str(), event.run_id, event.kind)
                    })
                    .collect::<Vec<_>>();
                let row = h.cancel_and_wait_for_finished(&manager_run_id).await;
                server.abort();
                panic!("manager loop timed out; row={row}; events={summary:?}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };

        let manager_row = h.cancel_and_wait_for_finished(&manager_run_id).await;
        server.abort();
        assert_eq!(manager_row["outcome"], "cancelled", "{manager_row}");
        assert!(
            !security::manager_config_path(&manager_run_id).exists(),
            "the bearer-bearing manager config must be removed independently"
        );
        let worker_row = h.state.store().run(delegated_run_id).unwrap().unwrap();
        assert_eq!(worker_row.runner, "goose");
        assert_eq!(worker_row.outcome.as_deref(), Some("ok"));
        assert_eq!(
            worker_row.task_id.to_string(),
            manager_row["task_id"],
            "the nested worker must stay on the manager's task"
        );
    }
    /// `22 cell addressing` and `06 cell transport` through the real MCP face:
    /// every refusal a cross-cell call owes the caller, checked over a real
    /// `rmcp` client rather than a hand-written JSON-RPC frame. None of these
    /// reach a process, which is why this is not one of the ignored tests.
    #[tokio::test]
    async fn a_cell_call_is_refused_unless_the_roster_granted_it_and_the_callee_exists() {
        use rmcp::ServiceExt;
        use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};
        use rmcp::transport::StreamableHttpClientTransport;
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

        let h = harness_with_cells(&[("zero", CELL_THAT_MAY_CALL), ("social", CALLEE_CELL)]);
        let (manager_run_id, _task_id, manager_token) = register_manager(&h);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = h.router.clone();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let transport = StreamableHttpClientTransport::<reqwest::Client>::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!(
                "http://127.0.0.1:{port}/v1/mcp"
            ))
            .auth_header(h.token.as_str().to_string()),
        );
        let client = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("farseer-api test client", "0.0.1"),
        )
        .serve(transport)
        .await
        .unwrap();

        let tools = client.list_tools(Default::default()).await.unwrap();
        assert!(
            tools
                .tools
                .iter()
                .any(|t| t.name.as_ref() == "delegate_to_cell"),
            "the cross-cell path must be visible to a manager that has it"
        );
        // Observed live on 2026-08-25: a tool on the face but absent from
        // `--allowedTools` makes the manager stall on a permission prompt no
        // operator is watching. Visible and callable are two different things.
        for tool in &tools.tools {
            let granted = format!("mcp__farseer__{}", tool.name);
            assert!(
                farseer_runner::invocation::MANAGER_ALLOWED_TOOLS.contains(&granted.as_str()),
                "`{granted}` is on the MCP face but not in --allowedTools, so a manager                  calling it hangs waiting for a permission answer"
            );
        }

        let call = async |arguments: serde_json::Value| {
            format!(
                "{:?}",
                client
                    .call_tool(
                        CallToolRequestParams::new("delegate_to_cell")
                            .with_arguments(arguments.as_object().cloned().unwrap()),
                    )
                    .await
            )
        };
        let base = |cell: &str| {
            json!({
                "manager_run_id": manager_run_id.to_string(),
                "manager_token": manager_token,
                "cell": cell,
                "goal": "post the changelog",
            })
        };

        // `22 cell addressing` section 3: naming it is not granting it.
        let ungranted = call(base("finance")).await;
        assert!(
            ungranted.contains("not a callable cell"),
            "an ungranted cell must stay ungranted however it is named: {ungranted}"
        );

        // `06 cell transport` section 2: the A2A endpoint is off by default.
        let peer = call(base("abroad")).await;
        assert!(
            peer.contains("A2A endpoint is off"),
            "a foreign peer must be refused while the endpoint is off: {peer}"
        );

        // A roster entry is a grant, not a definition.
        let missing = call(base("ghost")).await;
        assert!(
            missing.contains("no definition declares"),
            "a granted cell with no definition must say so: {missing}"
        );

        let empty_goal = call(json!({
            "manager_run_id": manager_run_id.to_string(),
            "manager_token": manager_token,
            "cell": "social",
            "goal": "   ",
        }))
        .await;
        assert!(
            empty_goal.contains("goal must not be empty"),
            "{empty_goal}"
        );

        let impersonation = call(json!({
            "manager_run_id": manager_run_id.to_string(),
            "manager_token": "wrong",
            "cell": "social",
            "goal": "post the changelog",
        }))
        .await;
        assert!(
            impersonation.contains("manager_token"),
            "a manager run id alone must not authorize a cell call: {impersonation}"
        );
    }
    /// The cross-cell loop, live: HTTP instruction -> Claude manager in cell
    /// zero -> farseer MCP `delegate_to_cell` -> a Goose manager running in
    /// cell social, under the caller's task.
    ///
    /// This is the round trip `06 cell transport` describes and the one thing
    /// the offline tests cannot prove, because every refusal they cover returns
    /// before a process exists.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "spawns real `claude` and `goose` processes - slow and consumes \
                the configured subscriptions; run explicitly to verify the cross-cell loop"]
    async fn a_manager_reaches_another_cell_through_farseers_mcp_face() {
        let h = harness_with_cells(&[("zero", CELL_THAT_MAY_CALL), ("social", LIVE_CALLEE_CELL)]);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        h.state.set_mcp_endpoint(port);
        let router = h.router.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let (status, body) = h
            .post(
                "/v1/cells/zero/instruct",
                json!({
                    "goal": "Call the farseer delegate_to_cell tool exactly once, with cell social and the goal: reply with exactly cell-ok. It is fire-and-forget, so report the call_id it returns and then stop."
                }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let manager_run_id = body["run_id"].as_str().unwrap().to_string();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        let (callee_run_id, call_event_seq) = loop {
            let events = h
                .state
                .store()
                .scan(0, 5_000, &ScanFilter::default())
                .unwrap();
            // The caller's own record entry, per `06 cell transport` section 6,
            // carrying the link `02 record scope` left open.
            let called = events.iter().find(|event| {
                event.run_id.to_string() == manager_run_id
                    && event.kind.as_str() == farseer_core::EventKind::CELL_CALLED
            });
            let finished = called.and_then(|event| {
                let callee: RunId = event.payload["callee_run_id"].as_str()?.parse().ok()?;
                h.state
                    .store()
                    .run(callee)
                    .ok()
                    .flatten()
                    .filter(|row| row.outcome.is_some())
                    .map(|_| (callee, event.seq))
            });
            if let Some(found) = finished {
                break found;
            }
            if std::time::Instant::now() >= deadline {
                let summary = events
                    .iter()
                    .map(|event| {
                        let mut payload = event.payload.to_string();
                        payload.truncate(400);
                        format!(
                            "{}:{}:{}={payload}",
                            event.actor.as_str(),
                            event.run_id,
                            event.kind
                        )
                    })
                    .collect::<Vec<_>>();
                let row = h.cancel_and_wait_for_finished(&manager_run_id).await;
                server.abort();
                panic!("cross-cell loop timed out; row={row}; events={summary:#?}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };

        let manager_row = h.cancel_and_wait_for_finished(&manager_run_id).await;
        server.abort();
        assert!(call_event_seq > 0);

        let callee_row = h.state.store().run(callee_run_id).unwrap().unwrap();
        assert_eq!(
            callee_row.cell_id.as_str(),
            "social",
            "the callee runs in its own cell, with its own manager"
        );
        assert_eq!(
            callee_row.runner, "goose",
            "`06 cell transport` section 4: the callee names its own runner, never the caller"
        );
        assert_eq!(
            callee_row.task_id.to_string(),
            manager_row["task_id"],
            "`22 cell addressing` section 2: one task, one owner"
        );
        assert_eq!(callee_row.outcome.as_deref(), Some("ok"), "{callee_row:?}");
    }
    /// The accepted half of a cell call, with no live runner involved.
    ///
    /// `06 cell transport` section 5 made a cell call fire-and-forget, so what
    /// the caller gets back is acceptance - a `call_id` and the callee's
    /// `run_id` - and the caller's own record entry naming that run. Whether the
    /// callee then succeeds is the callee's business and its own record.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_accepted_cell_call_records_the_callee_run_on_the_callers_own_entry() {
        use rmcp::ServiceExt;
        use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};
        use rmcp::transport::StreamableHttpClientTransport;
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

        let h = harness_with_cells(&[
            ("zero", CELL_THAT_MAY_CALL),
            ("social", UNRUNNABLE_CALLEE_CELL),
        ]);
        let (manager_run_id, task_id, manager_token) = register_manager(&h);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = h.router.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let transport = StreamableHttpClientTransport::<reqwest::Client>::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!(
                "http://127.0.0.1:{port}/v1/mcp"
            ))
            .auth_header(h.token.as_str().to_string()),
        );
        let client = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("farseer-api test client", "0.0.1"),
        )
        .serve(transport)
        .await
        .unwrap();

        let result = client
            .call_tool(
                CallToolRequestParams::new("delegate_to_cell").with_arguments(
                    json!({
                        "manager_run_id": manager_run_id.to_string(),
                        "manager_token": manager_token,
                        "cell": "social",
                        "goal": "post the changelog",
                        "autonomy_ceiling": "irreversible",
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                ),
            )
            .await;
        let text = format!("{result:?}");
        assert!(
            text.contains("call_id"),
            "the call must be accepted: {text}"
        );

        let called = h
            .state
            .store()
            .scan(0, 1_000, &ScanFilter::default())
            .unwrap()
            .into_iter()
            .find(|event| event.kind.as_str() == farseer_core::EventKind::CELL_CALLED)
            .expect("the caller keeps its own entry for the call");
        assert_eq!(
            called.run_id, manager_run_id,
            "`06 cell transport` section 6: the entry belongs to the caller"
        );
        assert_eq!(called.payload["call"]["to_cell"], "social");
        assert_eq!(
            called.payload["call"]["autonomy_ceiling"], "reversible",
            "irreversible was offered, the roster entry caps `social` at undoable, and the              callee's own policy caps at reversible - the floor neither side can lift"
        );
        let callee_run: RunId = called.payload["callee_run_id"]
            .as_str()
            .expect("the link `02 record scope` left open")
            .parse()
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let callee_row = loop {
            if let Some(row) = h.state.store().run(callee_run).unwrap() {
                break row;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the callee's run never reached the record"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        };
        server.abort();
        assert_eq!(callee_row.cell_id.as_str(), "social");
        assert_eq!(
            callee_row.task_id, task_id,
            "`22 cell addressing` section 2: one task, one owner"
        );
    }
    /// A refresh button on a poll nobody enabled must say so, not do nothing.
    ///
    /// `13 harness build kit` made the usage poll a menu item an operator picks
    /// (`usage_poll = true`), so the button is offered on every machine and
    /// works on the ones where it was asked for. The failure this pins is the
    /// one this repository keeps finding: **silence around a missing
    /// capability** - a control that appears to work, changes nothing, and
    /// leaves the operator with no way to learn why.
    #[tokio::test]
    async fn forcing_a_quota_read_refuses_by_name_when_no_source_is_configured() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zero.toml"), CELL).unwrap();
        let runs_dir = tempfile::tempdir().unwrap();
        let repo = git_repo_with_a_commit();
        let token = RuntimeToken::generate();
        let state = Arc::new(
            AppState::new(
                Store::open_in_memory().unwrap(),
                dir.path(),
                token.clone(),
                runs_dir.path(),
                repo.path(),
            )
            // No `usage_poll` anywhere, which is the default and the common case.
            .with_runner_config(
                RunnerConfig::load(
                    "[omp]
account = \"chatgpt\"
",
                )
                .unwrap(),
            ),
        );
        state.reload();
        let h = Harness {
            router: router(state.clone()),
            token,
            state,
            _dir: dir,
            _runs_dir: runs_dir,
            _repo: repo,
        };

        let (status, body) = h.post("/v1/quota/refresh", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let said = body["error"].as_str().unwrap_or_default();
        assert!(
            said.contains("[usage]") && said.contains("runners.toml"),
            "the refusal must name the line to add, not just refuse: {said}"
        );
    }

    /// `27 quota accounting`'s utilisation surface, end to end through the API.
    ///
    /// The assertion that matters most is the negative one: no percentage, ever.
    /// Farseer's own spend is a lower bound on a window drained by sessions it
    /// cannot see, so a percentage would be wrong in a way the operator could
    /// not detect, and most wrong exactly near exhaustion.
    #[tokio::test]
    async fn the_quota_surface_reports_windows_by_account_and_never_a_percentage() {
        use farseer_core::{Availability, WindowObservation};

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zero.toml"), CELL).unwrap();
        let runs_dir = tempfile::tempdir().unwrap();
        let repo = git_repo_with_a_commit();
        let token = RuntimeToken::generate();
        // `27 quota accounting` section 3: two runners on one login share one
        // window, and only the operator can say so.
        let config = RunnerConfig::load(
            r#"
[claude-code]
account = "anthropic-max"

[claude-acp]
account = "anthropic-max"
"#,
        )
        .unwrap();
        let state = Arc::new(
            AppState::new(
                Store::open_in_memory().unwrap(),
                dir.path(),
                token.clone(),
                runs_dir.path(),
                repo.path(),
            )
            .with_runner_config(config),
        );
        state.reload();
        let h = Harness {
            router: router(state.clone()),
            token,
            state,
            _dir: dir,
            _runs_dir: runs_dir,
            _repo: repo,
        };

        let observation = |availability| WindowObservation {
            account: "anthropic-max".into(),
            runner: "claude-code".into(),
            availability,
            rate_limit_type: "five_hour".into(),
            is_using_overage: false,
            used_percent: None,
            window_duration_mins: None,
            provider: None,
            label: None,
        };
        let observe = |observation: &WindowObservation, ts| {
            h.state
                .store()
                .observe_window(&CellId::new("zero"), RunId::new(), observation, ts)
                .unwrap()
        };

        assert!(observe(
            &observation(Availability::Allowed {
                resets_at: Some(1_787_003_600)
            }),
            1_000
        ));
        assert!(
            !observe(
                &observation(Availability::Allowed {
                    resets_at: Some(1_787_003_600)
                }),
                2_000
            ),
            "`10 runner inventory` measured this arriving on every run; only \
             transitions are history"
        );
        // Farseer's own spend inside the window, across both runners on the
        // account.
        for (runner, usd, tokens) in [
            ("claude-code", 250_000u64, 900u64),
            ("claude-acp", 50_000, 100),
        ] {
            h.state
                .store()
                .upsert_run(&RunRow {
                    run_id: RunId::new(),
                    task_id: TaskId::new(),
                    cell_id: CellId::new("zero"),
                    runner: runner.into(),
                    model: String::new(),
                    outcome: Some("ok".into()),
                    usd_micros: usd,
                    tokens,
                    operator_touched: false,
                    started_ts: 1_500,
                    finished_ts: Some(1_600),
                })
                .unwrap();
        }

        let (status, body) = h.get("/v1/quota").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let window = &body["windows"][0];
        assert_eq!(window["account"], "anthropic-max");
        assert_eq!(window["status"], "allowed");
        assert_eq!(
            window["runners"],
            json!(["claude-acp", "claude-code"]),
            "accounting keys by account, display keys by runner"
        );
        assert_eq!(window["farseer_usd_micros"], 300_000);
        assert_eq!(window["farseer_tokens"], 1_000);
        assert_eq!(
            window["resets_at"], 1_787_003_600i64,
            "`10 runner inventory` measured `resetsAt` on every report, so an              allowed window still has a countdown - dropping it left the widget              blank in the only case an operator sees on a good day"
        );

        let exhausted = observation(Availability::ExhaustedUntil {
            resets_at: 1_787_000_000,
        });
        assert!(observe(&exhausted, 3_000), "a status flip is a transition");
        let (_, body) = h.get("/v1/quota").await;
        assert_eq!(body["windows"][0]["status"], "exhausted_until");
        assert_eq!(body["windows"][0]["resets_at"], 1_787_000_000i64);

        // Scoped to the **recorded** window, not the whole payload, and the
        // distinction is `27 quota accounting`'s own: what farseer refuses is a
        // percentage **it computed**, because its spend is a lower bound on the
        // window and would be most wrong exactly at exhaustion. A number the
        // provider states is an observation and is admitted - `30 codex app
        // server` settled that, and `33 google quota`'s reversal now brings
        // omp's through the same door with `source: "omp usage"` on it.
        let wire = body["windows"][0].to_string();
        for absent in ["percent", "used_", "remaining", "quota_left"] {
            assert!(
                !wire.contains(absent),
                "`{absent}` would present a lower bound as a measurement: {wire}"
            );
        }
    }
    /// `28 operator surface`'s verb table is defined over a run line, and a
    /// surface that can only read one run by id cannot show which runs exist.
    #[tokio::test]
    async fn the_run_list_is_newest_first_and_carries_all_three_axes() {
        let h = harness();
        let cell = CellId::new("zero");
        let mut ids = Vec::new();
        for (started, outcome) in [(300, Some("ok")), (100, None), (200, Some("cancelled"))] {
            let run_id = RunId::new();
            ids.push((started, run_id));
            h.state
                .store()
                .upsert_run(&RunRow {
                    run_id,
                    task_id: TaskId::new(),
                    cell_id: cell.clone(),
                    runner: "claude-code".into(),
                    model: String::new(),
                    outcome: outcome.map(str::to_string),
                    usd_micros: 1_000,
                    tokens: 10,
                    operator_touched: false,
                    started_ts: started,
                    finished_ts: outcome.map(|_| started + 1),
                })
                .unwrap();
        }

        let (status, body) = h.get("/v1/runs").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let rows = body.as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["started_ts"], 300, "newest first");
        assert_eq!(rows[2]["started_ts"], 100);
        assert_eq!(rows[1]["lifecycle"], "finished");
        assert_eq!(rows[2]["lifecycle"], "running");
        assert!(
            rows[2]["liveness"].is_null(),
            "`05 run state model`: liveness is derived from a live handle, and \
             a row with no process has nothing to ask"
        );

        let (_, one) = h.get(&format!("/v1/runs/{}", ids[0].1)).await;
        assert_eq!(one["run_id"], rows[0]["run_id"], "one shape, two routes");

        let (_, capped) = h.get("/v1/runs?limit=1").await;
        assert_eq!(capped.as_array().unwrap().len(), 1);
    }

    fn cell_with(runner: &str) -> CellDefinition {
        farseer_core::CellDefinition::load(&format!(
            r#"
cell_id = "zero"
name = "Zero"
description = "d"
version = "1"
workspace_strategy = "worktree"

[manager]
runner = "{runner}"

[[roster]]
kind = "worker"
name = "coder"
runner = "{runner}"
"#
        ))
        .expect("a valid cell")
        .0
    }

    fn prompt_for(runner: &str) -> String {
        let h = harness();
        let spec = WorkerContractSpec {
            run_id: RunId::new(),
            task_id: farseer_core::TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "g".into(),
            workspace: farseer_core::WorkspaceStrategy::Worktree,
            runner: runner.into(),
            tool_grants: vec![],
            tool_level: Default::default(),
            autonomy_ceiling: farseer_core::Irreversibility::Reversible,
            budget: farseer_core::Budget::default(),
            definition_of_done: String::new(),
        };
        let contract = WorkerContract::seal(spec);
        let options = manager_run_options(
            &h.state,
            &contract,
            &cell_with(runner),
            &RuntimeToken::generate(),
        )
        .expect("options build");
        options
            .append_system_prompt
            .expect("every manager is told who it is")
    }

    /// `31 manager delegation reach`: this used to return early for every
    /// runner but Claude Code, so a pi manager never learned it had a roster.
    #[test]
    fn a_manager_on_any_runner_is_told_which_cell_it_manages() {
        for runner in ["pi", "omp", "codex-app-server", "goose-acp"] {
            let prompt = prompt_for(runner);
            assert!(prompt.contains("cell `zero`"), "{runner}: {prompt}");
            assert!(prompt.contains("coder"), "{runner}: {prompt}");
        }
    }

    /// `31 manager delegation reach`'s fix, in the two things it has to get
    /// right: the manager is pointed at a real tool, and it is **not** told the
    /// token. A credential in a prompt is one the model can quote, spend or
    /// leak into the record; the extension holds its own, so this prompt does
    /// not carry one at all.
    #[test]
    fn a_pi_manager_is_pointed_at_the_tool_and_never_told_the_token() {
        let h = harness();
        h.state.set_mcp_endpoint(8787);
        // The extension is checked on disk rather than assumed, so the test
        // has to put it there - which is the point: a missing file downgrades
        // the run to an honest no-reach instead of promising a tool that will
        // not load.
        let extension = h.state.repo_root().join("extensions").join("pi");
        std::fs::create_dir_all(&extension).unwrap();
        std::fs::write(
            extension.join("farseer-delegate.ts"),
            "export default () => {};",
        )
        .unwrap();
        let cell = cell_with("pi");
        let contract = WorkerContract::seal(WorkerContractSpec {
            run_id: RunId::new(),
            task_id: farseer_core::TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: "g".into(),
            workspace: farseer_core::WorkspaceStrategy::Worktree,
            runner: "pi".into(),
            tool_grants: vec![],
            tool_level: Default::default(),
            autonomy_ceiling: farseer_core::Irreversibility::Reversible,
            budget: farseer_core::Budget::default(),
            definition_of_done: String::new(),
        });
        let token = RuntimeToken::generate();
        let options =
            manager_run_options(&h.state, &contract, &cell, &token).expect("options build");

        let prompt = options
            .append_system_prompt
            .expect("a manager is told who it is");
        assert!(prompt.contains("delegate_to_worker"), "{prompt}");
        assert!(!prompt.contains("CANNOT reach"), "{prompt}");
        assert!(!prompt.contains(token.as_str()), "{prompt}");

        // The reach itself: farseer's own extension on the argv, and the
        // credentials in the environment, which is the one place neither a
        // process listing nor the model can read them.
        assert_eq!(options.extensions.len(), 1, "{:?}", options.extensions);
        let env: std::collections::HashMap<_, _> = options.runner_env.into_iter().collect();
        assert_eq!(
            env.get("FARSEER_MANAGER_TOKEN").map(String::as_str),
            Some(token.as_str())
        );
        assert_eq!(
            env.get("FARSEER_ENDPOINT").map(String::as_str),
            Some("http://127.0.0.1:8787")
        );
    }

    /// `32 harness capability floor`'s third consequence, answered with the menu
    /// a cell can actually order from rather than the one the harness discovered.
    #[tokio::test]
    async fn the_skills_menu_lists_the_repository_and_says_who_declares_each() {
        let h = harness();
        let dir = h.state.repo_root().join("skills");
        std::fs::create_dir_all(dir.join("farseer-record")).unwrap();
        std::fs::create_dir_all(dir.join("unused-one")).unwrap();
        // A loose file beside them is not a skill, and must not read as one.
        std::fs::write(dir.join("README.md"), b"not a skill").unwrap();

        let (status, body) = h.get("/v1/skills").await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = body["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["farseer-record", "unused-one"], "{body}");

        // The question an author actually asks: is anything using this?
        assert!(
            body["skills"][1]["declared_by"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{body}"
        );
    }

    /// `26 routing policy` in three states. The author's order is honoured, an
    /// account with a spent window takes every runner on it out of the running,
    /// and a runner farseer has never seen a window for stays eligible.
    #[test]
    fn routing_skips_a_spent_window_and_never_a_merely_unseen_one() {
        let h = harness();
        let pinned = vec!["pi".to_string()];
        let pair = vec!["pi".to_string(), "omp".to_string()];

        // Nothing observed yet. `unknown` behaves as available until proven
        // otherwise, which `26` calls permanent rather than a gap to close.
        assert_eq!(
            first_available_runner(&h.state, &pair).as_deref(),
            Some("pi")
        );

        let spent = farseer_core::WindowObservation {
            account: "pi".to_string(),
            runner: "pi".to_string(),
            availability: farseer_core::Availability::ExhaustedUntil {
                resets_at: 1_787_000_000,
            },
            rate_limit_type: "five_hour".to_string(),
            is_using_overage: false,
            used_percent: None,
            window_duration_mins: None,
            provider: None,
            label: None,
        };
        assert!(
            h.state
                .store()
                .observe_window(&CellId::new("zero"), RunId::new(), &spent, 1_000)
                .expect("the observation lands"),
            "a first sighting is a transition"
        );

        // The author's second choice, because the first has nowhere to spend.
        assert_eq!(
            first_available_runner(&h.state, &pair).as_deref(),
            Some("omp")
        );
        // And a pin with nowhere to go is `None`, which the delegation path
        // turns into `runner_exhausted` - `26` section 3, not a fifth outcome.
        assert_eq!(first_available_runner(&h.state, &pinned), None);
    }

    /// `runner = "pi"` and `runners = ["pi", "omp"]` are the same field, because
    /// `26` found a one-item list is the normal case and making every cell write
    /// brackets would tax the common path for the rare one.
    #[test]
    fn a_worker_may_name_one_runner_or_several() {
        let one: farseer_core::RosterEntry = serde_json::from_value(serde_json::json!({
            "kind": "worker", "name": "coder", "runner": "pi"
        }))
        .expect("singular");
        let many: farseer_core::RosterEntry = serde_json::from_value(serde_json::json!({
            "kind": "worker", "name": "coder", "runners": ["pi", "omp"]
        }))
        .expect("plural");
        let names = |entry: &farseer_core::RosterEntry| match entry {
            farseer_core::RosterEntry::Worker { runners, .. } => runners.clone(),
            _ => panic!("a worker"),
        };
        assert_eq!(names(&one), ["pi"]);
        assert_eq!(names(&many), ["pi", "omp"]);
    }

    /// The credential is gone from every prompt, not just pi's.
    ///
    /// `31 manager delegation reach` moved identity onto the bearer after a
    /// `goose-acp` manager mistyped the 64-character token it had been told to
    /// copy. The failure looked like authorization and was transcription, and
    /// the only durable fix is not asking a model to carry a secret at all.
    #[test]
    fn no_manager_is_ever_told_its_own_token() {
        let h = harness();
        h.state.set_mcp_endpoint(8787);
        // The three MCP transports. pi has its own test above, and its
        // extension only exists on disk in that fixture.
        for runner in ["claude-code", "codex-app-server", "goose-acp"] {
            let cell = cell_with(runner);
            let contract = WorkerContract::seal(WorkerContractSpec {
                run_id: RunId::new(),
                task_id: farseer_core::TaskId::new(),
                cell_id: CellId::new("zero"),
                goal: "g".into(),
                workspace: farseer_core::WorkspaceStrategy::Worktree,
                runner: runner.into(),
                tool_grants: vec![],
                tool_level: Default::default(),
                autonomy_ceiling: farseer_core::Irreversibility::Reversible,
                budget: farseer_core::Budget::default(),
                definition_of_done: String::new(),
            });
            let token = RuntimeToken::generate();
            let options =
                manager_run_options(&h.state, &contract, &cell, &token).expect("options build");
            let prompt = options
                .append_system_prompt
                .expect("a manager is told who it is");
            assert!(
                !prompt.contains(token.as_str()),
                "{runner} was handed its own token: {prompt}"
            );
            assert!(
                prompt.contains("delegate_to_worker"),
                "{runner} should be able to delegate: {prompt}"
            );
        }
    }

    /// The sentence that exists because a manager fabricated a delegation.
    /// A roster it cannot reach must be named **and** disclaimed, or the model
    /// does the nearest available thing and calls it compliance.
    #[test]
    fn a_manager_that_cannot_delegate_is_told_so_and_told_not_to_claim_it_did() {
        // `goose-acp` rather than `pi`, which this test used to name: pi can
        // delegate now, and a test whose premise has quietly expired asserts
        // nothing. ACP is the last runner with no channel for the verbs.
        let prompt = prompt_for("goose-acp");
        assert!(prompt.contains("CANNOT reach"), "{prompt}");
        assert!(
            prompt.contains("Never state or imply that you delegated"),
            "{prompt}"
        );
        // And it is not handed credentials for a face it cannot call.
        assert!(!prompt.contains("manager_token"), "{prompt}");
    }
}
