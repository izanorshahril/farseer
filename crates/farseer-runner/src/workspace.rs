//! Worktree creation and teardown, per `04 spike workspace teardown`.
//!
//! `04 spike workspace teardown`'s spike measured **zero stuck workspaces in 60 supervised cycles**
//! (p50 2.5ms, p95 38.5ms; 278ms with a real 48MB `node_modules` present),
//! against **10 of 10 stuck** when the delete ran without reaping first.
//! Every blocked delete was `ERROR_SHARING_VIOLATION` on the workspace root,
//! never a file, never Defender - caused by a live process's own current
//! directory being an open handle without `FILE_SHARE_DELETE`.
//!
//! **The hard ordering constraint this crate must not violate: teardown must
//! not begin until the process that held the workspace as its cwd has
//! exited.** That is satisfied by construction here - a caller only reaches
//! [`teardown_workspace`] after `farseer_manager::StartedWorker::run_to_completion`
//! has returned, and it returns only once the child's stdout pipe has closed,
//! which happens at or after process exit.
//!
//! **Quarantine-by-rename does not work and this module does not attempt
//! it.** `04 spike workspace teardown` forced that path and found rename fails 5 for 5, for the
//! identical reason the delete does: a directory held open as a cwd cannot
//! be renamed either. The honest ladder is two rungs - reap (already done by
//! the time this module runs), then delete with backoff - and a workspace
//! that survives the backoff is [`WorkspaceError::Stuck`], for the caller to
//! surface to the operator rather than something to keep guessing at.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_READONLY, FILE_FLAGS_AND_ATTRIBUTES, GetFileAttributesW,
    INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
};
use windows::core::PCWSTR;

/// `04 spike workspace teardown`'s measured schedule: total budget a little over 4 seconds before the
/// workspace is declared stuck.
const BACKOFF_MS: &[u64] = &[0, 10, 25, 50, 100, 200, 400, 800, 1200, 1500];

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("git {0} failed: {1}")]
    Git(&'static str, String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(
        "workspace at {path} could not be deleted after {attempts} attempts over {elapsed:?} - \
         a process still holds it open"
    )]
    Stuck {
        path: PathBuf,
        attempts: usize,
        elapsed: Duration,
    },
}

