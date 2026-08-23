//! Farseer's local API: **bespoke HTTP plus SSE on `127.0.0.1`.**
//!
//! `16` weighed making ACP the substrate, as berd does, and rejected it: roughly
//! a fifth of farseer's surface maps to an ACP verb, and **a protocol that
//! covers a fifth of the surface is not the transport**. ACP belongs on top of
//! this as a server adapter, exposing one cell's manager conversation and
//! nothing else.
//!
//! Two rules from `16` shape everything below:
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

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use serde::{Deserialize, Serialize};

use farseer_core::policy::Budget;
use farseer_core::run::{WorkerContract, WorkerContractSpec, WorkspaceStrategy};
use farseer_core::{CellDefinition, CellId, LivenessThresholds, NewEvent, RunId, Seq, TaskId};
use farseer_manager::{LivenessHandle, RunSink, SteerHandle};
use farseer_runner::spawn::CancelToken;
use farseer_store::{RunRow, ScanFilter, Store, StoreError, UI_STATE_CAP_BYTES};

pub mod security;

pub use security::{RuntimeToken, runtime_file_path, write_runtime_file};

/// How often the stream looks for new events.
///
/// Polling the record rather than fanning out from an in-process bus is what
/// makes "a slow client never slows a worker" structural instead of a rule
/// someone has to remember. `09` measured the read side at p99 478us while a
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
    /// *of*. `13` deliberately keeps no git flag on `CellDefinition`, so this
    /// has to come from somewhere else - the runtime's own working directory
    /// is the only unambiguous repo available without inventing a field. In
    /// practice this is the farseer checkout itself, which is exactly what
    /// cell zero - farseer's own builder harness - is for.
    repo_root: PathBuf,
    /// In-flight runs, keyed by run id. A run removes its own entry when it
    /// finishes, successfully or not - so a lookup miss here means either
    /// "already finished" or "farseer restarted since", and the run row is
    /// what still answers which.
    runs: Mutex<HashMap<RunId, RunHandle>>,
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
            runs_dir: runs_dir.into(),
            repo_root: repo_root.into(),
            runs: Mutex::new(HashMap::new()),
        }
    }

    /// Re-read every definition from disk.
    ///
    /// `16` gives the API read, validate and reload, and **no edit path**: an
    /// edit API would make farseer responsible for merge conflicts and skew
    /// against the operator's own editor, in exchange for nothing.
    /// Reloading makes the files on disk the truth: a cell whose own file is
    /// broken **disappears** until it parses again. `17` pins the definition
    /// version per run, so work already executing is unaffected.
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
        .route("/v1/events", get(read_events))
        .route("/v1/stream", get(stream_events))
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/cancel", post(cancel_run))
        .route("/v1/runs/{run_id}/steer", post(steer_run))
        .route("/v1/runs/{run_id}/rerun", post(rerun_run))
        .route("/v1/runs/{run_id}/rescope", post(rescope_run))
        .route("/v1/ui-state/{key}", get(get_ui_state).put(put_ui_state))
        .route("/v1/analytics/cost", get(analytics_cost))
        .route("/v1/analytics/intervention", get(analytics_intervention))
        .route("/v1/analytics/rework", get(analytics_rework))
        .route("/v1/analytics/lessons", get(analytics_lessons))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Bind loopback only, write the runtime file, and serve until cancelled.
pub async fn serve(state: Arc<AppState>, port: u16) -> std::io::Result<()> {
    // `16`: **bind `127.0.0.1` only.** Not `0.0.0.0`, not a hostname that might
    // resolve to something routable.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let bound = listener.local_addr()?.port();
    write_runtime_file(&runtime_file_path(), bound, &state.token)?;
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
    if !presented_token(headers).is_some_and(|t| state.token.matches(t)) {
        return ApiError::Unauthorized.into_response();
    }
    next.run(request).await
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
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            // `24`: over 1 MiB per key, the answer is `413`.
            Self::Store(StoreError::UiStateTooLarge { .. }) => StatusCode::PAYLOAD_TOO_LARGE,
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

/// **The command half of the API, no longer absent.** `16`: an instruction is
/// fire-and-forget - this returns `202` with a `run_id` the moment a process
/// is spawned, and the record is where the result shows up, via
/// `/v1/runs/{run_id}` or `/v1/stream`.
///
/// **What this deliberately is not**, per `22`: an instruction *delegates* to
/// exactly one owner, which `05`'s manager loop would decide by planning and
/// calling workers. There is no manager loop yet, so this runs the cell's own
/// **manager runner** directly against the goal - the only roster runner
/// value guaranteed to be `claude-code`, the one runner this binary can
/// execute. A worker roster entry naming `codex` or `cursor-agent` would fail
/// with `UnsupportedRunner` today; that gap is `farseer-manager`'s, not
/// hidden here.
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

    let contract = WorkerContract::seal(WorkerContractSpec {
        run_id: RunId::new(),
        task_id: TaskId::new(),
        cell_id: cell.cell_id.clone(),
        goal: body.goal,
        workspace: cell.workspace_strategy,
        runner: cell.manager.runner.clone(),
        tool_grants: cell.tool_grants(),
        autonomy_ceiling: cell.policy.autonomy_ceiling,
        // `23`'s three narrowing layers - definition, roster cap, caller's
        // remaining pool - are not wired to this entry point, so a run
        // started here is unbounded rather than silently capped at a guess.
        budget: Budget::default(),
        definition_of_done: String::new(),
    });
    let run_id = spawn_run(&state, contract)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(InstructResponse {
            run_id: run_id.to_string(),
        }),
    ))
}

