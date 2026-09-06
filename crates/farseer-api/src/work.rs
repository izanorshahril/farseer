//! Operator work commands and projections.
//!
//! `40 work model and session explorer` puts the stable interface here: widgets
//! issue validated commands and read projections; they never append raw events.

use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use farseer_core::{
    Actor, Conversation, ConversationId, RunId, Task, TaskId, TaskState, TranscriptCustody,
};
use farseer_store::{
    GraphEdge, GraphFilter, GraphNode, SessionRow, SimilarityEdge, TaskCursor, TaskFilter,
    TranscriptAttachment, TranscriptProjection,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{ApiError, ApiResult, AppState, now_ms};

const TRANSCRIPT_CAP_BYTES: u64 = 16 * 1024 * 1024;
const TRANSCRIPT_CANDIDATE_CAP: usize = 200;
const TRANSCRIPT_EDGE_CAP: usize = 20;
const TRANSCRIPT_TEXT_CAP_BYTES: usize = 32 * 1024 * 1024;
const REDACTION_VERSION: &str = "farseer-scrub-v1";
const PROJECTION_VERSION: &str = "hash-tf-v1";
const EMBEDDING_MODEL: &str = "farseer-hash-tf-64";
const DIMENSIONS: usize = 64;
const TASK_PAGE_DEFAULT: usize = 100;
const TASK_PAGE_MAX: usize = 500;
const STRUCTURED_RESPONSE_MAX_BYTES: usize = 1024 * 1024;
const GRAPH_NODE_DEFAULT: usize = 100;
const GRAPH_NODE_MAX: usize = 500;
const GRAPH_EDGE_DEFAULT: usize = 300;
const GRAPH_EDGE_MAX: usize = 1_500;

#[derive(Debug, Deserialize)]
pub(super) struct ConversationQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateConversationBody {
    pub title: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub manager_runner: Option<String>,
}

pub(super) async fn list_conversations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ConversationQuery>,
) -> ApiResult<Json<Vec<Conversation>>> {
    Ok(Json(
        state
            .store()
            .conversations(query.limit.unwrap_or(100).min(500))?,
    ))
}

