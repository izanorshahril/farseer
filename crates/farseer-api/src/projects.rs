//! What farseer is pointed at.
//!
//! `39 what an installed farseer points at` settled the shape: farseer is an
//! application that manages several projects rather than a tool run inside one.
//! The operator names **roots** - directories farseer may work in - and a
//! **project** is any directory inside a root. Projects are never registered,
//! so the list cannot drift from a disk the operator also edits in Explorer.
//!
//! Farseer creates projects inside a root and never creates a root. Granting
//! access is the operator's act; an application that can widen its own
//! authorization has none.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{ApiError, ApiResult, AppState, now_ms};

#[derive(Debug, Serialize)]
pub struct ProjectView {
    /// The directory's own name, which is what the operator called the project.
    pub name: String,
    pub path: String,
    /// Whether it holds a `.git`. A cell whose `workspace_strategy` is
    /// `worktree` needs one, so this is the difference between a project that
    /// cell zero can run in and one it cannot - reported rather than filtered,
    /// because a directory the operator made and farseer silently hid is worse
    /// than one shown with the reason it will not do.
    pub git: bool,
}

#[derive(Debug, Serialize)]
pub struct RootView {
    pub path: String,
    /// True when the directory has gone - removed from Explorer rather than
    /// from farseer. The grant survives, because withdrawing it is the
    /// operator's decision and a disconnected drive is not one.
    pub missing: bool,
    pub projects: Vec<ProjectView>,
}

#[derive(Debug, Deserialize)]
pub struct RootBody {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct NewProjectBody {
    /// Which authorized root to create inside.
    pub root: String,
    /// One path segment. Not a path: see [`single_segment`].
    pub name: String,
    /// Run `git init` in it. `39` keeps this the caller's choice because a
    /// `worktree` cell needs a repository and a `plain_directory` cell does not.
    #[serde(default)]
    pub git: bool,
}

/// Every root, with what is currently inside it.
pub(crate) async fn list_projects(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<RootView>>> {
    let roots = state.store().roots()?;
    Ok(Json(roots.iter().map(|r| describe_root(r)).collect()))
}

/// Authorize a directory.
///
/// Canonicalized on the way in, because every later check compares canonical
/// paths and a list holding the string somebody typed cannot answer them.
pub(crate) async fn add_root(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RootBody>,
) -> ApiResult<(StatusCode, Json<RootView>)> {
    let path = canonical_dir(&body.path)?;
    let text = display(&path);
    state.store().authorize_root(&text, now_ms())?;
    Ok((StatusCode::CREATED, Json(describe_root(&text))))
}

/// Withdraw a grant. **Nothing on disk is touched.**
pub(crate) async fn remove_root(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RootBody>,
) -> ApiResult<StatusCode> {
    // Not canonicalized: a root whose directory has been deleted or unplugged
    // still has a row, and refusing to revoke it because it cannot be resolved
    // would strand the grant exactly when the operator wants it gone.
    if state.store().revoke_root(&body.path)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("root"))
    }
}

/// Create a project inside a root.
pub(crate) async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(body): Json<NewProjectBody>,
) -> ApiResult<(StatusCode, Json<ProjectView>)> {
    let name = single_segment(&body.name)?;
    let root = authorized(&state, &body.root)?;
    let dir = root.join(name);
    if dir.exists() {
        return Err(ApiError::BadRequest(
            "a directory by that name is already there",
        ));
    }
    std::fs::create_dir(&dir).map_err(|e| ApiError::Workspace(e.to_string()))?;
    if body.git {
        let status = std::process::Command::new("git")
            .arg("init")
            .arg(&dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| ApiError::Workspace(format!("git init: {e}")))?;
        if !status.success() {
            return Err(ApiError::Workspace("git init failed".into()));
        }
    }
    Ok((StatusCode::CREATED, Json(describe_project(&dir))))
}