/// Shared by `instruct`, `rerun` and `rescope`: create the workspace
/// synchronously (so a failure is a `500` before the caller ever gets a
/// `run_id` to poll), then hand `contract` to `run_worker` on a blocking
/// task and tear the workspace down once it returns.
fn spawn_run(state: &Arc<AppState>, contract: WorkerContract) -> ApiResult<RunId> {
    let run_id = contract.run_id;
    let name = run_id.to_string();

    // `04`: a `Worktree` cell gets a real `git worktree`, off `repo_root`; a
    // `PlainDirectory` cell gets exactly that. Both are created synchronously
    // here, before the `202` goes out, so a workspace failure is a `500`
    // rather than a run that silently never starts.
    let repo_for_teardown = match contract.workspace {
        WorkspaceStrategy::Worktree => Some(state.repo_root.clone()),
        WorkspaceStrategy::PlainDirectory => None,
    };
    let cwd = match contract.workspace {
        WorkspaceStrategy::Worktree => {
            farseer_runner::workspace::create_worktree(&state.repo_root, &state.runs_dir, &name)
                .map_err(|e| ApiError::Workspace(e.to_string()))?
        }
        WorkspaceStrategy::PlainDirectory => {
            let dir = state.runs_dir.join(&name);
            std::fs::create_dir_all(&dir).map_err(|e| ApiError::Workspace(e.to_string()))?;
            dir
        }
    };

    let thresholds = state.thresholds;
    let background_state = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let result = farseer_manager::run_worker(
            background_state.as_ref(),
            &contract,
            &cwd,
            thresholds,
            now_ms,
            |cancel, liveness, steer| {
                background_state.runs().insert(
                    run_id,
                    RunHandle {
                        cancel,
                        liveness,
                        steer,
                    },
                );
            },
        );
        background_state.runs().remove(&run_id);
        // The outcome is already the run row's problem: `run_worker` writes
        // `Failed` on any error before returning it, so there is nothing this
        // background task needs to do with `result` beyond letting it drop.
        drop(result);

        // `04`'s ordering constraint is already satisfied here: `run_worker`
        // has returned, which only happens once the process's stdout pipe
        // closed, which happens at or after process exit - the cwd handle
        // that blocks a delete is gone by construction, not by a race this
        // code has to win.
        if let Err(e) =
            farseer_runner::workspace::teardown_workspace(&cwd, repo_for_teardown.as_deref())
        {
            // `04`: a workspace that survives the backoff is the operator's
            // problem to see, not this task's to keep retrying forever. No
            // record surface for it yet - see the README's open gaps.
            eprintln!("workspace teardown for run {run_id} did not complete: {e}");
        }
    });

    Ok(run_id)
}

