//! Public, bounded self-maintenance records.
//!
//! `17 bounded maintenance source proposals` requires maintenance to use the
//! ordinary task, run, and artifact projections rather than a second work
//! engine.  This module owns the explicit proposal/evidence boundary;
//! candidate creation is deterministic and promotion remains operator-controlled.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
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

#[derive(Debug)]
struct WorkerEvidence {
    candidate: CandidateSource,
    outcome: String,
    repo: std::path::PathBuf,
    workspace: std::path::PathBuf,
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
    let outcome = body.outcome.trim().to_ascii_lowercase();
    let succeeded = matches!(outcome.as_str(), "ok" | "passed" | "success" | "succeeded");
    if succeeded && body.validation.is_empty() {
        return Err(ApiError::BadRequest(
            "successful maintenance evidence requires validation",
        ));
    }
    let candidate = CandidateSource {
        artifact: body.artifact.clone(),
        branch: body.branch.clone(),
        reproducer: body.reproducer.clone(),
        validation: body.validation.clone(),
    };
    let response = persist_evidence(
        &state,
        &mut ledger,
        &proposal,
        candidate,
        outcome,
        Actor::Operator,
        false,
    )?;
    save(&state, &ledger)?;
    Ok(Json(response))
}

/// Create one deterministic candidate branch and record its local validation.
///
/// The worker writes only beneath the run directory and keeps the candidate
/// worktree available for inspection.  It never changes the active checkout or
/// invokes promotion, so a successful candidate cannot replace the process
/// that created it.
pub(super) async fn execute(
    State(state): State<Arc<AppState>>,
    UrlPath(proposal_id): UrlPath<String>,
) -> ApiResult<Json<EvidenceResponse>> {
    if !state.try_start_maintenance_worker() {
        return Err(ApiError::Policy(
            "a maintenance worker is already running".into(),
        ));
    }
    let proposal = match (|| -> ApiResult<ProposalMetadata> {
        let _gate = state.maintenance_gate();
        let ledger = load(&state)?;
        ledger
            .proposals
            .iter()
            .find(|proposal| proposal.proposal_id == proposal_id)
            .cloned()
            .ok_or(ApiError::NotFound("maintenance proposal"))
    })() {
        Ok(proposal) => proposal,
        Err(error) => {
            state.finish_maintenance_worker();
            return Err(error);
        }
    };
    if !matches!(proposal.status, ProposalStatus::Open) {
        state.finish_maintenance_worker();
        return Err(ApiError::Policy("maintenance proposal is not open".into()));
    }
    let task = match (|| -> ApiResult<farseer_core::Task> {
        let task_id = proposal
            .task_id
            .as_deref()
            .ok_or(ApiError::Corrupt("maintenance task id"))?
            .parse::<TaskId>()
            .map_err(|_| ApiError::Corrupt("maintenance task id"))?;
        state
            .store()
            .task(task_id)?
            .ok_or(ApiError::Corrupt("maintenance task"))
    })() {
        Ok(task) => task,
        Err(error) => {
            state.finish_maintenance_worker();
            return Err(error);
        }
    };
    let repo_root = match task.project_path.as_deref() {
        Some(project) => projects::resolve(&state, project)?,
        None => state.repo_root().to_path_buf(),
    };
    let worker_state = Arc::clone(&state);
    let worker_proposal = proposal.clone();
    let worker_repo = repo_root.clone();
    let worker_goal = task.goal.clone();
    let evidence = match tokio::task::spawn_blocking(move || {
        run_candidate_worker(&worker_state, &worker_proposal, &worker_repo, &worker_goal)
    })
    .await
    {
        Ok(evidence) => evidence,
        Err(error) => {
            state.finish_maintenance_worker();
            return Err(ApiError::Policy(format!(
                "maintenance worker failed to join: {error}"
            )));
        }
    };
    let result = (|| -> ApiResult<EvidenceResponse> {
        let _gate = state.maintenance_gate();
        let mut ledger = load(&state)?;
        let proposal = ledger
            .proposals
            .iter()
            .find(|proposal| proposal.proposal_id == proposal_id)
            .cloned()
            .ok_or(ApiError::NotFound("maintenance proposal"))?;
        if !matches!(proposal.status, ProposalStatus::Open) {
            cleanup_candidate(&evidence);
            return Err(ApiError::Policy("maintenance proposal is not open".into()));
        }
        let response = persist_evidence(
            &state,
            &mut ledger,
            &proposal,
            evidence.candidate,
            evidence.outcome,
            Actor::System,
            false,
        )?;
        save(&state, &ledger)?;
        Ok(response)
    })();
    state.finish_maintenance_worker();
    result.map(Json)
}