/// Resolve a project a caller named, or refuse.
///
/// The check is "is this canonical path inside a canonical root", and it runs
/// **after** canonicalization so `..`, a symlink pointing out and a path spelled
/// differently all resolve before the comparison rather than after it. A run's
/// workspace is `git worktree add` under this directory, so a path that arrives
/// from a manager, a widget or a URL and is used unchecked is a write primitive
/// with no fence around it.
pub(crate) fn resolve(state: &AppState, path: &str) -> ApiResult<PathBuf> {
    authorized(state, path)
}

fn authorized(state: &AppState, path: &str) -> ApiResult<PathBuf> {
    let target = canonical_dir(path)?;
    let roots = state.store().roots()?;
    let inside = roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|root| is_within(&root, &target))
            .unwrap_or(false)
    });
    if inside {
        Ok(target)
    } else {
        Err(ApiError::Forbidden(
            "that directory is not inside a folder you have authorized; add it first",
        ))
    }
}

/// A root contains itself, so a root is addressable as a project.
///
/// `Path::starts_with` compares whole components, which is the reason to use it
/// over a string prefix: `D:\Dev` must not authorize `D:\Development`.
fn is_within(root: &Path, target: &Path) -> bool {
    target.starts_with(root)
}

fn canonical_dir(path: &str) -> ApiResult<PathBuf> {
    if path.trim().is_empty() {
        return Err(ApiError::BadRequest("a path is required"));
    }
    let resolved =
        std::fs::canonicalize(path).map_err(|_| ApiError::BadRequest("no directory there"))?;
    if !resolved.is_dir() {
        return Err(ApiError::BadRequest("that is a file, not a directory"));
    }
    Ok(resolved)
}

/// A project name is one path segment.
///
/// `..`, a separator and an absolute path are all refused rather than sanitized:
/// silently rewriting what the operator typed produces a directory somewhere
/// they did not ask for, and the whole point of a root is that farseer's writes
/// are inside it.
fn single_segment(name: &str) -> ApiResult<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("a name is required"));
    }
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(name),
        _ => Err(ApiError::BadRequest(
            "a project name is one folder name, not a path",
        )),
    }
}

fn describe_root(path: &str) -> RootView {
    let dir = Path::new(path);
    let mut projects = Vec::new();
    let mut missing = true;
    if let Ok(entries) = std::fs::read_dir(dir) {
        missing = false;
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let child = entry.path();
                // A dotted directory is machinery - `.git`, `.cargo`, `.venv` -
                // not a project somebody started.
                if !child
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.'))
                {
                    projects.push(describe_project(&child));
                }
            }
        }
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    RootView {
        path: path.to_string(),
        missing,
        projects,
    }
}

fn describe_project(dir: &Path) -> ProjectView {
    ProjectView {
        name: dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string(),
        path: display(dir),
        git: dir.join(".git").exists(),
    }
}

/// Canonicalization on Windows produces a `\\?\` prefix, which is correct and
/// unreadable. It is stripped for storage and display; it is a spelling of the
/// same path, and every comparison canonicalizes both sides anyway.
pub(crate) fn display(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sibling_directory_with_a_longer_name_is_not_inside_the_root() {
        // The string prefix says yes and the answer is no. This is the whole
        // reason `is_within` compares components.
        assert!(!is_within(
            Path::new(r"D:\Dev"),
            Path::new(r"D:\Development")
        ));
        assert!(is_within(
            Path::new(r"D:\Dev"),
            Path::new(r"D:\Dev\farseer")
        ));
        assert!(is_within(Path::new(r"D:\Dev"), Path::new(r"D:\Dev")));
    }

    #[test]
    fn a_project_name_is_one_folder_name() {
        assert!(single_segment("farseer").is_ok());
        assert!(single_segment("  farseer  ").is_ok());
        for bad in ["..", "a/b", r"a\b", "/etc", r"C:\x", ""] {
            assert!(single_segment(bad).is_err(), "{bad} should be refused");
        }
    }
}
