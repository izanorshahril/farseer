//! A runner's child process, reaped as one tree.
//!
//! `jobspike` (`.scratch/farseer/spikes/jobspike`) proved a Win32 Job Object
//! with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` reaps a five-deep process tree in
//! 300-400us on handle close with zero survivors, against five of six
//! surviving a root-only `TerminateProcess`. This is that mechanism, wired to
//! a piped child so its stdout can be read line by line and fed to
//! [`crate::claude_code::parse_line`].
//!
//! **The spike's ordering, with `std`'s pipes kept.** `03 spike job objects`
//! requires `CREATE_SUSPENDED`, then `AssignProcessToJobObject`, then resume,
//! and names any other ordering a race that fails rarely and unreproducibly.
//! This once assigned the job as the first statement after `spawn()` instead,
//! because `std::process::Command` exposes no primary-thread handle to resume
//! and hand-rolling `CreateProcessW` would have meant reimplementing pipe and
//! handle-inheritance machinery that `std` already has tested.
//!
//! Both halves are available at once. The creation flag is `std`'s to pass, and
//! the thread to resume is found by enumerating the process's threads: a
//! process created suspended has **exactly one**, so the first thread owned by
//! that pid is its primary thread and there is nothing to disambiguate. No
//! pipes are re-implemented and no window is left open.

use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};
use windows::core::PCWSTR;

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    /// **Names the executable and the arguments, not just the failure.**
    ///
    /// Windows resolves most Node-installed runners to a `.CMD` shim, and Rust
    /// refuses to spawn a batch file with an argument it cannot safely quote -
    /// answering `batch file arguments are invalid` and nothing else. That
    /// message says a run failed and gives an operator no way at all to find
    /// out which argument did it, which is how one broken flag looks identical
    /// to a broken runner.
    #[error("spawning `{exe}` failed: {source}
  args: {}", args.join(" "))]
    Io {
        exe: String,
        args: Vec<String>,
        source: std::io::Error,
    },
    #[error("job object setup failed: {0}")]
    Job(#[from] windows::core::Error),
}

/// A raw job handle, stored as `isize` rather than the `windows` crate's own
/// `HANDLE` because that type is a bare pointer and is not `Send`. The value
/// is opaque to the OS either way; only the bit pattern needs to cross a
/// thread boundary.
struct RawJobHandle(isize);

/// A cloneable handle that can close the job's handle - and so kill the
/// whole tree, per `jobspike` - from any thread, without needing ownership
/// of the process's pipes. Exists because [`SupervisedProcess::run`]-style
/// callers block on `read_line` on one thread; cancellation has to be able
/// to reach in from another.
///
/// The handle lives behind a mutex-guarded `Option` shared with
/// [`SupervisedProcess`] itself, so **closing happens at most once**: taking
/// the value out and closing it is one atomic step, so a token cloned twice,
/// or raced against the process's own `Drop`, cannot double-close a handle
/// and risk closing a since-reused one instead.
///
/// The `AtomicBool` alongside it is a second, independent piece of shared
/// state: **whether a cancel was ever requested**, queryable through any
/// clone regardless of which one called `cancel()`. `05 run state model`: `cancelled` must
/// never read as `failed`, and the only way to tell the two apart once the
/// process is gone and its own terminal result never arrived is to have
/// asked, in band, whether ending it was deliberate.
#[derive(Clone)]
pub struct CancelToken(Arc<Mutex<Option<RawJobHandle>>>, Arc<AtomicBool>);

impl CancelToken {
    /// Idempotent: closing an empty slot - already cancelled, or the process
    /// already finished and dropped - is a no-op, not an error.
    pub fn cancel(&self) {
        self.1.store(true, Ordering::SeqCst);
        close(&self.0);
    }

    /// Whether `cancel()` was ever called on this token or any clone of it -
    /// true forever after, even once the job handle it also closed is long
    /// gone.
    pub fn was_cancelled(&self) -> bool {
        self.1.load(Ordering::SeqCst)
    }
}

fn close(job: &Mutex<Option<RawJobHandle>>) {
    let taken = job.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(RawJobHandle(raw)) = taken {
        unsafe {
            let _ = CloseHandle(HANDLE(raw as *mut _));
        }
    }
}

