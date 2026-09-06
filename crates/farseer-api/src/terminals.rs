//! HTTP adapter for optional operator shell sessions.
//!
//! `15 terminal profiles` defines the profile, workspace authority and
//! reconnect/end contract exposed here.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use farseer_runner::terminal::{TerminalProfile, TerminalSize, TerminalSnapshot, TerminalSpec};
use serde::{Deserialize, Serialize};

use crate::{ApiError, ApiResult, AppState};

#[derive(Debug, Serialize)]
pub struct ProfileView {
    pub profile: TerminalProfile,
    pub available: bool,
}

#[derive(Debug, Deserialize)]
pub struct OpenBody {
    pub profile: TerminalProfile,
    pub cwd: String,
    pub owner: String,
    #[serde(default = "default_columns")]
    pub columns: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
}

#[derive(Debug, Deserialize)]
pub struct InputBody {
    pub input: String,
}

#[derive(Debug, Deserialize)]
pub struct ResizeBody {
    pub columns: u16,
    pub rows: u16,
}

fn default_columns() -> u16 {
    120
}

fn default_rows() -> u16 {
    40
}

pub(crate) async fn profiles() -> Json<Vec<ProfileView>> {
    Json(
        [
            TerminalProfile::PowerShell,
            TerminalProfile::Cmd,
            TerminalProfile::GitBash,
        ]
        .into_iter()
        .map(|profile| ProfileView {
            available: profile.resolve().is_ok(),
            profile,
        })
        .collect(),
    )
}

pub(crate) async fn open(
    State(state): State<Arc<AppState>>,
    Json(body): Json<OpenBody>,
) -> ApiResult<(StatusCode, Json<TerminalSnapshot>)> {
    let cwd = authorized_cwd(&state, &body.cwd)?;
    let mut spec = TerminalSpec::new(body.profile, cwd, body.owner);
    spec.columns = body.columns;
    spec.rows = body.rows;
    let session = state.terminals().open(spec).map_err(ApiError::Terminal)?;
    Ok((StatusCode::CREATED, Json(session.snapshot())))
}

pub(crate) async fn read(
    State(state): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<TerminalSnapshot>> {
    let session = state
        .terminals()
        .reconnect(&id)
        .map_err(ApiError::Terminal)?;
    Ok(Json(session.snapshot()))
}

pub(crate) async fn input(
    State(state): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<InputBody>,
) -> ApiResult<StatusCode> {
    let session = state
        .terminals()
        .reconnect(&id)
        .map_err(ApiError::Terminal)?;
    session
        .input(body.input.as_bytes())
        .map_err(ApiError::Terminal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn resize(
    State(state): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<ResizeBody>,
) -> ApiResult<Json<TerminalSize>> {
    let session = state
        .terminals()
        .reconnect(&id)
        .map_err(ApiError::Terminal)?;
    let size = session
        .resize(body.columns, body.rows)
        .map_err(ApiError::Terminal)?;
    Ok(Json(size))
}

pub(crate) async fn end(
    State(state): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<StatusCode> {
    state.terminals().end(&id).map_err(ApiError::Terminal)?;
    Ok(StatusCode::NO_CONTENT)
}

fn authorized_cwd(state: &AppState, requested: &str) -> ApiResult<std::path::PathBuf> {
    if let Ok(project) = crate::projects::resolve(state, requested) {
        return Ok(project);
    }
    let cwd = std::fs::canonicalize(requested)
        .map_err(|_| ApiError::Forbidden("that terminal workspace is not authorized"))?;
    if !cwd.is_dir() {
        return Err(ApiError::BadRequest(
            "terminal workspace is not a directory",
        ));
    }
    let runs_dir = state.runs_dir();
    let is_active_run_workspace = state
        .active_run_ids()
        .into_iter()
        .map(|id| runs_dir.join(id))
        .any(|run| cwd.starts_with(run));
    if is_active_run_workspace {
        Ok(cwd)
    } else {
        Err(ApiError::Forbidden(
            "that terminal workspace is not authorized",
        ))
    }
}