/// `git worktree add --detach` at `parent/name`. `parent` is created if
/// missing. `--detach` because a run owns no branch of its own - `13 harness build kit`
/// deliberately keeps definition versioning in plain git, outside this path.
pub fn create_worktree(repo: &Path, parent: &Path, name: &str) -> Result<PathBuf, WorkspaceError> {
    std::fs::create_dir_all(parent)?;
    let workspace = parent.join(name);
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("worktree")
        .arg("add")
        .arg("--detach")
        .arg(&workspace)
        .output()?;
    if !output.status.success() {
        return Err(WorkspaceError::Git(
            "worktree add",
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(workspace)
}

/// Deletes `workspace`, retrying with `04 spike workspace teardown`'s measured backoff. `repo` is
/// `Some` only for a worktree - `git worktree prune` runs after a successful
/// delete so the worktree stops being registered and its path can be reused;
/// a plain directory has nothing to prune.
///
/// Caller's responsibility, not this function's: reap first. See the module
/// doc comment.
pub fn teardown_workspace(workspace: &Path, repo: Option<&Path>) -> Result<(), WorkspaceError> {
    let t0 = Instant::now();
    for (attempt, wait_ms) in BACKOFF_MS.iter().enumerate() {
        if *wait_ms > 0 {
            std::thread::sleep(Duration::from_millis(*wait_ms));
        }
        if try_delete(workspace).is_ok() {
            if let Some(repo) = repo {
                // Best-effort: a failed prune leaves a stale registration,
                // which is a `git worktree list` annoyance, not a correctness
                // problem, so it does not turn a successful delete into an
                // error.
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(repo)
                    .arg("worktree")
                    .arg("prune")
                    .output();
            }
            return Ok(());
        }
        let _ = attempt;
    }
    Err(WorkspaceError::Stuck {
        path: workspace.to_path_buf(),
        attempts: BACKOFF_MS.len(),
        elapsed: t0.elapsed(),
    })
}

/// One delete attempt, depth-first. `04 spike workspace teardown`: git marks packfiles read-only, so a
/// plain recursive delete dies inside `.git` before reaching anything
/// interesting - the read-only bit is cleared on the way down, file by file.
fn try_delete(dir: &Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() && !meta.file_type().is_symlink() {
            try_delete(&path)?;
        } else {
            clear_readonly(&path);
            std::fs::remove_file(&path)?;
        }
    }
    clear_readonly(dir);
    std::fs::remove_dir(dir)
}

/// `04 spike workspace teardown`: every filesystem call that touches attributes goes through the
/// extended-length form, because `node_modules` alone can blow past
/// `MAX_PATH` from a short root. `\\?\` also disables path normalisation, so
/// the input must already be fully qualified with backslashes only - `remove_file`
/// and `remove_dir` above are left on plain paths, since `std`'s own path
/// handling already extends them where it matters; only the raw `GetFileAttributesW`/
/// `SetFileAttributesW` calls below need it done by hand.
fn extended(p: &Path) -> Vec<u16> {
    let s = p.to_string_lossy().replace('/', "\\");
    let s = if s.starts_with(r"\\?\") {
        s
    } else {
        format!(r"\\?\{s}")
    };
    OsStr::new(&s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn clear_readonly(p: &Path) {
    unsafe {
        let w = extended(p);
        let attrs = GetFileAttributesW(PCWSTR(w.as_ptr()));
        if attrs != INVALID_FILE_ATTRIBUTES && attrs & FILE_ATTRIBUTE_READONLY.0 != 0 {
            let _ = SetFileAttributesW(
                PCWSTR(w.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(attrs & !FILE_ATTRIBUTE_READONLY.0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// A repo with one commit - `worktree add` needs a valid ref to detach at.
    fn repo_with_a_commit() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        git(
            dir.path(),
            &["config", "user.email", "test@example.invalid"],
        );
        git(dir.path(), &["config", "user.name", "test"]);
        std::fs::write(dir.path().join("README.md"), "farseer test fixture\n").unwrap();
        git(dir.path(), &["add", "README.md"]);
        git(dir.path(), &["commit", "--quiet", "-m", "initial"]);
        dir
    }

    #[test]
    fn a_created_worktree_holds_the_repos_tracked_files() {
        let repo = repo_with_a_commit();
        let parent = tempfile::tempdir().unwrap();

        let workspace = create_worktree(repo.path(), parent.path(), "run-1").unwrap();

        assert!(workspace.is_dir());
        assert!(workspace.join("README.md").is_file());
        assert!(
            workspace.join(".git").exists(),
            "a worktree carries its own .git file"
        );
    }

    #[test]
    fn teardown_deletes_the_directory_and_unregisters_the_worktree() {
        let repo = repo_with_a_commit();
        let parent = tempfile::tempdir().unwrap();
        let workspace = create_worktree(repo.path(), parent.path(), "run-2").unwrap();

        teardown_workspace(&workspace, Some(repo.path())).unwrap();

        assert!(!workspace.exists());
        let list = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["worktree", "list", "--porcelain"])
            .output()
            .unwrap();
        let list = String::from_utf8_lossy(&list.stdout);
        assert!(
            !list.contains("run-2"),
            "pruned worktree should not be listed: {list}"
        );
    }

    #[test]
    fn a_plain_directory_tears_down_without_a_repo_to_prune() {
        let parent = tempfile::tempdir().unwrap();
        let workspace = parent.path().join("plain-run");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("scratch.txt"), "hi").unwrap();

        teardown_workspace(&workspace, None).unwrap();

        assert!(!workspace.exists());
    }

    #[test]
    fn a_readonly_file_does_not_block_teardown() {
        // `04 spike workspace teardown`: git marks packfiles read-only, so a plain recursive delete
        // dies inside `.git` before reaching anything interesting. Proven
        // here with a plain read-only file rather than a real packfile,
        // since a linked worktree's own `.git` is just a small text pointer
        // and carries no packs of its own - the read-only bit is the thing
        // under test, not where git happens to set it.
        let parent = tempfile::tempdir().unwrap();
        let workspace = parent.path().join("readonly-run");
        std::fs::create_dir_all(&workspace).unwrap();
        let locked = workspace.join("locked.txt");
        std::fs::write(&locked, "immutable").unwrap();
        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&locked, perms).unwrap();

        teardown_workspace(&workspace, None).unwrap();

        assert!(!workspace.exists());
    }
}
