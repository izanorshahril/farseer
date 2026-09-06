//! File-backed team selection for an authorized project.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

use farseer_core::{CellDefinition, CellId};

use crate::{ApiError, ApiResult, AppState};

/// The profile is deliberately inside the project and versionable with it.
pub const PROFILE_PATH: &str = ".farseer/profile.toml";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectProfile {
    #[serde(default = "profile_version")]
    pub version: u32,
    #[serde(alias = "path", alias = "project")]
    pub project_path: String,
    #[serde(alias = "cell", alias = "cell_id")]
    pub coordinating_cell: String,
    #[serde(default)]
    pub specialist_cells: Vec<String>,
}

fn profile_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileProjection {
    pub path: String,
    pub source: &'static str,
    pub valid: bool,
    pub version: u32,
    pub project_path: String,
    pub coordinating_cell: String,
    pub specialist_cells: Vec<String>,
    pub cell: Option<CellDefinition>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectQuery {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct ReloadBody {
    pub path: String,
}

/// The default profile remains cell zero, so existing projects do not need a
/// file and existing top-manager ingress keeps its behavior.
pub(crate) fn effective(
    state: &AppState,
    project: &Path,
) -> ApiResult<(ProjectProfile, Option<PathBuf>)> {
    let profile_path = project.join(PROFILE_PATH);
    let text = match std::fs::read_to_string(&profile_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return default_profile(state, project);
        }
        Err(error) => {
            return Err(ApiError::Policy(format!(
                "cannot read project profile at {}: {error}; repair or remove {PROFILE_PATH}",
                profile_path.display()
            )));
        }
    };
    let profile: ProjectProfile = toml::from_str(&text).map_err(|error| {
        ApiError::Policy(format!(
            "invalid project profile at {}: {error}; repair or remove {PROFILE_PATH}",
            profile_path.display()
        ))
    })?;
    validate(state, project, &profile)?;
    Ok((profile, Some(profile_path)))
}

fn default_profile(
    state: &AppState,
    project: &Path,
) -> ApiResult<(ProjectProfile, Option<PathBuf>)> {
    let profile = ProjectProfile {
        version: 1,
        project_path: crate::projects::display(project),
        coordinating_cell: "zero".into(),
        specialist_cells: Vec::new(),
    };
    validate(state, project, &profile)?;
    Ok((profile, None))
}

fn validate(state: &AppState, project: &Path, profile: &ProjectProfile) -> ApiResult<()> {
    if profile.version != 1 {
        return Err(ApiError::Policy(format!(
            "unsupported project profile version {}; use version = 1",
            profile.version
        )));
    }
    let declared = std::fs::canonicalize(&profile.project_path).map_err(|_| {
        ApiError::Policy(format!(
            "project profile path `{}` is not a directory; repair {PROFILE_PATH}",
            profile.project_path
        ))
    })?;
    if declared != project {
        return Err(ApiError::Policy(format!(
            "project profile belongs to `{}`, not `{}`; repair or remove {PROFILE_PATH}",
            crate::projects::display(&declared),
            crate::projects::display(project)
        )));
    }
    let cells = state.cells();
    let cell = cells.get(&CellId::new(profile.coordinating_cell.clone())).ok_or_else(|| {
        ApiError::Policy(format!(
            "project profile references unavailable coordinating cell `{}`; repair {PROFILE_PATH}",
            profile.coordinating_cell
        ))
    })?;
    for specialist in &profile.specialist_cells {
        if !cells.contains_key(&CellId::new(specialist.clone())) {
            return Err(ApiError::Policy(format!(
                "project profile references unavailable specialist cell `{specialist}`; repair {PROFILE_PATH}"
            )));
        }
    }
    crate::lifecycle::ensure_accepts_work(state, &cell.cell_id)
}

pub(crate) fn projection(state: &AppState, project: &Path) -> ProfileProjection {
    let path = project.join(PROFILE_PATH);
    match effective(state, project) {
        Ok((profile, source)) => ProfileProjection {
            path: path.display().to_string(),
            source: if source.is_some() { "file" } else { "default" },
            valid: true,
            version: profile.version,
            project_path: profile.project_path,
            coordinating_cell: profile.coordinating_cell.clone(),
            specialist_cells: profile.specialist_cells,
            cell: state
                .cells()
                .get(&CellId::new(profile.coordinating_cell))
                .cloned(),
            error: None,
        },
        Err(error) => ProfileProjection {
            path: path.display().to_string(),
            source: "file",
            valid: false,
            version: 0,
            project_path: crate::projects::display(project),
            coordinating_cell: String::new(),
            specialist_cells: Vec::new(),
            cell: None,
            error: Some(error.to_string()),
        },
    }
}

pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Json<ProfileProjection>> {
    let project = crate::projects::resolve(&state, &query.path)?;
    Ok(Json(projection(&state, &project)))
}

pub(crate) async fn reload(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ReloadBody>,
) -> ApiResult<Json<ProfileProjection>> {
    let project = crate::projects::resolve(&state, &body.path)?;
    Ok(Json(projection(&state, &project)))
}

/// The prior selected profile is only provenance. Runs keep their sealed cell
/// and profile choice, so changing the file affects future tasks only.
pub(crate) fn prior_profile(state: &AppState, project: &str) -> Option<String> {
    state
        .store()
        .scan(0, 5_000, &farseer_store::ScanFilter::default())
        .ok()?
        .into_iter()
        .rev()
        .find_map(|event| {
            (event.kind.as_str() == farseer_core::EventKind::OPERATOR_CONTEXT
                && event.payload.get("project")?.as_str()? == project)
                .then(|| {
                    event
                        .payload
                        .get("project_profile")
                        .and_then(|value| value.get("new"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .flatten()
        })
}
