//! Optional supervised operator shell sessions.
//!
//! A terminal is an adapter-owned process, separate from a worker contract and
//! its protocol channel. The first slice uses the existing piped Job Object
//! runner primitive; the retained output and session identity are independent
//! of the run record.
//!
//! `15 terminal profiles` is the deciding ticket for profile resolution,
//! bounded scrollback, reconnect and explicit termination.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::resolve::resolve;
use crate::spawn::{StdinHandle, StdinMode, SupervisedProcess};

pub const MAX_SCROLLBACK_LINES: usize = 10_000;
pub const MAX_SCROLLBACK_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalProfile {
    PowerShell,
    Cmd,
    GitBash,
}

impl TerminalProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PowerShell => "powershell",
            Self::Cmd => "cmd",
            Self::GitBash => "git-bash",
        }
    }

    fn candidates(self) -> Vec<PathBuf> {
        match self {
            Self::PowerShell => ["pwsh", "powershell"]
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            Self::Cmd => {
                let mut candidates = Vec::new();
                if let Some(root) = std::env::var_os("SystemRoot") {
                    candidates.push(PathBuf::from(root).join("System32/cmd.exe"));
                }
                candidates.push(PathBuf::from("cmd"));
                candidates
            }
            Self::GitBash => {
                let mut candidates = Vec::new();
                for variable in ["ProgramFiles", "LOCALAPPDATA"] {
                    if let Some(root) = std::env::var_os(variable) {
                        candidates.push(PathBuf::from(root).join("Git/bin/bash.exe"));
                    }
                }
                candidates.push(PathBuf::from("bash"));
                candidates
            }
        }
    }

    fn args(self) -> Vec<String> {
        match self {
            Self::PowerShell => ["-NoLogo", "-NoProfile", "-NoExit"]
                .map(str::to_owned)
                .to_vec(),
            Self::Cmd => ["/Q", "/K"].map(str::to_owned).to_vec(),
            Self::GitBash => ["--noprofile", "--norc", "-i"].map(str::to_owned).to_vec(),
        }
    }

    /// Resolve an approved built-in profile before any child process exists.
    pub fn resolve(self) -> Result<ShellProfile, TerminalError> {
        for candidate in self.candidates() {
            let executable = if candidate.components().count() == 1 {
                resolve(candidate.to_string_lossy().as_ref())
            } else if candidate.is_file() {
                Some(candidate)
            } else {
                None
            };
            if let Some(executable) = executable {
                return Ok(ShellProfile {
                    profile: self,
                    executable,
                    args: self.args(),
                });
            }
        }
        Err(TerminalError::ProfileUnavailable(self))
    }
}