fn persist_evidence(
    state: &AppState,
    ledger: &mut ProposalLedger,
    proposal: &ProposalMetadata,
    candidate: CandidateSource,
    outcome: String,
    actor: Actor,
    operator_touched: bool,
) -> ApiResult<EvidenceResponse> {
    let succeeded = matches!(outcome.as_str(), "ok" | "passed" | "success" | "succeeded");
    if succeeded && candidate.validation.is_empty() {
        return Err(ApiError::BadRequest(
            "successful maintenance evidence requires validation",
        ));
    }
    let task_id = proposal
        .task_id
        .as_deref()
        .map(|id| id.parse::<TaskId>())
        .transpose()
        .map_err(|_| ApiError::Corrupt("maintenance task id"))?;
    let run_id = task_id.map(|_| RunId::new());
    let now = now_ms();
    ledger
        .record_attempt(
            &proposal.proposal_id,
            MaintenanceAttempt {
                number: proposal.attempts.len() + 1,
                started_ts: now,
                finished_ts: Some(now),
                evidence: candidate.validation.clone(),
            },
        )
        .map_err(maintenance_error)?;
    ledger
        .attach_candidate(&proposal.proposal_id, candidate.clone())
        .map_err(maintenance_error)?;
    ledger
        .finish(
            &proposal.proposal_id,
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
            operator_touched,
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
            staged_path: candidate.artifact.clone(),
            final_path: candidate.branch.clone(),
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
            actor,
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
            actor,
            now,
            serde_json::json!({
                "outcome": if succeeded { "ok" } else { "failed" },
                "proposal_id": proposal.proposal_id,
                "artifact": candidate.artifact,
            }),
        ))?;
        artifact = Some(row);
    }
    let proposal = ledger
        .proposals
        .iter()
        .find(|item| item.proposal_id == proposal.proposal_id)
        .cloned()
        .ok_or(ApiError::Corrupt("maintenance proposal ledger"))?;
    Ok(EvidenceResponse {
        task_id: proposal.task_id.clone(),
        proposal,
        run_id: run_id.map(|id| id.to_string()),
        artifact,
    })
}