pub(super) async fn create_conversation(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateConversationBody>,
) -> ApiResult<(StatusCode, Json<Conversation>)> {
    if body.title.trim().is_empty() {
        return Err(ApiError::BadRequest("conversation title must not be empty"));
    }
    let cell = state
        .cells()
        .get(&farseer_core::CellId::new("zero"))
        .cloned()
        .ok_or(ApiError::NotFound("cell"))?;
    let runner = body
        .manager_runner
        .as_deref()
        .unwrap_or_else(|| cell.manager.runner());
    if !cell.manager.has_runner(runner) {
        return Err(ApiError::BadRequest(
            "manager runner is not a candidate for cell zero",
        ));
    }
    let project_path = body
        .project
        .as_deref()
        .map(|path| crate::projects::resolve(&state, path))
        .transpose()?
        .map(|path| crate::projects::display(&path));
    let now = now_ms();
    let conversation = Conversation {
        conversation_id: ConversationId::new(),
        title: body.title.trim().to_owned(),
        project_path,
        manager_runner: Some(runner.to_owned()),
        created_ts: now,
        updated_ts: now,
        archived_ts: None,
    };
    state.store().create_conversation(&conversation)?;
    Ok((StatusCode::CREATED, Json(conversation)))
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct TasksQuery {
    pub conversation_id: Option<String>,
    pub project: Option<String>,
    pub state: Option<String>,
    pub limit: Option<usize>,
}

pub(super) async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TasksQuery>,
) -> ApiResult<Json<Vec<Task>>> {
    let conversation_id = query
        .conversation_id
        .as_deref()
        .map(parse_conversation)
        .transpose()?;
    let task_state = query
        .state
        .as_deref()
        .map(|state| {
            state
                .parse::<TaskState>()
                .map_err(|_| ApiError::BadRequest("unknown task state"))
        })
        .transpose()?;
    let project = query
        .project
        .as_deref()
        .map(|path| crate::projects::resolve(&state, path))
        .transpose()?
        .map(|path| crate::projects::display(&path));
    Ok(Json(state.store().tasks(&TaskFilter {
        conversation_id,
        project_path: project.as_deref(),
        state: task_state,
        limit: query.limit.unwrap_or(500).min(1_000),
        after: None,
    })?))
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskPageQuery {
    pub conversation_id: Option<String>,
    pub project: Option<String>,
    pub state: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TaskPageScope {
    conversation_id: Option<ConversationId>,
    project_path: Option<String>,
    state: Option<TaskState>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TaskPageCursor {
    version: u8,
    scope: TaskPageScope,
    priority: i32,
    updated_ts: i64,
    task_id: TaskId,
}

#[derive(Debug, Serialize)]
pub(super) struct TaskRunSummary {
    pub run_count: usize,
    pub active_runs: usize,
    pub latest_outcome: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct TaskCard {
    #[serde(flatten)]
    pub task: Task,
    pub run_summary: TaskRunSummary,
}

#[derive(Debug, Serialize)]
pub(super) struct TaskPage {
    pub tasks: Vec<TaskCard>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    /// Task transitions can race a page read; callers should refresh when they
    /// need a newer projection rather than treating this as a snapshot.
    pub freshness: &'static str,
    pub generated_ts: i64,
}

pub(super) async fn list_task_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TaskPageQuery>,
) -> ApiResult<Json<TaskPage>> {
    let scope = task_page_scope(&state, &query)?;
    let after = query
        .cursor
        .as_deref()
        .map(decode_task_cursor)
        .transpose()?
        .map(|cursor| {
            if cursor.version != 1 || cursor.scope != scope {
                return Err(ApiError::BadRequest(
                    "task page cursor does not match this scope",
                ));
            }
            Ok(TaskCursor {
                priority: cursor.priority,
                updated_ts: cursor.updated_ts,
                task_id: cursor.task_id,
            })
        })
        .transpose()?;
    let limit = query
        .limit
        .unwrap_or(TASK_PAGE_DEFAULT)
        .clamp(1, TASK_PAGE_MAX);
    let project = scope.project_path.as_deref();
    let mut tasks = state.store().task_page(&TaskFilter {
        conversation_id: scope.conversation_id,
        project_path: project,
        state: scope.state,
        limit,
        after,
    })?;
    let has_more = tasks.len() > limit;
    if has_more {
        tasks.truncate(limit);
    }
    let tasks = tasks
        .into_iter()
        .map(|task| {
            let runs = state.store().runs_for_task(task.task_id)?;
            let summary = TaskRunSummary {
                run_count: runs.len(),
                active_runs: runs.iter().filter(|run| run.outcome.is_none()).count(),
                latest_outcome: runs.last().and_then(|run| run.outcome.clone()),
            };
            Ok(TaskCard {
                task,
                run_summary: summary,
            })
        })
        .collect::<farseer_store::Result<Vec<_>>>()?;
    let generated_ts = now_ms();
    let mut page = TaskPage {
        tasks,
        next_cursor: None,
        has_more,
        freshness: "eventual",
        generated_ts,
    };
    page.next_cursor = page.tasks.last().filter(|_| page.has_more).map(|card| {
        let task = &card.task;
        encode_task_cursor(&TaskPageCursor {
            version: 1,
            scope: scope.clone(),
            priority: task.priority,
            updated_ts: task.updated_ts,
            task_id: task.task_id,
        })
    });
    while serde_json::to_vec(&page)
        .expect("task page is serializable")
        .len()
        > STRUCTURED_RESPONSE_MAX_BYTES
    {
        page.tasks.pop().ok_or(ApiError::BadRequest(
            "a task is too large for the structured page limit",
        ))?;
        if page.tasks.is_empty() {
            return Err(ApiError::BadRequest(
                "a task is too large for the structured page limit",
            ));
        }
        page.has_more = true;
        page.next_cursor = page.tasks.last().map(|card| {
            let task = &card.task;
            encode_task_cursor(&TaskPageCursor {
                version: 1,
                scope: scope.clone(),
                priority: task.priority,
                updated_ts: task.updated_ts,
                task_id: task.task_id,
            })
        });
    }
    Ok(Json(page))
}

fn task_page_scope(state: &AppState, query: &TaskPageQuery) -> ApiResult<TaskPageScope> {
    let conversation_id = query
        .conversation_id
        .as_deref()
        .map(parse_conversation)
        .transpose()?;
    let state_filter = query
        .state
        .as_deref()
        .map(|state| {
            state
                .parse::<TaskState>()
                .map_err(|_| ApiError::BadRequest("unknown task state"))
        })
        .transpose()?;
    let project_path = query
        .project
        .as_deref()
        .map(|path| crate::projects::resolve(state, path))
        .transpose()?
        .map(|path| crate::projects::display(&path));
    Ok(TaskPageScope {
        conversation_id,
        project_path,
        state: state_filter,
    })
}

fn encode_task_cursor(cursor: &TaskPageCursor) -> String {
    let bytes = serde_json::to_vec(cursor).expect("task page cursor is serializable");
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}").expect("writing to a string cannot fail");
    }
    encoded
}

fn decode_task_cursor(value: &str) -> ApiResult<TaskPageCursor> {
    if value.is_empty() || value.len() > 4096 || !value.len().is_multiple_of(2) {
        return Err(ApiError::BadRequest("invalid task page cursor"));
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ApiError::BadRequest("invalid task page cursor"))?;
    serde_json::from_slice(&bytes).map_err(|_| ApiError::BadRequest("invalid task page cursor"))
}

#[derive(Debug, Serialize)]
pub(super) struct TaskDetail {
    pub task: Task,
    pub usage: TaskUsage,
    pub allowed_transitions: Vec<TaskState>,
    pub transitions: Vec<farseer_core::TaskTransition>,
    pub runs: Vec<crate::RunView>,
    pub sessions: Vec<farseer_core::HarnessSession>,
    pub attachments: Vec<TranscriptAttachmentView>,
    pub artifacts: Vec<farseer_store::ArtifactRow>,
}

#[derive(Debug, Serialize)]
pub(super) struct TaskUsage {
    pub scope: &'static str,
    pub runs: usize,
    pub successful_runs: usize,
    pub failed_runs: usize,
    pub tokens: u64,
    pub usd_micros: u64,
    pub reported_usd_micros: u64,
    pub estimated_usd_micros: u64,
    pub duration_ms: i64,
    pub cost_basis: &'static str,
}

/// `12 attributed usage` aggregates distinct run rows while keeping reported
/// and estimated spend separate and naming the task scope.
fn task_usage(state: &Arc<AppState>, rows: &[farseer_store::RunRow]) -> TaskUsage {
    let mut successful_runs = 0;
    let mut failed_runs = 0;
    let mut tokens: u64 = 0;
    let mut usd_micros: u64 = 0;
    let mut reported_usd_micros: u64 = 0;
    let mut estimated_usd_micros: u64 = 0;
    let mut duration_ms: i64 = 0;
    let mut has_reported = false;
    let mut has_estimated = false;
    let mut has_unknown = false;
    for row in rows {
        if row.outcome.as_deref() == Some("ok") {
            successful_runs += 1;
        } else if row.outcome.is_some() {
            failed_runs += 1;
        }
        tokens = tokens.saturating_add(row.tokens);
        usd_micros = usd_micros.saturating_add(row.usd_micros);
        duration_ms = duration_ms.saturating_add(
            row.finished_ts
                .unwrap_or_else(now_ms)
                .saturating_sub(row.started_ts)
                .max(0),
        );
        match crate::run_view(state, row.clone()).cost_basis {
            "reported" => {
                has_reported = true;
                reported_usd_micros = reported_usd_micros.saturating_add(row.usd_micros);
            }
            "estimated" => {
                has_estimated = true;
                estimated_usd_micros = estimated_usd_micros.saturating_add(row.usd_micros);
            }
            _ => has_unknown = true,
        }
    }
    let cost_basis = match (has_reported, has_estimated, has_unknown) {
        (true, false, false) => "reported",
        (false, true, false) => "estimated",
        (false, false, _) => "unknown",
        _ => "mixed",
    };
    TaskUsage {
        scope: "task",
        runs: rows.len(),
        successful_runs,
        failed_runs,
        tokens,
        usd_micros,
        reported_usd_micros,
        estimated_usd_micros,
        duration_ms,
        cost_basis,
    }
}

#[derive(Debug, Serialize)]
pub(super) struct TranscriptAttachmentView {
    #[serde(flatten)]
    pub attachment: TranscriptAttachment,
    pub projection: Option<TranscriptProjection>,
}

pub(crate) struct ProjectionRequest {
    attachment: TranscriptAttachment,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct SessionsQuery {
    pub project: Option<String>,
    pub conversation_id: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(super) struct SessionPage {
    pub rows: Vec<SessionRow>,
    pub next_offset: Option<usize>,
}

/// Bounded session explorer projection from `40 work model and session explorer`.
/// A missing log pointer is an honest unavailable state; farseer never guesses private harness paths.
pub(super) async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SessionsQuery>,
) -> ApiResult<Json<SessionPage>> {
    let project = query
        .project
        .as_deref()
        .map(|path| crate::projects::resolve(&state, path))
        .transpose()?
        .map(|path| crate::projects::display(&path));
    let conversation_id = query
        .conversation_id
        .as_deref()
        .map(parse_conversation)
        .transpose()?;
    let task_id = query.task_id.as_deref().map(parse_task).transpose()?;
    let run_id = query.run_id.as_deref().map(parse_run).transpose()?;
    let (rows, next_offset) = state.store().harness_session_page(
        query.limit.unwrap_or(100).min(500),
        query.offset.unwrap_or(0),
        project.as_deref(),
        conversation_id,
        task_id,
        run_id,
    )?;
    Ok(Json(SessionPage { rows, next_offset }))
}

pub(super) async fn get_task(
    State(state): State<Arc<AppState>>,
    UrlPath(task_id): UrlPath<String>,
) -> ApiResult<Json<TaskDetail>> {
    let task_id = parse_task(&task_id)?;
    let store = state.store();
    let task = store.task(task_id)?.ok_or(ApiError::NotFound("task"))?;
    let rows = store.runs_for_task(task_id)?;
    let mut sessions = Vec::new();
    let mut attachments = Vec::new();
    for row in &rows {
        sessions.extend(store.harness_sessions(Some(row.run_id))?);
        attachments.extend(
            store
                .transcript_attachments(Some(row.run_id))?
                .into_iter()
                .map(|attachment| {
                    let projection =
                        store.transcript_projection(&attachment.digest, attachment.run_id)?;
                    Ok(TranscriptAttachmentView {
                        attachment,
                        projection,
                    })
                })
                .collect::<farseer_store::Result<Vec<_>>>()?,
        );
    }
    let artifacts = store.artifacts_for_task(task_id)?;
    let transitions = store.task_transitions(task_id)?;
    let allowed_transitions = TaskState::ALL
        .into_iter()
        .filter(|to| *to != task.state && task.state.allows(*to))
        .collect();
    drop(store);
    // `run_view` and `task_usage` read the store again for cost, title, and
    // control provenance.  Release the outer guard first or this route
    // deadlocks while a worker-completed task is being opened (ticket 20).
    let usage = task_usage(&state, &rows);
    let runs = rows
        .into_iter()
        .map(|row| crate::run_view(&state, row))
        .collect();
    Ok(Json(TaskDetail {
        task,
        usage,
        allowed_transitions,
        transitions,
        runs,
        sessions,
        attachments,
        artifacts,
    }))
}

#[derive(Debug, Deserialize)]
pub(super) struct TransitionBody {
    pub state: String,
    pub reason: String,
}

pub(super) async fn transition_task(
    State(state): State<Arc<AppState>>,
    UrlPath(task_id): UrlPath<String>,
    Json(body): Json<TransitionBody>,
) -> ApiResult<Json<farseer_core::TaskTransition>> {
    if body.reason.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "task transition reason must not be empty",
        ));
    }
    let to = body
        .state
        .parse::<TaskState>()
        .map_err(|_| ApiError::BadRequest("unknown task state"))?;
    let changed = state.store().transition_task(
        parse_task(&task_id)?,
        to,
        Actor::Operator,
        body.reason.trim(),
        now_ms(),
    )?;
    Ok(Json(changed))
}

