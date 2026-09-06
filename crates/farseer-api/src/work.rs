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
use farseer_store::{SimilarityEdge, TaskCursor, TaskFilter, TranscriptAttachment};
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
const ALL_ROWS: usize = i64::MAX as usize;
const TASK_PAGE_DEFAULT: usize = 100;
const TASK_PAGE_MAX: usize = 500;
const STRUCTURED_RESPONSE_MAX_BYTES: usize = 1024 * 1024;

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
pub(super) struct TaskPage {
    pub tasks: Vec<Task>,
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
    let generated_ts = now_ms();
    let mut page = TaskPage {
        tasks,
        next_cursor: None,
        has_more,
        freshness: "eventual",
        generated_ts,
    };
    page.next_cursor = page.tasks.last().filter(|_| page.has_more).map(|task| {
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
        page.next_cursor = page.tasks.last().map(|task| {
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
    pub allowed_transitions: Vec<TaskState>,
    pub transitions: Vec<farseer_core::TaskTransition>,
    pub runs: Vec<crate::RunView>,
    pub sessions: Vec<farseer_core::HarnessSession>,
    pub attachments: Vec<TranscriptAttachment>,
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
        attachments.extend(store.transcript_attachments(Some(row.run_id))?);
    }
    let transitions = store.task_transitions(task_id)?;
    let allowed_transitions = TaskState::ALL
        .into_iter()
        .filter(|to| *to != task.state && task.state.allows(*to))
        .collect();
    drop(store);
    let runs = rows
        .into_iter()
        .map(|row| crate::run_view(&state, row))
        .collect();
    Ok(Json(TaskDetail {
        task,
        allowed_transitions,
        transitions,
        runs,
        sessions,
        attachments,
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
) -> ApiResult<(StatusCode, Json<TranscriptAttachment>)> {
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
    let (digest, stored_path, indexed) = match custody {
        TranscriptCustody::Reference => (
            sha256(format!("reference\0{source}").as_bytes()),
            None,
            None,
        ),
        TranscriptCustody::Copy | TranscriptCustody::CopyPlusIndex => copy_transcript(
            &state,
            Path::new(&source),
            custody == TranscriptCustody::CopyPlusIndex,
        )?,
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
    drop(store);
    if let Some(body) = indexed {
        let scrubbed = farseer_core::scrub(&body);
        let documents = state.store().indexed_transcripts_bounded(
            TRANSCRIPT_CANDIDATE_CAP,
            TRANSCRIPT_TEXT_CAP_BYTES.saturating_sub(scrubbed.len()),
        )?;
        let current = vector(&scrubbed);
        let edges = documents
            .into_iter()
            .filter(|(other, _)| other != &digest)
            .filter_map(|(other, body)| {
                let (left, right) = if digest < other {
                    (digest.clone(), other.clone())
                } else {
                    (other.clone(), digest.clone())
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
                    source_digest: digest.clone(),
                    evidence: vec![digest.clone(), other],
                };
                (edge.score > 0.0).then_some(edge)
            })
            .collect::<Vec<_>>();
        let mut edges = edges;
        edges.sort_by(|left, right| right.score.total_cmp(&left.score));
        edges.truncate(TRANSCRIPT_EDGE_CAP);
        if let Some(stored_path) = &attachment.stored_path {
            let stored = read_bounded(Path::new(stored_path))?;
            if sha256(&stored) != digest {
                return Err(ApiError::Transcript(
                    "copied transcript changed before indexing".into(),
                ));
            }
        }
        state.store().commit_transcript_projection(
            &attachment,
            &scrubbed,
            REDACTION_VERSION,
            PROJECTION_VERSION,
            &edges,
        )?;
    }
    Ok((StatusCode::CREATED, Json(attachment)))
}

pub(super) async fn list_transcripts(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<Json<Vec<TranscriptAttachment>>> {
    Ok(Json(
        state
            .store()
            .transcript_attachments(Some(parse_run(&run_id)?))?,
    ))
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchQuery {
    pub q: String,
}

#[derive(Debug, Serialize)]
pub(super) struct SearchHit {
    pub digest: String,
    pub excerpt: String,
    pub coverage: &'static str,
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
            })
        })
        .take(50)
        .collect();
    Ok(Json(hits))
}

#[derive(Debug, Serialize)]
pub(super) struct WorkGraph {
    pub projects: Vec<String>,
    pub conversations: Vec<Conversation>,
    pub tasks: Vec<Task>,
    pub runs: Vec<GraphRun>,
    pub sessions: Vec<farseer_core::HarnessSession>,
    pub attachments: Vec<TranscriptAttachment>,
    pub parents: Vec<farseer_store::RunParent>,
    pub similarities: Vec<SimilarityEdge>,
}

#[derive(Debug, Serialize)]
pub(super) struct GraphRun {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub cell_id: farseer_core::CellId,
    pub runner: String,
    pub outcome: Option<String>,
}

pub(super) async fn graph(State(state): State<Arc<AppState>>) -> ApiResult<Json<WorkGraph>> {
    let store = state.store();
    let conversations = store.conversations(ALL_ROWS)?;
    let tasks = store.tasks(&TaskFilter {
        limit: ALL_ROWS,
        ..Default::default()
    })?;
    let projects = conversations
        .iter()
        .filter_map(|conversation| conversation.project_path.clone())
        .chain(tasks.iter().filter_map(|task| task.project_path.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let runs = store
        .recent_runs(ALL_ROWS)?
        .into_iter()
        .map(|row| GraphRun {
            run_id: row.run_id,
            task_id: row.task_id,
            cell_id: row.cell_id,
            runner: row.runner,
            outcome: row.outcome,
        })
        .collect();
    Ok(Json(WorkGraph {
        projects,
        conversations,
        tasks,
        runs,
        sessions: store.harness_sessions(None)?,
        attachments: store.transcript_attachments(None)?,
        parents: store.run_parents()?,
        similarities: store.similarity_edges()?,
    }))
}

fn copy_transcript(
    state: &AppState,
    source: &Path,
    index: bool,
) -> ApiResult<(String, Option<String>, Option<String>)> {
    let bytes = read_bounded(source)?;
    let digest = sha256(&bytes);
    std::fs::create_dir_all(&state.transcript_dir)
        .map_err(|error| ApiError::Transcript(error.to_string()))?;
    let target = state.transcript_dir.join(&digest);
    if !target.exists() {
        std::fs::write(&target, &bytes).map_err(|error| ApiError::Transcript(error.to_string()))?;
    }
    let text =
        if index {
            Some(String::from_utf8(bytes).map_err(|_| {
                ApiError::BadRequest("copy-plus-index requires UTF-8 transcript text")
            })?)
        } else {
            None
        };
    Ok((digest, Some(target.display().to_string()), text))
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