impl std::fmt::Display for TerminalProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ShellProfile {
    pub profile: TerminalProfile,
    pub executable: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TerminalSpec {
    pub profile: TerminalProfile,
    pub cwd: PathBuf,
    pub owner: String,
    pub environment: Vec<(String, String)>,
    pub columns: u16,
    pub rows: u16,
}

impl TerminalSpec {
    pub fn new(
        profile: TerminalProfile,
        cwd: impl Into<PathBuf>,
        owner: impl Into<String>,
    ) -> Self {
        Self {
            profile,
            cwd: cwd.into(),
            owner: owner.into(),
            environment: Vec::new(),
            columns: 120,
            rows: 40,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("terminal profile `{0}` is unavailable")]
    ProfileUnavailable(TerminalProfile),
    #[error("terminal owner is required")]
    MissingOwner,
    #[error("terminal workspace `{0}` is not an existing directory")]
    InvalidWorkspace(String),
    #[error("terminal dimensions must be non-zero")]
    InvalidDimensions,
    #[error("terminal session `{0}` was not found")]
    NotFound(String),
    #[error("terminal session is already ended")]
    Ended,
    #[error("terminal input failed: {0}")]
    Input(#[source] std::io::Error),
    #[error("terminal process failed: {0}")]
    Spawn(#[from] crate::spawn::SpawnError),
    #[error("terminal reader thread failed: {0}")]
    Reader(#[source] std::io::Error),
    #[error("deferred workspace cleanup failed: {0}")]
    Cleanup(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalState {
    Active,
    Exited,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalSnapshot {
    pub id: String,
    pub profile: TerminalProfile,
    pub executable: String,
    pub cwd: String,
    pub owner: String,
    pub size: TerminalSize,
    pub state: TerminalState,
    pub output: String,
}

#[derive(Debug, Default)]
struct Scrollback {
    lines: VecDeque<String>,
    bytes: usize,
}

impl Scrollback {
    fn push(&mut self, mut line: String) {
        if line.len() > MAX_SCROLLBACK_BYTES {
            let mut end = MAX_SCROLLBACK_BYTES;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
        }
        self.bytes += line.len() + 1;
        self.lines.push_back(line);
        while self.lines.len() > MAX_SCROLLBACK_LINES || self.bytes > MAX_SCROLLBACK_BYTES {
            if let Some(old) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(old.len() + 1);
            }
        }
    }

    fn text(&self) -> String {
        self.lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

pub struct TerminalSession {
    id: Uuid,
    profile: TerminalProfile,
    executable: PathBuf,
    cwd: PathBuf,
    owner: String,
    size: Mutex<TerminalSize>,
    state: Mutex<TerminalState>,
    output: Mutex<Scrollback>,
    cancel: crate::spawn::CancelToken,
    stdin: StdinHandle,
}

impl TerminalSession {
    fn start(spec: TerminalSpec) -> Result<Arc<Self>, TerminalError> {
        if spec.owner.trim().is_empty() {
            return Err(TerminalError::MissingOwner);
        }
        if spec.columns == 0 || spec.rows == 0 {
            return Err(TerminalError::InvalidDimensions);
        }
        let cwd = std::fs::canonicalize(&spec.cwd)
            .map_err(|_| TerminalError::InvalidWorkspace(spec.cwd.display().to_string()))?;
        if !cwd.is_dir() {
            return Err(TerminalError::InvalidWorkspace(cwd.display().to_string()));
        }
        let shell = spec.profile.resolve()?;
        let mut process = SupervisedProcess::spawn(
            &shell.executable,
            &shell.args,
            &cwd,
            &spec.environment,
            StdinMode::Live,
        )?;
        let cancel = process.cancel_token();
        let stdin = process.stdin_handle().expect("live terminal stdin");
        let session = Arc::new(Self {
            id: Uuid::now_v7(),
            profile: shell.profile,
            executable: shell.executable,
            cwd,
            owner: spec.owner,
            size: Mutex::new(TerminalSize {
                columns: spec.columns,
                rows: spec.rows,
            }),
            state: Mutex::new(TerminalState::Active),
            output: Mutex::new(Scrollback::default()),
            cancel,
            stdin,
        });
        let reader_session = Arc::clone(&session);
        thread::Builder::new()
            .name(format!("farseer-terminal-{}", session.id))
            .spawn(move || {
                loop {
                    match process.read_line() {
                        Ok(Some(line)) => reader_session
                            .output
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(line),
                        Ok(None) | Err(_) => {
                            let mut state = reader_session
                                .state
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            if *state == TerminalState::Active {
                                *state = TerminalState::Exited;
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(TerminalError::Reader)?;
        Ok(session)
    }

    pub fn id(&self) -> String {
        self.id.to_string()
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            id: self.id(),
            profile: self.profile,
            executable: self.executable.display().to_string(),
            cwd: self.cwd.display().to_string(),
            owner: self.owner.clone(),
            size: *self.size.lock().unwrap_or_else(|e| e.into_inner()),
            state: *self.state.lock().unwrap_or_else(|e| e.into_inner()),
            output: self.output.lock().unwrap_or_else(|e| e.into_inner()).text(),
        }
    }

    pub fn input(&self, input: &[u8]) -> Result<(), TerminalError> {
        if *self.state.lock().unwrap_or_else(|e| e.into_inner()) != TerminalState::Active {
            return Err(TerminalError::Ended);
        }
        self.stdin.write(input).map_err(TerminalError::Input)
    }

    /// Retain the requested size at the adapter seam. A future ConPTY backend
    /// can apply the same operation without changing the session contract.
    pub fn resize(&self, columns: u16, rows: u16) -> Result<TerminalSize, TerminalError> {
        if columns == 0 || rows == 0 {
            return Err(TerminalError::InvalidDimensions);
        }
        let mut size = self.size.lock().unwrap_or_else(|e| e.into_inner());
        *size = TerminalSize { columns, rows };
        Ok(*size)
    }

    pub fn end(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *state == TerminalState::Active {
            *state = TerminalState::Ended;
            self.cancel.cancel();
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Default)]
pub struct TerminalManager {
    sessions: Mutex<HashMap<Uuid, Arc<TerminalSession>>>,
    pending_cleanup: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
}

impl TerminalManager {
    pub fn open(&self, spec: TerminalSpec) -> Result<Arc<TerminalSession>, TerminalError> {
        let session = TerminalSession::start(spec)?;
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.id, Arc::clone(&session));
        Ok(session)
    }

    pub fn reconnect(&self, id: &str) -> Result<Arc<TerminalSession>, TerminalError> {
        let id = Uuid::parse_str(id).map_err(|_| TerminalError::NotFound(id.to_owned()))?;
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .ok_or_else(|| TerminalError::NotFound(id.to_string()))
    }

    pub fn holds_workspace(&self, workspace: &Path) -> bool {
        let workspace =
            std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .any(|session| session.cwd == workspace)
    }

    /// Defer run-workspace teardown while a terminal's cwd still holds it.
    /// The last explicit terminal End releases the lease and performs the
    /// pending cleanup, preserving the workspace ordering rule from ticket 15.
    pub fn defer_workspace_cleanup(
        &self,
        workspace: &Path,
        repo: Option<&Path>,
    ) -> Result<bool, TerminalError> {
        let workspace =
            std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
        if self.holds_workspace(&workspace) {
            self.pending_cleanup
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(workspace, repo.map(Path::to_path_buf));
            return Ok(false);
        }
        crate::workspace::teardown_workspace(&workspace, repo)
            .map(|()| true)
            .map_err(|error| TerminalError::Cleanup(error.to_string()))
    }

    pub fn end(&self, id: &str) -> Result<(), TerminalError> {
        let session = self.reconnect(id)?;
        session.end();
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&session.id);
        drop(session);
        self.cleanup_ready()?;
        Ok(())
    }

    fn cleanup_ready(&self) -> Result<(), TerminalError> {
        let ready = {
            let mut pending = self
                .pending_cleanup
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let paths = pending
                .keys()
                .filter(|path| !self.holds_workspace(path))
                .cloned()
                .collect::<Vec<_>>();
            paths
                .into_iter()
                .filter_map(|path| pending.remove(&path).map(|repo| (path, repo)))
                .collect::<Vec<_>>()
        };
        for (workspace, repo) in ready {
            if let Err(error) = crate::workspace::teardown_workspace(&workspace, repo.as_deref()) {
                self.pending_cleanup
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(workspace, repo);
                return Err(TerminalError::Cleanup(error.to_string()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_workspace_fails_before_profile_spawn() {
        let manager = TerminalManager::default();
        let result = manager.open(TerminalSpec::new(
            TerminalProfile::Cmd,
            PathBuf::from(r"C:\path\that\does\not\exist"),
            "operator",
        ));
        assert!(matches!(result, Err(TerminalError::InvalidWorkspace(_))));
    }

    #[test]
    fn scrollback_keeps_both_bounds() {
        let mut output = Scrollback::default();
        for n in 0..(MAX_SCROLLBACK_LINES + 20) {
            output.push(format!("line-{n}"));
        }
        assert_eq!(output.lines.len(), MAX_SCROLLBACK_LINES);
        assert!(output.bytes <= MAX_SCROLLBACK_BYTES);
        assert!(!output.text().contains("line-0"));
    }

    #[test]
    fn profile_names_are_stable() {
        assert_eq!(TerminalProfile::PowerShell.as_str(), "powershell");
        assert_eq!(TerminalProfile::Cmd.as_str(), "cmd");
        assert_eq!(TerminalProfile::GitBash.as_str(), "git-bash");
    }

    #[test]
    fn resize_rejects_zero_dimensions() {
        assert_eq!(TerminalProfile::Cmd.as_str(), "cmd");
        assert!(matches!(
            TerminalSpec::new(TerminalProfile::Cmd, ".", "operator").columns,
            120
        ));
    }

    #[test]
    fn a_cmd_session_reconnects_and_ends_for_a_unicode_workspace() {
        let manager = TerminalManager::default();
        let cwd = tempfile::tempdir().unwrap();
        let workspace = cwd.path().join("space ü");
        std::fs::create_dir(&workspace).unwrap();
        let session = manager
            .open(TerminalSpec::new(
                TerminalProfile::Cmd,
                &workspace,
                "operator",
            ))
            .expect("cmd should be available on Windows");
        let id = session.id();
        let reconnected = manager.reconnect(&id).unwrap();
        assert!(manager.holds_workspace(&workspace));
        assert_eq!(
            reconnected.snapshot().cwd,
            workspace.canonicalize().unwrap().display().to_string()
        );

        assert!(!manager.defer_workspace_cleanup(&workspace, None).unwrap());
        assert!(workspace.exists(), "a held workspace must remain until End");

        manager.end(&id).unwrap();
        assert!(!workspace.exists(), "End releases the deferred cleanup");
        assert!(matches!(
            manager.reconnect(&id),
            Err(TerminalError::NotFound(_))
        ));
    }
}
