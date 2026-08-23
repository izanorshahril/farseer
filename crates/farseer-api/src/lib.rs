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

use std::collections::BTreeMap;
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

use farseer_core::{CellDefinition, CellId, LivenessThresholds, RunId, Seq};
use farseer_store::{ScanFilter, Store, StoreError, UI_STATE_CAP_BYTES};

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

    pub fn new(store: Store, cells_dir: impl Into<PathBuf>, token: RuntimeToken) -> Self {
        Self {
            store: Mutex::new(store),
            cells: Mutex::new(BTreeMap::new()),
            cells_dir: cells_dir.into(),
            token,
            thresholds: LivenessThresholds::default(),
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
        .route("/v1/events", get(read_events))
        .route("/v1/stream", get(stream_events))
        .route("/v1/runs/{run_id}", get(get_run))
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
    }))
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
    }

    fn harness() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zero.toml"), CELL).unwrap();
        let token = RuntimeToken::generate();
        let state = Arc::new(AppState::new(
            Store::open_in_memory().unwrap(),
            dir.path(),
            token.clone(),
        ));
        state.reload();
        Harness {
            router: router(state.clone()),
            token,
            state,
            _dir: dir,
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
}