/// `05`'s simplest manager verb: end the process, no plan, no re-scope.
/// Idempotent, and `404` rather than a silent no-op when there is nothing to
/// cancel - a run that already finished, or one that never existed.
///
/// **Does not yet produce `05`'s `Cancelled` outcome** - see
/// `farseer_manager`'s own doc comment. The run row this leaves behind reads
/// `failed`.
async fn cancel_run(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<StatusCode> {
    let run_id: RunId = run_id.parse().map_err(|_| ApiError::NotFound("run"))?;
    let token = state.runs().get(&run_id).map(|h| h.cancel.clone());
    match token {
        Some(token) => {
            token.cancel();
            Ok(StatusCode::ACCEPTED)
        }
        None => Err(ApiError::NotFound("run")),
    }
}

#[derive(Debug, Deserialize)]
pub struct SteerBody {
    pub message: String,
}

/// `05`'s **steer**: a follow-up message into a run's live process, on the
/// envelope `claude_code::steer_envelope`'s 2026-08-23 probe verified.
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
    /// The field being changed. `05`: re-scope is a new run because a
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

/// Reconstructs the `WorkerContractSpec` a past run was sealed with, from the
/// `run_queued` event `farseer_manager::run_worker` writes before spawning
/// anything. `05`: immutability is what makes this answer one answer - the
/// run row itself never carried the goal or the grants, so this event is the
/// only place a re-run or re-scope can read them back from.
fn original_contract(state: &AppState, run_id: RunId) -> ApiResult<WorkerContractSpec> {
    let events = {
        let store = state.store();
        store.scan(0, 5_000, &ScanFilter::run(run_id))?
    };
    let queued = events
        .iter()
        .find(|e| e.kind.as_str() == farseer_core::EventKind::RUN_QUEUED)
        .ok_or(ApiError::NotFound("run"))?;
    serde_json::from_value(queued.payload.clone())
        .map_err(|_| ApiError::Corrupt("run_queued event"))
}

/// `05`'s **re-run**: same contract, fresh run, fresh workspace. `16`:
/// operator-initiated re-run leaves an event behind - here, the
/// `rescoped_from` edge `11`'s rework-depth query already walks, so a chain
/// of re-runs reads exactly like a chain of re-scopes to that analytics query.
async fn rerun_run(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<(StatusCode, Json<RespawnResponse>)> {
    let parent_run_id: RunId = run_id.parse().map_err(|_| ApiError::NotFound("run"))?;
    let mut spec = original_contract(&state, parent_run_id)?;
    spec.run_id = RunId::new();
    respawn(&state, spec, parent_run_id).await
}

/// `05`'s **re-scope**: a new run against the same task, with a changed
/// contract field. Only `goal` is reachable here today - `tool_grants`,
/// `autonomy_ceiling` and the rest come from the cell definition at
/// `instruct` time, not from anything an operator can override per run yet.
async fn rescope_run(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
    Json(body): Json<RescopeBody>,
) -> ApiResult<(StatusCode, Json<RespawnResponse>)> {
    let parent_run_id: RunId = run_id.parse().map_err(|_| ApiError::NotFound("run"))?;
    let mut spec = original_contract(&state, parent_run_id)?;
    let Some(goal) = body.goal else {
        return Err(ApiError::BadRequest(
            "rescope needs a changed field - pass goal, or use rerun to repeat the same contract",
        ));
    };
    if goal.trim().is_empty() {
        return Err(ApiError::BadRequest("goal must not be empty"));
    }
    if goal == spec.goal {
        return Err(ApiError::BadRequest(
            "goal is unchanged - use rerun, not rescope, to repeat the same contract",
        ));
    }
    spec.run_id = RunId::new();
    spec.goal = goal;
    respawn(&state, spec, parent_run_id).await
}

async fn respawn(
    state: &Arc<AppState>,
    spec: WorkerContractSpec,
    parent_run_id: RunId,
) -> ApiResult<(StatusCode, Json<RespawnResponse>)> {
    let run_id = spec.run_id;
    state.store().record_rescope(run_id, parent_run_id)?;
    let run_id = spawn_run(state, WorkerContract::seal(spec))?;
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
    /// Scope, applied server-side. `16` rejected a firehose the client filters.
    pub cell: Option<String>,
    pub run: Option<String>,
    /// The cursor. Omit for live only.
    pub since: Option<Seq>,
    pub limit: Option<usize>,
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
    let events = store.scan(
        query.since.unwrap_or(0),
        query.limit.unwrap_or(500).min(5_000),
        &query.filter()?,
    )?;
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

    // A bounded channel *is* `16`'s bounded per-connection buffer. When the
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
/// `16`: **liveness is derived, never stored**, so there is no write path for it
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
    /// `18`/`05`'s watchdog state - `"live"`, `"stalled"` or `"likely_hung"` -
    /// or `None` when there is nothing in memory to ask: the run already
    /// finished, or farseer restarted since it started. `17` chose no orphan
    /// survival over run survival, so a restart losing this is the same
    /// trade already made everywhere else, not a new gap.
    pub liveness: Option<String>,
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

    Ok(Json(RunView {
        run_id: row.run_id.to_string(),
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
    }))
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

/// `24`: farseer stores an opaque blob and **never parses it**. No validation,
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

    fn harness() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zero.toml"), CELL).unwrap();
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

        /// Cancels immediately, then polls until the run row reads
        /// `finished` - bounding how long a real `claude` invocation (if
        /// installed) gets to run before a test asserts against a terminal
        /// row.
        async fn cancel_and_wait_for_finished(&self, run_id: &str) -> serde_json::Value {
            self.post(&format!("/v1/runs/{run_id}/cancel"), json!({}))
                .await;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
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

        // There is no edit path, per `16` section 6.
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
    async fn steering_a_claude_code_run_reaches_its_live_stdin() {
        // `claude_code::steer_envelope`'s 2026-08-23 probe verified the wire
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

    /// This is the one test in the suite that actually spawns a process - the
    /// mechanics of spawning, reaping and mapping stream-json are already
    /// proven against `cmd.exe` fixtures in `farseer-runner` and
    /// `farseer-manager`'s own tests, so this only needs to prove the HTTP
    /// wiring around them: fire-and-forget returns a `run_id` before the run
    /// finishes, the record picks it up, and a workspace directory exists.
    /// Whether `claude` is actually installed on the machine running this
    /// test is deliberately not asserted either way - `10` confirms it is on
    /// the dev machine, but the row this test waits for reaches a terminal
    /// state either way: `ok`/`failed` if it ran, `failed` immediately via
    /// `ExecutableNotFound` if it did not.
    #[tokio::test]
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

        // `04`'s ordering constraint, proven end to end: the workspace this
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

        // `11`'s rework-depth query walks the same `rescoped_from` edge both
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
}