fn run_candidate_worker(
    state: &AppState,
    proposal: &ProposalMetadata,
    repo_root: &Path,
    goal: &str,
) -> WorkerEvidence {
    let id = safe_component(&proposal.proposal_id);
    let root = state.runs_dir().join("maintenance").join(&id);
    let workspace = root.join("workspace");
    let mut artifact_path = workspace.join(format!(".farseer-maintenance-candidate-{id}.md"));
    let reproducer_path = workspace.join("farseer-maintenance-reproducer.txt");
    let branch = format!("farseer/maintenance/{id}");
    let artifact = display_path(&artifact_path);
    let reproducer = display_path(&reproducer_path);
    let mut validation = Vec::new();
    let mut worktree_added = false;

    let setup = (|| -> Result<(), String> {
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let source_revision = resolve_revision(repo_root, &proposal.source_revision)?;
        let output = git(
            repo_root,
            &[
                "worktree".into(),
                "add".into(),
                "-b".into(),
                branch.clone(),
                workspace.display().to_string(),
                source_revision,
            ],
        )?;
        if !output.status.success() {
            return Err(command_detail(&output));
        }
        worktree_added = true;
        artifact_path = scoped_candidate_path(&workspace, &proposal.scope, &id)?;
        fs::write(
            &artifact_path,
            format!(
                "# Farseer maintenance candidate\n\nproposal: {}\ngoal: {}\nsource: {}\nscope: {}\n",
                proposal.proposal_id,
                goal,
                proposal.source_revision,
                proposal.scope.join(", ")
            ),
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            &reproducer_path,
            format!(
                "reproduce trigger `{}` from source revision `{}`\n",
                proposal.trigger_id, proposal.source_revision
            ),
        )
        .map_err(|error| error.to_string())?;
        let diff = git(&workspace, &["diff".into(), "--check".into()])?;
        validation.push(validation_row("git diff --check", &diff));
        if !diff.status.success() {
            return Err("candidate diff validation failed".into());
        }
        if workspace.join("Cargo.toml").is_file() {
            let formatting = process(
                "cargo",
                &workspace,
                &["fmt".into(), "--all".into(), "--".into(), "--check".into()],
            )?;
            validation.push(validation_row("cargo fmt --all -- --check", &formatting));
            if !formatting.status.success() {
                return Err("candidate formatting validation failed".into());
            }
            let (test_args, test_command) = if workspace.join("crates/farseer-store").is_dir() {
                (
                    vec![
                        "test".into(),
                        "-p".into(),
                        "farseer-store".into(),
                        "maintenance::tests::proposal_trigger_deduplicates_and_bounds_attempts"
                            .into(),
                        "--lib".into(),
                    ],
                    "cargo test -p farseer-store maintenance::tests::proposal_trigger_deduplicates_and_bounds_attempts --lib",
                )
            } else {
                (
                    vec!["test".into(), "--workspace".into(), "--lib".into()],
                    "cargo test --workspace --lib",
                )
            };
            let tests = process("cargo", &workspace, &test_args)?;
            validation.push(validation_row(test_command, &tests));
            if !tests.status.success() {
                return Err("candidate repository validation failed".into());
            }
        } else {
            validation.push(ValidationEvidence {
                command: "repository fixture validation".into(),
                outcome: "ok".into(),
                exit_code: Some(0),
                detail: Some("no Cargo.toml; structural fixture validation only".into()),
            });
        }
        let add = git(&workspace, &["add".into(), "--".into(), ".".into()])?;
        if !add.status.success() {
            return Err(command_detail(&add));
        }
        let commit = git(
            &workspace,
            &[
                "-c".into(),
                "user.name=Farseer Maintenance".into(),
                "-c".into(),
                "user.email=farseer@localhost".into(),
                "commit".into(),
                "--quiet".into(),
                "-m".into(),
                "Create maintenance candidate".into(),
            ],
        )?;
        if !commit.status.success() {
            return Err(command_detail(&commit));
        }
        let status = git(
            &workspace,
            &["status".into(), "--short".into(), "--branch".into()],
        )?;
        validation.push(validation_row("git status --short --branch", &status));
        if !status.status.success() {
            return Err("candidate validation failed".into());
        }
        Ok(())
    })();

    match setup {
        Ok(()) => WorkerEvidence {
            candidate: CandidateSource {
                artifact: display_path(&artifact_path),
                branch: Some(branch),
                reproducer: Some(reproducer),
                validation,
            },
            outcome: "ok".into(),
            repo: repo_root.to_path_buf(),
            workspace,
        },
        Err(error) => {
            if worktree_added {
                let _ = git(
                    repo_root,
                    &[
                        "worktree".into(),
                        "remove".into(),
                        "--force".into(),
                        workspace.display().to_string(),
                    ],
                );
                let _ = git(repo_root, &["branch".into(), "-D".into(), branch.clone()]);
            }
            if validation.is_empty() {
                validation.push(ValidationEvidence {
                    command: "maintenance candidate setup".into(),
                    outcome: "failed".into(),
                    exit_code: None,
                    detail: Some(error.clone()),
                });
            }
            WorkerEvidence {
                candidate: CandidateSource {
                    artifact,
                    branch: None,
                    reproducer: None,
                    validation,
                },
                outcome: error,
                repo: repo_root.to_path_buf(),
                workspace,
            }
        }
    }
}

fn git(repo: &Path, args: &[String]) -> Result<Output, String> {
    process("git", repo, args)
}

fn process(program: &str, directory: &Path, args: &[String]) -> Result<Output, String> {
    Command::new(program)
        .current_dir(directory)
        .args(args)
        .output()
        .map_err(|error| format!("{program}: {error}"))
}

fn resolve_revision(repo: &Path, revision: &str) -> Result<String, String> {
    let revision = revision.trim();
    if revision.is_empty() || revision.starts_with('-') || revision.contains('\0') {
        return Err("source revision must be a commit reference".into());
    }
    let expression = format!("{revision}^{{commit}}");
    let output = git(
        repo,
        &[
            "rev-parse".into(),
            "--verify".into(),
            "--end-of-options".into(),
            expression,
        ],
    )?;
    if !output.status.success() {
        return Err(format!(
            "source revision does not resolve: {}",
            command_detail(&output)
        ));
    }
    let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !(40..=64).contains(&resolved.len())
        || !resolved.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("source revision did not resolve to an object id".into());
    }
    Ok(resolved)
}