#[derive(Debug, Deserialize)]
pub(super) struct TranscriptBody {
    pub mode: String,
    pub path: String,
}

pub(super) async fn add_transcript(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
    Json(body): Json<TranscriptBody>,
) -> ApiResult<(StatusCode, Json<TranscriptAttachmentView>)> {
    let run_id = parse_run(&run_id)?;
    if state.store().run(run_id)?.is_none() {
        return Err(ApiError::NotFound("run"));
    }
    let custody = body
        .mode
        .parse::<TranscriptCustody>()
        .map_err(|_| ApiError::BadRequest("unknown transcript custody mode"))?;
    if body.path.trim().is_empty() {
        return Err(ApiError::BadRequest("transcript path must not be empty"));
    }
    let source = body.path;
    let (digest, stored_path) = match custody {
        TranscriptCustody::Reference => (sha256(format!("reference\0{source}").as_bytes()), None),
        TranscriptCustody::Copy | TranscriptCustody::CopyPlusIndex => {
            copy_transcript(&state, Path::new(&source))?
        }
    };
    let attachment = TranscriptAttachment {
        digest: digest.clone(),
        run_id,
        custody,
        source,
        stored_path,
        created_ts: now_ms(),
    };
    let store = state.store();
    store.record_transcript_attachment(&attachment)?;
    if custody == TranscriptCustody::CopyPlusIndex {
        store.queue_transcript_projection(&attachment, now_ms())?;
    }
    drop(store);
    if custody == TranscriptCustody::CopyPlusIndex {
        enqueue_projection(&state, attachment.clone());
    }
    let projection = state.store().transcript_projection(&digest, run_id)?;
    Ok((
        StatusCode::CREATED,
        Json(TranscriptAttachmentView {
            attachment,
            projection,
        }),
    ))
}