/// A cloneable handle to a child's stdin, independent of [`SupervisedProcess`]
/// itself. Exists because `drive` holds `&mut SupervisedProcess` exclusively
/// for the whole blocking read loop on one thread; a later write - a steer
/// message arriving from an HTTP handler on another thread - needs its own
/// handle, fetched *before* that loop starts, same as [`CancelToken`].
#[derive(Clone)]
pub struct StdinHandle(Arc<Mutex<ChildStdin>>);

impl StdinHandle {
    pub fn write_line(&self, line: &str) -> std::io::Result<()> {
        let mut stdin = self.0.lock().unwrap_or_else(|e| e.into_inner());
        writeln!(stdin, "{line}")?;
        stdin.flush()
    }
}

/// A child process under a Job Object it cannot escape, with piped, line-
/// buffered I/O.
pub struct SupervisedProcess {
    // Kept alive for its pid (tests only, via `child.id()`) and so its
    // process handle stays open for the struct's lifetime; stdout/stdin are
    // taken out of it at construction and read through their own fields.
    #[allow(dead_code)]
    child: Child,
    job: Arc<Mutex<Option<RawJobHandle>>>,
    cancelled: Arc<AtomicBool>,
    stdout: BufReader<ChildStdout>,
    /// `None` for a runner nobody is going to steer.
    stdin: Option<Arc<Mutex<ChildStdin>>>,
}

/// Whether the child gets a stdin at all.
///
/// **A one-shot runner must be given a closed stdin, not an open pipe nobody
/// writes to.** Observed 2026-08-25: `codex exec` prints "Reading additional
/// input from stdin..." and waits for EOF before it starts work, so a pipe held
/// open for the process's lifetime means it never begins - a live process, zero
/// output, and a run that looks hung forever.
///
/// `20 worker control channel` made steering the exception rather than the rule,
/// and this is that rule expressed at the spawn: a pipe exists only where
/// something is going to write to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdinMode {
    /// A live session that will receive the goal, and later steer messages.
    Live,
    /// Closed at spawn. The runner sees EOF immediately and gets on with it.
    Closed,
}

