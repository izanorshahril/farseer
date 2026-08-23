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

/// A child process under a Job Object it cannot escape, with piped, line-
/// buffered I/O.
pub struct SupervisedProcess {
    child: Child,
    job: HANDLE,
    stdout: BufReader<ChildStdout>,
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
        Ok(Self {
            child,
            job,
            stdout: BufReader::new(stdout),
        })
    }

    pub fn stdin(&mut self) -> &mut ChildStdin {
        self.child.stdin.as_mut().expect("stdin was piped")
    }

    pub fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let stdin = self.stdin();
        writeln!(stdin, "{line}")?;
        stdin.flush()
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
    /// value; spelled out because "cancel a run" should read as an action,
    /// not an implicit side effect of scope exit.
    pub fn kill(self) {
        drop(self);
    }
}

impl Drop for SupervisedProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.job);
        }
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
}