fn enqueue_projection(state: &Arc<AppState>, attachment: TranscriptAttachment) {
    let Some(sender) = state.transcript_queue.get() else {
        return;
    };
    if sender
        .try_send(ProjectionRequest {
            attachment: attachment.clone(),
        })
        .is_err()
    {
        let _ = state.store().set_transcript_projection_status(
            &attachment,
            "failed",
            Some("transcript analysis queue is full"),
            "restricted",
            now_ms(),
        );
    }
}

pub(crate) fn spawn_projection_worker(state: Arc<AppState>) {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
    if state.transcript_queue.set(sender.clone()).is_err() {
        return;
    }
    if let Ok(pending) = state.store().pending_transcript_projections() {
        for (index, attachment) in pending.into_iter().enumerate() {
            if index >= 32 {
                let _ = state.store().set_transcript_projection_status(
                    &attachment,
                    "failed",
                    Some("transcript analysis queue is full"),
                    "restricted",
                    now_ms(),
                );
                continue;
            }
            let _ = sender.try_send(ProjectionRequest { attachment });
        }
    }
    tokio::spawn(async move {
        while let Some(request) = receiver.recv().await {
            let worker_state = Arc::clone(&state);
            let attachment = request.attachment;
            let job_attachment = attachment.clone();
            let result = tokio::task::spawn_blocking(move || {
                project_transcript(&worker_state, &job_attachment)
            })
            .await
            .unwrap_or_else(|error| Err(format!("transcript analysis worker stopped: {error}")));
            if let Err(error) = result {
                let _ = state.store().set_transcript_projection_status(
                    &attachment,
                    "failed",
                    Some(&error),
                    "restricted",
                    now_ms(),
                );
            }
        }
    });
}

