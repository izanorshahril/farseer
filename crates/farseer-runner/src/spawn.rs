//! A runner's child process, reaped as one tree.
//!
//! `jobspike` (`.scratch/farseer/spikes/jobspike`) proved a Win32 Job Object
//! with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` reaps a five-deep process tree in
//! 300-400us on handle close with zero survivors, against five of six
//! surviving a root-only `TerminateProcess`. This is that mechanism, wired to
//! a piped child so its stdout can be read line by line and fed to
//! [`crate::claude_code::parse_line`].
//!
//! **Weaker than the spike in one respect, on purpose.** `jobspike` spawns
//! suspended via raw `CreateProcessW`, assigns the job, then resumes - a
//! race-free ordering, because a process that runs even briefly before
//! assignment could spawn a child outside the job. `std::process::Command`
//! has no supported way to create a process suspended and later resume it,
//! so this assigns the job as the **first** statement after `spawn()`
//! instead. The race window that reopens is a child spawning its own
//! grandchild in the gap between two syscalls on the same thread - narrow,
//! not zero. The trade is deliberate: `std`'s pipe and handle-inheritance
//! machinery is tested; a hand-rolled reimplementation of it here would not
//! be. If that race is ever observed in practice, close it by porting
//! `jobspike`'s suspended-spawn path in rather than loosening this note.

use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::core::PCWSTR;

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("spawning the child process failed: {0}")]
    Io(#[from] std::io::Error),
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
    stdin: Arc<Mutex<ChildStdin>>,
}

impl SupervisedProcess {
    /// `exe` must already be resolved - see [`crate::resolve`] - since
    /// `CreateProcessW` does not consult `PATHEXT` and neither does
    /// `std::process::Command`.
    pub fn spawn(exe: &Path, args: &[String], cwd: &Path) -> Result<Self, SpawnError> {
        let mut child = Command::new(exe)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()?;

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

        let stdout = child.stdout.take().expect("stdout was piped");
        let stdin = child.stdin.take().expect("stdin was piped");
        Ok(Self {
            child,
            job: Arc::new(Mutex::new(Some(RawJobHandle(job.0 as isize)))),
            cancelled: Arc::new(AtomicBool::new(false)),
            stdout: BufReader::new(stdout),
            stdin: Arc::new(Mutex::new(stdin)),
        })
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
    pub fn stdin_handle(&self) -> StdinHandle {
        StdinHandle(Arc::clone(&self.stdin))
    }

    pub fn write_line(&self, line: &str) -> std::io::Result<()> {
        self.stdin_handle().write_line(line)
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

    fn cmd(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn stdout_lines_from_a_real_child_are_read_back_in_order() {
        let mut proc = SupervisedProcess::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &cmd(&["/c", "echo one&echo two"]),
            &std::env::current_dir().unwrap(),
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
