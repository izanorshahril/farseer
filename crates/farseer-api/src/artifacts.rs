//! Deterministic local artifact jobs.
//!
//! The manifest worker is intentionally a filesystem slice: no Git, network,
//! provider runner or domain-specific execution engine is involved.
//!
//! `20 noncoding artifact-manifest pipeline` is the source of truth for its
//! authorization, deterministic ordering, staged output and cancellation rules.

use std::fmt::Write as FmtWrite;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use farseer_core::{
    Actor, CellId, Conversation, ConversationId, EventKind, RunId, Task, TaskId, TaskState,
};
use farseer_store::{ArtifactRow, RunRow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{ApiError, ApiResult, AppState, now_ms, projects};

const KIND: &str = "artifact-manifest";
const RUNNER: &str = "local-manifest";
const MAX_MANIFEST_ENTRIES: usize = 100_000;
const MAX_MANIFEST_PATH_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub(super) struct StartBody {
    /// The authorized project containing the input directory.
    pub project: String,
    /// An existing directory inside `project`.
    #[serde(alias = "input_path", alias = "directory")]
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct StartResponse {
    pub task_id: String,
    pub conversation_id: String,
    pub run_id: String,
    pub artifact: ArtifactRow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ManifestEntry {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Debug)]
enum WorkerResult {
    Complete,
    Cancelled,
}

pub(super) async fn start_manifest(
    State(state): State<Arc<AppState>>,
    Json(body): Json<StartBody>,
) -> ApiResult<(StatusCode, Json<StartResponse>)> {
    let project = projects::resolve(&state, &body.project)?;
    let input = projects::resolve(&state, &body.input)?;
    if !input.starts_with(&project) {
        return Err(ApiError::Forbidden(
            "the manifest input must be inside the selected project",
        ));
    }
    let artifact_slot = Arc::clone(&state.artifact_slots)
        .try_acquire_owned()
        .map_err(|_| ApiError::Policy("artifact worker queue is full; retry later".into()))?;
    state.admit_run()?;

    let now = now_ms();
    let conversation_id = ConversationId::new();
    let task_id = TaskId::new();
    let run_id = RunId::new();
    let project_path = projects::display(&project);
    let input_path = projects::display(&input);
    let title = format!("Manifest {input_path}");
    let conversation = Conversation {
        conversation_id,
        title: title.clone(),
        project_path: Some(project_path.clone()),
        manager_runner: Some(RUNNER.into()),
        created_ts: now,
        updated_ts: now,
        archived_ts: None,
    };
    let task = Task {
        task_id,
        conversation_id,
        goal: format!("Create a deterministic artifact manifest for {input_path}"),
        title,
        project_path: Some(project_path),
        state: TaskState::InProgress,
        priority: 0,
        created_ts: now,
        updated_ts: now,
    };
    let stage_dir = state.runs_dir.join("artifacts").join(run_id.to_string());
    let staged_path = stage_dir.join("manifest.json.partial");
    let artifact = ArtifactRow {
        artifact_id: run_id,
        task_id,
        run_id,
        kind: KIND.into(),
        status: "running".into(),
        input_path: input_path.clone(),
        staged_path: staged_path.display().to_string(),
        final_path: None,
        error: None,
        created_ts: now,
        finished_ts: None,
    };
    let run = RunRow {
        run_id,
        task_id,
        cell_id: CellId::new("zero"),
        runner: RUNNER.into(),
        model: "local".into(),
        outcome: None,
        usd_micros: 0,
        tokens: 0,
        operator_touched: false,
        started_ts: now,
        finished_ts: None,
    };
    {
        let store = state.store();
        if let Err(error) = store
            .create_conversation(&conversation)
            .and_then(|_| store.create_task(&task))
            .and_then(|_| store.upsert_run(&run))
            .and_then(|_| store.create_artifact(&artifact))
            .and_then(|_| {
                store.append(&farseer_core::NewEvent::new(
                    CellId::new("zero"),
                    run_id,
                    EventKind::new(EventKind::RUN_QUEUED),
                    Actor::System,
                    now,
                    serde_json::json!({
                        "task_id": task_id,
                        "goal": task.goal,
                        "role": "worker",
                        "runner": RUNNER,
                        "artifact": KIND,
                        "input_path": input_path,
                    }),
                ))
            })
        {
            state.release_run();
            return Err(ApiError::Store(error));
        }
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    state
        .pending_cancellations
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(run_id, Arc::clone(&cancelled));
    let worker_state = Arc::clone(&state);
    let worker_input = input.clone();
    tokio::task::spawn_blocking(move || {
        let _artifact_slot = artifact_slot;
        let outcome = write_manifest(
            &worker_state,
            run_id,
            task_id,
            &worker_input,
            &stage_dir,
            &cancelled,
        );
        finish_manifest(&worker_state, run_id, task_id, &stage_dir, outcome);
        worker_state
            .pending_cancellations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&run_id);
        worker_state.release_run();
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(StartResponse {
            task_id: task_id.to_string(),
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
            artifact,
        }),
    ))
}

fn write_manifest(
    state: &AppState,
    run_id: RunId,
    task_id: TaskId,
    input: &Path,
    stage_dir: &Path,
    cancelled: &AtomicBool,
) -> Result<WorkerResult, String> {
    fs::create_dir_all(stage_dir).map_err(|error| error.to_string())?;
    let partial = stage_dir.join("manifest.json.partial");
    let file = File::create(&partial).map_err(|error| error.to_string())?;
    let mut output = BufWriter::new(file);
    output
        .write_all(br#"{"version":1,"entries":["#)
        .map_err(|error| error.to_string())?;
    let files = files_under(input, cancelled)?;
    let mut first = true;
    for (index, path) in files.iter().enumerate() {
        if cancelled.load(Ordering::Acquire) {
            output.flush().map_err(|error| error.to_string())?;
            return Ok(WorkerResult::Cancelled);
        }
        let entry = manifest_entry(input, path)?;
        if !first {
            output.write_all(b",").map_err(|error| error.to_string())?;
        }
        first = false;
        serde_json::to_writer(&mut output, &entry).map_err(|error| error.to_string())?;
        if index % 32 == 0 {
            output.flush().map_err(|error| error.to_string())?;
            let _ = state.store().append(&farseer_core::NewEvent::new(
                CellId::new("zero"),
                run_id,
                EventKind::new(EventKind::STATUS_CHANGED),
                Actor::System,
                now_ms(),
                serde_json::json!({ "artifact": KIND, "task_id": task_id, "entries": index + 1 }),
            ));
            std::thread::yield_now();
        }
    }
    if cancelled.load(Ordering::Acquire) {
        output.flush().map_err(|error| error.to_string())?;
        return Ok(WorkerResult::Cancelled);
    }
    output
        .write_all(b"]}")
        .and_then(|_| output.flush())
        .map_err(|error| error.to_string())?;
    let final_path = stage_dir.join("manifest.json");
    fs::rename(&partial, &final_path).map_err(|error| error.to_string())?;
    Ok(WorkerResult::Complete)
}

fn files_under(input: &Path, cancelled: &AtomicBool) -> Result<Vec<PathBuf>, String> {
    let mut pending = vec![input.to_path_buf()];
    let mut files = Vec::new();
    let mut path_bytes = 0usize;
    while let Some(directory) = pending.pop() {
        if cancelled.load(Ordering::Acquire) {
            return Ok(files);
        }
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| format!("reading {}: {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries.into_iter().rev() {
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                if files.len() >= MAX_MANIFEST_ENTRIES {
                    return Err(format!(
                        "manifest exceeds the {} file entry limit",
                        MAX_MANIFEST_ENTRIES
                    ));
                }
                path_bytes = path_bytes.saturating_add(entry.path().to_string_lossy().len());
                if path_bytes > MAX_MANIFEST_PATH_BYTES {
                    return Err(format!(
                        "manifest paths exceed the {} byte limit",
                        MAX_MANIFEST_PATH_BYTES
                    ));
                }
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

fn manifest_entry(input: &Path, path: &Path) -> Result<ManifestEntry, String> {
    let input = fs::canonicalize(input).map_err(|error| error.to_string())?;
    let resolved = fs::canonicalize(path).map_err(|error| error.to_string())?;
    if !resolved.starts_with(&input) {
        return Err(format!(
            "input contains a path outside the authorized directory: {}",
            path.display()
        ));
    }
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    let before = metadata.len();
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let after = file.metadata().map_err(|error| error.to_string())?.len();
    if before != after {
        return Err(format!("file changed while being read: {}", path.display()));
    }
    let mut sha256 = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut sha256, "{byte:02x}").map_err(|error| error.to_string())?;
    }
    Ok(ManifestEntry {
        path: resolved
            .strip_prefix(&input)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/"),
        size: before,
        sha256,
    })
}

fn finish_manifest(
    state: &AppState,
    run_id: RunId,
    task_id: TaskId,
    stage_dir: &Path,
    outcome: Result<WorkerResult, String>,
) {
    let now = now_ms();
    let started_ts = state
        .store()
        .run(run_id)
        .ok()
        .flatten()
        .map(|row| row.started_ts)
        .unwrap_or(now);
    let (run_outcome, artifact_status, reason, final_path, task_state) = match outcome {
        Ok(WorkerResult::Complete) => (
            "ok",
            "complete",
            "artifact manifest completed",
            Some(stage_dir.join("manifest.json")),
            TaskState::Review,
        ),
        Ok(WorkerResult::Cancelled) => (
            "cancelled",
            "cancelled",
            "artifact manifest cancelled; staged output is incomplete",
            None,
            TaskState::Cancelled,
        ),
        Err(error) => {
            let store = state.store();
            let _ = store.update_artifact(run_id, "failed", None, Some(&error), Some(now));
            let _ = store.transition_task(
                task_id,
                TaskState::Blocked,
                Actor::System,
                "artifact manifest failed",
                now,
            );
            let _ = store.upsert_run(&RunRow {
                run_id,
                task_id,
                cell_id: CellId::new("zero"),
                runner: RUNNER.into(),
                model: "local".into(),
                outcome: Some("failed".into()),
                usd_micros: 0,
                tokens: 0,
                operator_touched: false,
                started_ts,
                finished_ts: Some(now),
            });
            let _ = store.append(&farseer_core::NewEvent::new(
                CellId::new("zero"),
                run_id,
                EventKind::new(EventKind::RUN_FINISHED),
                Actor::System,
                now,
                serde_json::json!({ "outcome": "failed", "reason": error }),
            ));
            return;
        }
    };
    let store = state.store();
    let final_path = final_path.map(|path| path.display().to_string());
    let _ = store.update_artifact(
        run_id,
        artifact_status,
        final_path.as_deref(),
        None,
        Some(now),
    );
    let _ = store.transition_task(task_id, task_state, Actor::System, reason, now);
    let _ = store.upsert_run(&RunRow {
        run_id,
        task_id,
        cell_id: CellId::new("zero"),
        runner: RUNNER.into(),
        model: "local".into(),
        outcome: Some(run_outcome.into()),
        usd_micros: 0,
        tokens: 0,
        operator_touched: false,
        started_ts,
        finished_ts: Some(now),
    });
    let _ = store.append(&farseer_core::NewEvent::new(
        CellId::new("zero"),
        run_id,
        EventKind::new(EventKind::RUN_FINISHED),
        Actor::System,
        now,
        serde_json::json!({ "outcome": run_outcome, "reason": reason, "artifact": KIND }),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_sorted_and_repeatable_for_identical_bytes() {
        let fixture = tempfile::tempdir().unwrap();
        fs::create_dir(fixture.path().join("nested")).unwrap();
        fs::write(fixture.path().join("z.txt"), b"same").unwrap();
        fs::write(fixture.path().join("nested").join("a.txt"), b"bytes").unwrap();
        let cancelled = AtomicBool::new(false);
        let first = files_under(fixture.path(), &cancelled).unwrap();
        let second = files_under(fixture.path(), &cancelled).unwrap();
        assert_eq!(first, second);
        let entries = first
            .iter()
            .map(|path| manifest_entry(fixture.path(), path).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(entries[0].path, "nested/a.txt");
        assert_eq!(entries[1].path, "z.txt");
        assert_eq!(entries[0].size, 5);
        assert_eq!(
            entries[0].sha256,
            "277089d91c0bdf4f2e6862ba7e4a07605119431f5d13f726dd352b06f1b206a9"
        );
        assert_eq!(
            entries,
            first
                .iter()
                .map(|path| manifest_entry(fixture.path(), path).unwrap())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn cancellation_stops_before_reading_the_fixture() {
        let fixture = tempfile::tempdir().unwrap();
        fs::write(fixture.path().join("input.txt"), b"bytes").unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(files_under(fixture.path(), &cancelled).unwrap().is_empty());
    }

    #[test]
    fn a_path_outside_the_input_directory_is_refused() {
        let input = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        assert!(manifest_entry(input.path(), outside.path()).is_err());
    }
}