impl SupervisedProcess {
    /// `exe` must already be resolved - see [`crate::resolve`] - since
    /// `CreateProcessW` does not consult `PATHEXT` and neither does
    /// `std::process::Command`.
    ///
    /// `env` is added to the inherited environment rather than replacing it.
    /// It exists because a credential belongs there and nowhere else: not on
    /// the argv, which every process listing on this machine can read, and not
    /// in the prompt, where the model itself would learn it. `31 manager
    /// delegation reach` needed a delegation token to reach a pi extension;
    /// Codex's `bearer_token_env_var` will want the same channel.
    pub fn spawn(
        exe: &Path,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
        stdin_mode: StdinMode,
    ) -> Result<Self, SpawnError> {
        let mut child = Command::new(exe)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .current_dir(cwd)
            .stdin(match stdin_mode {
                StdinMode::Live => Stdio::piped(),
                StdinMode::Closed => Stdio::null(),
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // Suspended, so nothing this process does can precede its
            // assignment to the job below. Resumed by [`resume_primary_thread`]
            // once that assignment has succeeded.
            .creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0)
            .spawn()
            .map_err(|source| SpawnError::Io {
                exe: exe.display().to_string(),
                args: args.to_vec(),
                source,
            })?;

        let job = unsafe { CreateJobObjectW(None, PCWSTR::null())? };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let result: windows::core::Result<()> = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
            .and_then(|_| AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())))
        };
        if let Err(e) = result {
            unsafe { CloseHandle(job).ok() };
            let _ = child.kill();
            return Err(e.into());
        }

        // Only now. Everything the child spawns from here is inside the job,
        // which is the whole point of the ordering.
        //
        // A failure to resume leaves a process that will never run, so it is
        // killed rather than returned: the job is already assigned, so the kill
        // reaps whatever the suspended process would have become.
        if let Err(e) = Self::resume_primary_thread(child.id()) {
            unsafe { CloseHandle(job).ok() };
            let _ = child.kill();
            return Err(e.into());
        }

        let stdout = child.stdout.take().expect("stdout was piped");
        let stdin = child.stdin.take().map(|stdin| Arc::new(Mutex::new(stdin)));
        Ok(Self {
            child,
            job: Arc::new(Mutex::new(Some(RawJobHandle(job.0 as isize)))),
            cancelled: Arc::new(AtomicBool::new(false)),
            stdout: BufReader::new(stdout),
            stdin,
        })
    }

    /// Start a process that `CREATE_SUSPENDED` left frozen.
    ///
    /// `std` hands out no thread handle, so the thread is found by enumerating
    /// the system's threads and taking the first one this pid owns. That is not
    /// a guess: a process created suspended has exactly one thread until its
    /// own code runs, and its own code has not run.
    ///
    /// The snapshot is system-wide because Windows offers no per-process thread
    /// enumeration; the filter is `th32OwnerProcessID`, which the snapshot
    /// carries on every entry.
    fn resume_primary_thread(pid: u32) -> windows::core::Result<()> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)? };
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut found = None;
        unsafe {
            if Thread32First(snapshot, &mut entry).is_ok() {
                loop {
                    if entry.th32OwnerProcessID == pid {
                        found = Some(entry.th32ThreadID);
                        break;
                    }
                    if Thread32Next(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            CloseHandle(snapshot).ok();
        }
        let Some(thread_id) = found else {
            // The process has no thread, which for one that has never run means
            // it is already gone.
            return Err(windows::core::Error::from_thread());
        };
        unsafe {
            let thread = OpenThread(THREAD_SUSPEND_RESUME, false, thread_id)?;
            // `u32::MAX` is the documented failure value, and it is the only
            // one worth acting on: any real count means the thread was resumed.
            let previous = ResumeThread(thread);
            CloseHandle(thread).ok();
            if previous == u32::MAX {
                return Err(windows::core::Error::from_thread());
            }
        }
        Ok(())
    }

    /// A handle that can kill this process's whole tree from another thread.
    /// Fetch it before calling a blocking read - the token has to exist
    /// before you might need it.
    pub fn cancel_token(&self) -> CancelToken {
        CancelToken(Arc::clone(&self.job), Arc::clone(&self.cancelled))
    }

    /// A handle that can write to this process's stdin from another thread.
    /// Fetch it before calling a blocking read, same reason as
    /// [`Self::cancel_token`].
    pub fn stdin_handle(&self) -> Option<StdinHandle> {
        self.stdin
            .as_ref()
            .map(|stdin| StdinHandle(Arc::clone(stdin)))
    }

    /// Write to a live session's stdin.
    ///
    /// Writing to a runner spawned with [`StdinMode::Closed`] is a programming
    /// error rather than a runtime condition - nothing should be steering a
    /// one-shot - so it says so instead of silently succeeding.
    pub fn write_line(&self, line: &str) -> std::io::Result<()> {
        match self.stdin_handle() {
            Some(handle) => handle.write_line(line),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "this runner was spawned with no stdin, so nothing can be written to it",
            )),
        }
    }

    /// The next line, sans terminator. `Ok(None)` is end of stream - the
    /// child closed stdout, which normally means it exited.
    pub fn read_line(&mut self) -> std::io::Result<Option<String>> {
        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }

    /// Kill-on-close: closing the job handle is a kernel guarantee that
    /// every process still assigned to it dies, transitively, no matter how
    /// deep the tree has grown since spawn. Equivalent to dropping this
    /// value or calling [`CancelToken::cancel`]; spelled out because "cancel
    /// a run" should read as an action, not an implicit side effect of scope
    /// exit.
    pub fn kill(self) {
        drop(self);
    }
}

impl Drop for SupervisedProcess {
    fn drop(&mut self) {
        close(&self.job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::STILL_ACTIVE;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn alive(pid: u32) -> bool {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code).is_ok();
            let _ = CloseHandle(h);
            ok && code == STILL_ACTIVE.0 as u32
        }
    }