fn project_transcript(
    state: &AppState,
    attachment: &TranscriptAttachment,
) -> std::result::Result<(), String> {
    if attachment.custody != TranscriptCustody::CopyPlusIndex {
        return Ok(());
    }
    let projection = state
        .store()
        .transcript_projection(&attachment.digest, attachment.run_id)
        .map_err(|error| error.to_string())?;
    if projection.as_ref().map(|value| value.status.as_str()) != Some("pending") {
        return Ok(());
    }
    let path = attachment
        .stored_path
        .as_deref()
        .ok_or_else(|| "copy-plus-index has no stored transcript".to_owned())?;
    let bytes = read_bounded(Path::new(path)).map_err(|error| error.to_string())?;
    if sha256(&bytes) != attachment.digest {
        return Err("copied transcript changed before indexing; retry".into());
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| "copy-plus-index requires UTF-8 transcript text".to_owned())?;
    let scrubbed = farseer_core::scrub(&text);
    let documents = state
        .store()
        .indexed_transcripts_bounded(
            TRANSCRIPT_CANDIDATE_CAP,
            TRANSCRIPT_TEXT_CAP_BYTES.saturating_sub(scrubbed.len()),
        )
        .map_err(|error| error.to_string())?;
    let coverage = if documents.len() == TRANSCRIPT_CANDIDATE_CAP {
        "truncated"
    } else {
        "restricted"
    };
    let current = vector(&scrubbed);
    let mut edges = documents
        .into_iter()
        .filter(|(other, _)| other != &attachment.digest)
        .filter_map(|(other, body)| {
            let (left, right) = if attachment.digest < other {
                (attachment.digest.clone(), other.clone())
            } else {
                (other.clone(), attachment.digest.clone())
            };
            let edge = SimilarityEdge {
                left_digest: left,
                right_digest: right,
                score: cosine(&current, &vector(&body)),
                embedding_model: EMBEDDING_MODEL.into(),
                dimensions: DIMENSIONS as u32,
                distance_metric: "cosine".into(),
                redaction_version: REDACTION_VERSION.into(),
                projection_version: PROJECTION_VERSION.into(),
                source_digest: attachment.digest.clone(),
                evidence: vec![attachment.digest.clone(), other],
            };
            (edge.score > 0.0).then_some(edge)
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| right.score.total_cmp(&left.score));
    edges.truncate(TRANSCRIPT_EDGE_CAP);
    state
        .store()
        .commit_transcript_projection_with_coverage(
            attachment,
            &scrubbed,
            REDACTION_VERSION,
            PROJECTION_VERSION,
            coverage,
            &edges,
        )
        .map_err(|error| error.to_string())
}

pub(super) async fn retry_transcript(
    State(state): State<Arc<AppState>>,
    UrlPath((run_id, digest)): UrlPath<(String, String)>,
) -> ApiResult<Json<TranscriptAttachmentView>> {
    let run_id = parse_run(&run_id)?;
    let attachment = state
        .store()
        .transcript_attachments(Some(run_id))?
        .into_iter()
        .find(|attachment| attachment.digest == digest)
        .ok_or(ApiError::NotFound("transcript"))?;
    if attachment.custody != TranscriptCustody::CopyPlusIndex {
        return Err(ApiError::BadRequest(
            "transcript analysis was not requested",
        ));
    }
    state
        .store()
        .queue_transcript_projection(&attachment, now_ms())?;
    enqueue_projection(&state, attachment.clone());
    let projection = state.store().transcript_projection(&digest, run_id)?;
    Ok(Json(TranscriptAttachmentView {
        attachment,
        projection,
    }))
}

pub(super) async fn cancel_transcript(
    State(state): State<Arc<AppState>>,
    UrlPath((run_id, digest)): UrlPath<(String, String)>,
) -> ApiResult<Json<TranscriptAttachmentView>> {
    let run_id = parse_run(&run_id)?;
    let attachment = state
        .store()
        .transcript_attachments(Some(run_id))?
        .into_iter()
        .find(|attachment| attachment.digest == digest)
        .ok_or(ApiError::NotFound("transcript"))?;
    let projection = state
        .store()
        .transcript_projection(&digest, run_id)?
        .ok_or(ApiError::NotFound("transcript analysis"))?;
    if projection.status != "pending" {
        return Err(ApiError::BadRequest("transcript analysis is not pending"));
    }
    state.store().set_transcript_projection_status(
        &attachment,
        "cancelled",
        None,
        &projection.coverage,
        now_ms(),
    )?;
    let projection = state.store().transcript_projection(&digest, run_id)?;
    Ok(Json(TranscriptAttachmentView {
        attachment,
        projection,
    }))
}

pub(super) async fn list_transcripts(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<Json<Vec<TranscriptAttachmentView>>> {
    let store = state.store();
    let attachments = store
        .transcript_attachments(Some(parse_run(&run_id)?))?
        .into_iter()
        .map(|attachment| {
            let projection = store.transcript_projection(&attachment.digest, attachment.run_id)?;
            Ok(TranscriptAttachmentView {
                attachment,
                projection,
            })
        })
        .collect::<farseer_store::Result<Vec<_>>>()?;
    Ok(Json(attachments))
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchQuery {
    pub q: String,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(super) struct SearchHit {
    pub digest: String,
    pub excerpt: String,
    pub coverage: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_version: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct SearchPage {
    pub rows: Vec<SearchHit>,
    pub next_offset: Option<usize>,
}

pub(super) async fn search_transcripts(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Vec<SearchHit>>> {
    let needle = query.q.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let hits = state
        .store()
        .indexed_transcripts_bounded(TRANSCRIPT_CANDIDATE_CAP, TRANSCRIPT_TEXT_CAP_BYTES)?
        .into_iter()
        .filter_map(|(digest, body)| {
            body.to_lowercase().contains(&needle).then(|| SearchHit {
                digest,
                excerpt: body.chars().take(160).collect(),
                coverage: "restricted",
                projection_version: None,
            })
        })
        .take(50)
        .collect();
    Ok(Json(hits))
}

pub(super) async fn search_transcript_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<SearchPage>> {
    let needle = query.q.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Json(SearchPage {
            rows: Vec::new(),
            next_offset: None,
        }));
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0);
    let (documents, next_offset) = state.store().indexed_transcript_search_page(
        &needle,
        limit,
        offset,
        TRANSCRIPT_TEXT_CAP_BYTES,
    )?;
    let hits = documents
        .into_iter()
        .map(|document| {
            let farseer_store::IndexedTranscript {
                digest,
                body,
                projection_version,
            } = document;
            SearchHit {
                digest,
                excerpt: body.chars().take(160).collect(),
                coverage: "restricted",
                projection_version: Some(projection_version),
            }
        })
        .collect();
    Ok(Json(SearchPage {
        rows: hits,
        next_offset,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GraphScope {
    project_path: Option<String>,
    runner: Option<String>,
    from_ts: Option<i64>,
    to_ts: Option<i64>,
    kinds: Vec<String>,
    node_limit: usize,
    edge_limit: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct GraphCursor {
    version: u8,
    scope: GraphScope,
    offset: usize,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct GraphQuery {
    pub project: Option<String>,
    pub runner: Option<String>,
    #[serde(alias = "from")]
    pub from_ts: Option<i64>,
    #[serde(alias = "to")]
    pub to_ts: Option<i64>,
    #[serde(alias = "entity_kind")]
    pub kind: Option<String>,
    #[serde(alias = "node_limit")]
    pub limit: Option<usize>,
    #[serde(alias = "edge_budget")]
    pub edge_limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct WorkGraph {
    pub nodes: Vec<GraphNode>,
    pub observed_edges: Vec<GraphEdge>,
    pub derived_edges: Vec<GraphEdge>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub freshness: &'static str,
    pub generated_ts: i64,
}

pub(super) async fn graph(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GraphQuery>,
) -> ApiResult<Json<WorkGraph>> {
    if let (Some(from_ts), Some(to_ts)) = (query.from_ts, query.to_ts)
        && from_ts > to_ts
    {
        return Err(ApiError::BadRequest("from_ts must not exceed to_ts"));
    }
    let project_path = query
        .project
        .as_deref()
        .map(|path| crate::projects::resolve(&state, path))
        .transpose()?
        .map(|path| crate::projects::display(&path));
    let mut kinds = query
        .kind
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|kind| !kind.trim().is_empty())
        .map(|kind| kind.trim().to_owned())
        .collect::<Vec<_>>();
    kinds.sort();
    kinds.dedup();
    if kinds.iter().any(|kind| {
        !matches!(
            kind.as_str(),
            "project" | "conversation" | "task" | "run" | "session" | "attachment"
        )
    }) {
        return Err(ApiError::BadRequest("unknown graph entity kind"));
    }
    let scope = GraphScope {
        project_path,
        runner: query.runner.filter(|runner| !runner.trim().is_empty()),
        from_ts: query.from_ts,
        to_ts: query.to_ts,
        kinds,
        node_limit: query
            .limit
            .unwrap_or(GRAPH_NODE_DEFAULT)
            .clamp(1, GRAPH_NODE_MAX),
        edge_limit: query
            .edge_limit
            .unwrap_or(GRAPH_EDGE_DEFAULT)
            .clamp(1, GRAPH_EDGE_MAX),
    };
    let offset = query
        .cursor
        .as_deref()
        .map(decode_graph_cursor)
        .transpose()?
        .map(|cursor| {
            if cursor.version != 1 || cursor.scope != scope {
                return Err(ApiError::BadRequest(
                    "graph cursor does not match this scope",
                ));
            }
            Ok(cursor.offset)
        })
        .transpose()?
        .unwrap_or(0);
    let mut page = state.store().graph_page(&GraphFilter {
        project_path: scope.project_path.clone(),
        runner: scope.runner.clone(),
        from_ts: scope.from_ts,
        to_ts: scope.to_ts,
        kinds: scope.kinds.clone(),
        offset,
        node_limit: scope.node_limit,
        edge_limit: scope.edge_limit,
    })?;
    for node in &mut page.nodes {
        if let Some(target) = node.target.as_mut()
            && let Some(formatted) = format_compact_uuid(target)
        {
            *target = formatted;
        }
    }
    let next_cursor = page.has_more.then(|| {
        encode_graph_cursor(&GraphCursor {
            version: 1,
            scope: scope.clone(),
            offset: offset.saturating_add(page.nodes.len()),
        })
    });
    let mut response = WorkGraph {
        nodes: page.nodes,
        observed_edges: page.observed_edges,
        derived_edges: page.derived_edges,
        next_cursor,
        has_more: page.has_more,
        freshness: "eventual",
        generated_ts: now_ms(),
    };
    while serde_json::to_vec(&response)
        .expect("graph response is serializable")
        .len()
        > STRUCTURED_RESPONSE_MAX_BYTES
    {
        if response.derived_edges.pop().is_some() || response.observed_edges.pop().is_some() {
            response.has_more = true;
            continue;
        }
        return Err(ApiError::BadRequest(
            "graph node is too large for the structured response limit",
        ));
    }
    Ok(Json(response))
}

fn encode_graph_cursor(cursor: &GraphCursor) -> String {
    let bytes = serde_json::to_vec(cursor).expect("graph cursor is serializable");
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(encoded, "{byte:02x}").expect("writing to a string cannot fail");
    }
    encoded
}

fn decode_graph_cursor(value: &str) -> ApiResult<GraphCursor> {
    if value.is_empty() || value.len() > 4096 || !value.len().is_multiple_of(2) {
        return Err(ApiError::BadRequest("invalid graph cursor"));
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ApiError::BadRequest("invalid graph cursor"))?;
    serde_json::from_slice(&bytes).map_err(|_| ApiError::BadRequest("invalid graph cursor"))
}

fn format_compact_uuid(value: &str) -> Option<String> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}-{}-{}",
        &value[0..8],
        &value[8..12],
        &value[12..16],
        &value[16..20],
        &value[20..32]
    ))
}

fn copy_transcript(state: &AppState, source: &Path) -> ApiResult<(String, Option<String>)> {
    let bytes = read_bounded(source)?;
    let digest = sha256(&bytes);
    std::fs::create_dir_all(&state.transcript_dir)
        .map_err(|error| ApiError::Transcript(error.to_string()))?;
    let target = state.transcript_dir.join(&digest);
    if !target.exists() {
        std::fs::write(&target, &bytes).map_err(|error| ApiError::Transcript(error.to_string()))?;
    }
    Ok((digest, Some(target.display().to_string())))
}

fn read_bounded(source: &Path) -> ApiResult<Vec<u8>> {
    let mut file = std::fs::File::open(source)
        .map_err(|_| ApiError::BadRequest("transcript path is not a readable file"))?;
    let before = file
        .metadata()
        .map_err(|_| ApiError::BadRequest("transcript path is not a readable file"))?;
    if !before.is_file() || before.len() > TRANSCRIPT_CAP_BYTES {
        return Err(ApiError::BadRequest(
            "transcript file exceeds the 16 MiB copy cap",
        ));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(TRANSCRIPT_CAP_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ApiError::BadRequest("transcript path is not a readable file"))?;
    let after = file
        .metadata()
        .map_err(|_| ApiError::BadRequest("transcript path is not a readable file"))?;
    if bytes.len() as u64 > TRANSCRIPT_CAP_BYTES {
        return Err(ApiError::BadRequest(
            "transcript file exceeds the 16 MiB copy cap",
        ));
    }
    if before.len() != after.len() || after.len() != bytes.len() as u64 {
        return Err(ApiError::BadRequest(
            "transcript source changed while it was being read; retry",
        ));
    }
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

/// Build the versioned, rebuildable text projection selected by
/// `40 work model and session explorer`.
///
/// The fixed hash-TF buckets are deliberately local and deterministic rather
/// than a network embedding dependency; changing their meaning requires a new
/// `PROJECTION_VERSION` and `EMBEDDING_MODEL`.
fn vector(text: &str) -> [f64; DIMENSIONS] {
    let mut vector = [0.0; DIMENSIONS];
    for token in text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        let mut hasher = DefaultHasher::new();
        token.to_lowercase().hash(&mut hasher);
        vector[(hasher.finish() as usize) % DIMENSIONS] += 1.0;
    }
    vector
}

/// Score two `40 work model and session explorer` projection vectors.
///
/// The metric is stored on every derived edge, so rebuilding with another
/// metric cannot silently reinterpret prior scores.
fn cosine(left: &[f64; DIMENSIONS], right: &[f64; DIMENSIONS]) -> f64 {
    let dot = left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    let left_norm = left.iter().map(|value| value * value).sum::<f64>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f64>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm * right_norm)
    }
}

fn parse_conversation(value: &str) -> ApiResult<ConversationId> {
    value
        .parse()
        .map_err(|_| ApiError::NotFound("conversation"))
}

fn parse_task(value: &str) -> ApiResult<TaskId> {
    value.parse().map_err(|_| ApiError::NotFound("task"))
}

fn parse_run(value: &str) -> ApiResult<RunId> {
    value.parse().map_err(|_| ApiError::NotFound("run"))
}