fn scoped_candidate_path(
    workspace: &Path,
    scope: &[String],
    id: &str,
) -> Result<std::path::PathBuf, String> {
    let relative = scope
        .first()
        .map(Path::new)
        .unwrap_or_else(|| Path::new("."));
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
                    | std::path::Component::ParentDir
            )
        })
    {
        return Err("maintenance scope must stay inside the candidate workspace".into());
    }
    let target = workspace.join(relative);
    let directory = if target.is_dir() {
        target
    } else if target.is_file() {
        target
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "maintenance scope has no parent directory".to_string())?
    } else {
        return Err(format!(
            "maintenance scope does not exist: {}",
            relative.display()
        ));
    };
    Ok(directory.join(format!(".farseer-maintenance-candidate-{id}.md")))
}

fn cleanup_candidate(evidence: &WorkerEvidence) {
    let _ = git(
        &evidence.repo,
        &[
            "worktree".into(),
            "remove".into(),
            "--force".into(),
            evidence.workspace.display().to_string(),
        ],
    );
    if let Some(branch) = evidence.candidate.branch.as_deref() {
        let _ = git(
            &evidence.repo,
            &["branch".into(), "-D".into(), branch.into()],
        );
    }
}

fn command_detail(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if text.is_empty() {
        format!("command exited with {}", output.status)
    } else {
        text.chars().take(512).collect()
    }
}

fn validation_row(command: &str, output: &Output) -> ValidationEvidence {
    let mut detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        if !detail.is_empty() {
            detail.push('\n');
        }
        detail.push_str(&stderr);
    }
    ValidationEvidence {
        command: command.into(),
        outcome: if output.status.success() {
            "ok".into()
        } else {
            "failed".into()
        },
        exit_code: output.status.code(),
        detail: (!detail.is_empty()).then(|| detail.chars().take(512).collect()),
    }
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
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
        let run_id = RunId::new();
        let now = now_ms();
        let run = RunRow {
            run_id,
            task_id,
            cell_id: CellId::new("zero"),
            runner: RUNNER.into(),
            model: "local".into(),
            outcome: Some("cancelled".into()),
            usd_micros: 0,
            tokens: 0,
            operator_touched: true,
            started_ts: now,
            finished_ts: Some(now),
        };
        let artifact = ArtifactRow {
            artifact_id: run_id,
            task_id,
            run_id,
            kind: "maintenance-candidate".into(),
            status: "cancelled".into(),
            input_path: proposal.source_revision.clone(),
            staged_path: String::new(),
            final_path: None,
            error: Some("cancelled by operator".into()),
            created_ts: now,
            finished_ts: Some(now),
        };
        let store = state.store();
        store.upsert_run(&run)?;
        store.create_artifact(&artifact)?;
        store.transition_task(
            task_id,
            TaskState::Cancelled,
            Actor::Operator,
            "maintenance proposal cancelled",
            now,
        )?;
        store.append(&farseer_core::NewEvent::new(
            CellId::new("zero"),
            run_id,
            EventKind::new(EventKind::RUN_FINISHED),
            Actor::Operator,
            now,
            serde_json::json!({
                "outcome": "cancelled",
                "proposal_id": proposal_id,
                "reason": "cancelled by operator",
            }),
        ))?;
    }
    save(&state, &ledger)?;
    ledger
        .proposals
        .into_iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .ok_or(ApiError::Corrupt("maintenance proposal ledger"))
        .map(Json)
}

/// Return the maintenance proposal linked to an ordinary task.
///
/// `17 bounded maintenance source proposals` keeps proposal metadata in its
/// bounded ledger while task detail remains the operator's single work view.
pub(super) fn for_task(
    state: &AppState,
    task_id: farseer_core::TaskId,
) -> ApiResult<Option<ProposalMetadata>> {
    let _gate = state.maintenance_gate();
    let ledger = load(state)?;
    let task_id = task_id.to_string();
    Ok(ledger
        .proposals
        .into_iter()
        .find(|proposal| proposal.task_id.as_deref() == Some(task_id.as_str())))
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