    /// A grandchild spawned by the child is inside the job, and dies with it.
    ///
    /// `03 spike job objects` reproduced the negative case deliberately: killing
    /// the root alone left five of six processes alive. This is the positive
    /// one, at the depth that matters - the child spawns its own process
    /// immediately, before farseer has read a single line from it.
    ///
    /// What this cannot do is reproduce the race the suspended-spawn ordering
    /// closes. That race is two syscalls wide on one thread; a test that waited
    /// for it would be the flaky test the ticket warns about. The ordering is
    /// verified by construction - the child cannot run before assignment
    /// because it is not running - and this proves the reaping it protects.
    #[test]
    fn a_grandchild_spawned_immediately_is_reaped_with_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("grandchild-was-alive.txt");
        // `start /b` detaches a second `cmd` that outlives its parent's own
        // exit - the shape that survives a root-only kill.
        let script = format!(
            "start /b cmd /c \"timeout /t 4 /nobreak >nul & echo alive > {}\" & echo spawned & timeout /t 20 /nobreak >nul",
            marker.display()
        );
        let mut proc = SupervisedProcess::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &cmd(&["/c", &script]),
            dir.path(),
            &[],
            StdinMode::Closed,
        )
        .unwrap();
        assert_eq!(
            // `cmd`'s `echo x & ...` keeps the space before the separator.
            proc.read_line().unwrap().as_deref().map(str::trim_end),
            Some("spawned"),
            "the child ran, so it was resumed after assignment"
        );

        proc.cancel_token().cancel();
        // Past when the grandchild would have written, had it lived.
        std::thread::sleep(std::time::Duration::from_secs(6));
        assert!(
            !marker.exists(),
            "the grandchild outlived the job it was spawned inside"
        );
    }

    fn cmd(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn stdout_lines_from_a_real_child_are_read_back_in_order() {
        let mut proc = SupervisedProcess::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &cmd(&["/c", "echo one&echo two"]),
            &std::env::current_dir().unwrap(),
            &[],
            StdinMode::Live,
        )
        .unwrap();

        assert_eq!(proc.read_line().unwrap().as_deref(), Some("one"));
        assert_eq!(proc.read_line().unwrap().as_deref(), Some("two"));
        assert_eq!(
            proc.read_line().unwrap(),
            None,
            "the child exited, so stdout is at EOF"
        );
    }

    #[test]
    fn closing_the_job_kills_a_still_running_child() {
        // `ping` ignores redirected stdin, unlike `timeout`, which refuses to
        // run at all when its input is not a console.
        let proc = SupervisedProcess::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &cmd(&["/c", "ping -n 30 127.0.0.1 >nul"]),
            &std::env::current_dir().unwrap(),
            &[],
            StdinMode::Live,
        )
        .unwrap();
        let pid = proc.child.id();
        assert!(alive(pid), "the child should still be running before kill");

        proc.kill();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while alive(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!alive(pid), "closing the job should have reaped the child");
    }

    #[test]
    fn a_cancel_token_kills_the_process_from_another_thread_while_the_owner_blocks_on_read() {
        let mut proc = SupervisedProcess::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &cmd(&["/c", "ping -n 30 127.0.0.1 >nul"]),
            &std::env::current_dir().unwrap(),
            &[],
            StdinMode::Live,
        )
        .unwrap();
        let pid = proc.child.id();
        let token = proc.cancel_token();

        let canceller = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            token.cancel();
        });

        // Blocks until EOF, which only arrives once the job is closed and the
        // child is gone - proving the token reached across the thread.
        assert_eq!(proc.read_line().unwrap(), None);
        canceller.join().unwrap();

        assert!(
            !alive(pid),
            "the token's cancel should have reaped the child"
        );
    }

    #[test]
    fn cancelling_twice_and_then_dropping_does_not_double_close() {
        let proc = SupervisedProcess::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &cmd(&["/c", "echo hi"]),
            &std::env::current_dir().unwrap(),
            &[],
            StdinMode::Live,
        )
        .unwrap();
        let token = proc.cancel_token();
        token.cancel();
        token.cancel();
        drop(proc);
    }

    #[test]
    fn was_cancelled_is_true_through_any_clone_once_any_clone_cancels() {
        let proc = SupervisedProcess::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &cmd(&["/c", "echo hi"]),
            &std::env::current_dir().unwrap(),
            &[],
            StdinMode::Live,
        )
        .unwrap();
        let a = proc.cancel_token();
        let b = a.clone();
        assert!(!a.was_cancelled());
        assert!(!b.was_cancelled());

        b.cancel();

        assert!(a.was_cancelled(), "every clone shares the same flag");
        // A fresh token, fetched after the fact, must also see it - this is
        // exactly the query `farseer-manager` makes after the process has
        // already exited.
        assert!(proc.cancel_token().was_cancelled());
    }
}
