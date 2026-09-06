//! Public, bounded self-maintenance records.
//!
//! `17 bounded maintenance source proposals` requires maintenance to use the
//! ordinary task, run, and artifact projections rather than a second work
//! engine.  This module owns only the explicit proposal/evidence boundary;
//! candidate creation and promotion remain operator-controlled.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use farseer_core::{
    Actor, CellId, Conversation, ConversationId, EventKind, RunId, Task, TaskId, TaskState,
};
use farseer_store::maintenance::{
    BeginProposal, CandidateSource, MaintenanceAttempt, ProposalLedger, ProposalMetadata,
    ProposalRequest, ProposalStatus, ValidationEvidence,
};
use farseer_store::{ArtifactRow, RunRow};
use serde::{Deserialize, Serialize};

use crate::{ApiError, ApiResult, AppState, now_ms, projects};

const MAX_PROPOSALS: usize = 100;
const RUNNER: &str = "maintenance-fixture";

#[derive(Debug, Deserialize)]
pub(super) struct BeginBody {
    pub trigger_id: String,
    pub lineage_id: String,
    pub actor: String,
    pub source_revision: String,
    pub previous_revision: String,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub project: Option<String>,
    pub goal: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct BeginResponse {
    pub proposal: ProposalMetadata,
    pub created: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct EvidenceBody {
    pub artifact: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub reproducer: Option<String>,
    #[serde(default)]
    pub validation: Vec<ValidationEvidence>,
    #[serde(default = "default_validation_outcome")]
    pub outcome: String,
}

fn default_validation_outcome() -> String {
    "ok".into()
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct EvidenceResponse {
    pub proposal: ProposalMetadata,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub artifact: Option<ArtifactRow>,
}

pub(super) async fn list(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<ProposalMetadata>>> {
    let _gate = state.maintenance_gate();
    let ledger = load(&state)?;
    Ok(Json(
        ledger
            .proposals
            .into_iter()
            .rev()
            .take(MAX_PROPOSALS)
            .collect(),
    ))
}

pub(super) async fn begin(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BeginBody>,
) -> ApiResult<(StatusCode, Json<BeginResponse>)> {
    if body.goal.trim().is_empty() {
        return Err(ApiError::BadRequest("maintenance goal must not be empty"));
    }
    if !matches!(body.actor.as_str(), "operator" | "system") {
        return Err(ApiError::BadRequest(
            "maintenance actor must be `operator` or `system`",
        ));
    }
    let project = body
        .project
        .as_deref()
        .map(|path| projects::resolve(&state, path))
        .transpose()?;
    let project_path = project.as_deref().map(projects::display);
    let request = ProposalRequest {
        proposal_id: format!("proposal-{}", uuid::Uuid::now_v7()),
        lineage_id: body.lineage_id,
        trigger_id: body.trigger_id,
        actor: body.actor,
        source_revision: body.source_revision,
        previous_revision: body.previous_revision,
        scope: body.scope,
    };
    let _gate = state.maintenance_gate();
    let mut ledger = load(&state)?;
    let result = ledger.begin(request.clone()).map_err(maintenance_error)?;
    let BeginProposal::Created(proposal_id) = result else {
        let proposal_id = match result {
            BeginProposal::Existing(id) => id,
            BeginProposal::Created(_) => unreachable!(),
        };
        let proposal = ledger
            .proposals
            .iter()
            .find(|proposal| proposal.proposal_id == proposal_id)
            .cloned()
            .ok_or(ApiError::Corrupt("maintenance proposal ledger"))?;
        return Ok((
            StatusCode::OK,
            Json(BeginResponse {
                proposal,
                created: false,
            }),
        ));
    };

    let now = now_ms();
    let conversation_id = ConversationId::new();
    let task_id = TaskId::new();
    let conversation = Conversation {
        conversation_id,
        title: format!("Maintenance: {}", body.goal.trim()),
        project_path: project_path.clone(),
        manager_runner: Some(RUNNER.into()),
        created_ts: now,
        updated_ts: now,
        archived_ts: None,
    };
    let task = Task {
        task_id,
        conversation_id,
        goal: body.goal.trim().into(),
        title: format!("Maintenance: {}", body.goal.trim()),
        project_path,
        state: TaskState::InProgress,
        priority: 0,
        created_ts: now,
        updated_ts: now,
    };
    {
        let store = state.store();
        store.create_conversation(&conversation)?;
        store.create_task(&task)?;
        store.append(&farseer_core::NewEvent::new(
            CellId::new("zero"),
            RunId::none(),
            EventKind::new("maintenance_proposal_created"),
            Actor::Operator,
            now,
            serde_json::json!({
                "proposal_id": proposal_id,
                "task_id": task_id,
                "trigger_id": request.trigger_id,
                "lineage_id": request.lineage_id,
                "source_revision": request.source_revision,
                "scope": request.scope,
            }),
        ))?;
    }
    ledger
        .link_task(&proposal_id, task_id.to_string())
        .map_err(maintenance_error)?;
    save(&state, &ledger)?;
    let proposal = ledger
        .proposals
        .iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .cloned()
        .ok_or(ApiError::Corrupt("maintenance proposal ledger"))?;
    Ok((
        StatusCode::CREATED,
        Json(BeginResponse {
            proposal,
            created: true,
        }),
    ))
}

pub(super) async fn record_evidence(
    State(state): State<Arc<AppState>>,
    UrlPath(proposal_id): UrlPath<String>,
    Json(body): Json<EvidenceBody>,
) -> ApiResult<Json<EvidenceResponse>> {
    if body.artifact.trim().is_empty() {
        return Err(ApiError::BadRequest("candidate artifact must not be empty"));
    }
    let _gate = state.maintenance_gate();
    let mut ledger = load(&state)?;
    let proposal = ledger
        .proposals
        .iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .cloned()
        .ok_or(ApiError::NotFound("maintenance proposal"))?;
    let task_id = proposal
        .task_id
        .as_deref()
        .map(|id| id.parse::<TaskId>())
        .transpose()
        .map_err(|_| ApiError::Corrupt("maintenance task id"))?;
    let run_id = task_id.map(|_| RunId::new());
    let now = now_ms();
    let outcome = body.outcome.trim().to_ascii_lowercase();
    let succeeded = matches!(outcome.as_str(), "ok" | "passed" | "success" | "succeeded");
    let candidate = CandidateSource {
        artifact: body.artifact.clone(),
        branch: body.branch.clone(),
        reproducer: body.reproducer.clone(),
        validation: body.validation.clone(),
    };
    let attempt = MaintenanceAttempt {
        number: proposal.attempts.len() + 1,
        started_ts: now,
        finished_ts: Some(now),
        evidence: body.validation.clone(),
    };
    ledger
        .record_attempt(&proposal_id, attempt)
        .map_err(maintenance_error)?;
    ledger
        .attach_candidate(&proposal_id, candidate)
        .map_err(maintenance_error)?;
    ledger
        .finish(
            &proposal_id,
            if succeeded {
                ProposalStatus::Succeeded
            } else {
                ProposalStatus::Failed
            },
        )
        .map_err(maintenance_error)?;

    let mut artifact = None;
    if let (Some(task_id), Some(run_id)) = (task_id, run_id) {
        let run = RunRow {
            run_id,
            task_id,
            cell_id: CellId::new("zero"),
            runner: RUNNER.into(),
            model: "local".into(),
            outcome: Some(if succeeded { "ok" } else { "failed" }.into()),
            usd_micros: 0,
            tokens: 0,
            operator_touched: false,
            started_ts: now,
            finished_ts: Some(now),
        };
        let row = ArtifactRow {
            artifact_id: run_id,
            task_id,
            run_id,
            kind: "maintenance-candidate".into(),
            status: if succeeded { "complete" } else { "failed" }.into(),
            input_path: proposal.source_revision.clone(),
            staged_path: body.artifact.clone(),
            final_path: body.branch.clone(),
            error: (!succeeded).then(|| outcome.clone()),
            created_ts: now,
            finished_ts: Some(now),
        };
        let store = state.store();
        store.upsert_run(&run)?;
        store.create_artifact(&row)?;
        store.transition_task(
            task_id,
            if succeeded {
                TaskState::Review
            } else {
                TaskState::Blocked
            },
            Actor::Operator,
            if succeeded {
                "maintenance candidate and validation evidence recorded"
            } else {
                "maintenance candidate validation failed"
            },
            now,
        )?;
        store.append(&farseer_core::NewEvent::new(
            CellId::new("zero"),
            run_id,
            EventKind::new(EventKind::RUN_FINISHED),
            Actor::Operator,
            now,
            serde_json::json!({
                "outcome": if succeeded { "ok" } else { "failed" },
                "proposal_id": proposal_id,
                "artifact": body.artifact,
            }),
        ))?;
        artifact = Some(row);
    }
    save(&state, &ledger)?;
    let proposal = ledger
        .proposals
        .iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .cloned()
        .ok_or(ApiError::Corrupt("maintenance proposal ledger"))?;
    Ok(Json(EvidenceResponse {
        task_id: proposal.task_id.clone(),
        proposal,
        run_id: run_id.map(|id| id.to_string()),
        artifact,
    }))
}

pub(super) async fn cancel(
    State(state): State<Arc<AppState>>,
    UrlPath(proposal_id): UrlPath<String>,
) -> ApiResult<Json<ProposalMetadata>> {
    let _gate = state.maintenance_gate();
    let mut ledger = load(&state)?;
    let proposal = ledger
        .proposals
        .iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .cloned()
        .ok_or(ApiError::NotFound("maintenance proposal"))?;
    ledger
        .finish(&proposal_id, ProposalStatus::Cancelled)
        .map_err(maintenance_error)?;
    if let Some(task_id) = proposal.task_id.as_deref().and_then(|id| id.parse().ok()) {
        state.store().transition_task(
            task_id,
            TaskState::Cancelled,
            Actor::Operator,
            "maintenance proposal cancelled",
            now_ms(),
        )?;
    }
    save(&state, &ledger)?;
    ledger
        .proposals
        .into_iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .ok_or(ApiError::Corrupt("maintenance proposal ledger"))
        .map(Json)
}

fn load(state: &AppState) -> ApiResult<ProposalLedger> {
    if !state.maintenance_path().is_file() {
        return Ok(ProposalLedger::default());
    }
    ProposalLedger::load(state.maintenance_path()).map_err(maintenance_error)
}

fn save(state: &AppState, ledger: &ProposalLedger) -> ApiResult<()> {
    ledger
        .save(state.maintenance_path())
        .map_err(maintenance_error)
}

fn maintenance_error(error: farseer_store::maintenance::MaintenanceError) -> ApiError {
    ApiError::Policy(format!("maintenance: {error}"))
}
