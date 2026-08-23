//! Spike: can a git worktree workspace be created and destroyed reliably on Windows
//! while a dev server runs inside it, and what actually blocks the delete?
//!
//! Usage:
//!   wsspike cycles <n>   reap the job, then tear down. The proposed strategy.
//!   wsspike naive <n>    tear down without reaping first. The control.
//!   wsspike npm          one heavy cycle with a real node_modules present.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY, FILE_FLAGS_AND_ATTRIBUTES,
    INVALID_FILE_ATTRIBUTES,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, ResumeThread, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
};

/// Short root, per the ticket. Long paths still get the `\\?\` prefix on every
/// filesystem call, because node_modules alone can blow past MAX_PATH from here.
const ROOT: &str = r"D:\fw";

/// Backoff schedule for the supervised teardown, in milliseconds.
/// Total budget is a little over 4 seconds before quarantine.
const BACKOFF_MS: &[u64] = &[0, 10, 25, 50, 100, 200, 400, 800, 1200, 1500];

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Every filesystem call goes through the extended-length form. `\\?\` also turns off
/// path normalisation, so it must be a fully qualified path with backslashes only.
fn extended(p: &Path) -> Vec<u16> {
    let s = p.to_string_lossy().replace('/', "\\");
    if s.starts_with(r"\\?\") {
        wide(&s)
    } else {
        wide(&format!(r"\\?\{s}"))
    }
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

/// One delete attempt. Returns the first failing (path, os error code) if any.
///
/// git marks packfiles read-only, so a plain remove_dir_all fails on a worktree's
/// object store before it ever reaches a locked file. The read-only bit is cleared
/// on the way down.
fn try_delete(dir: &Path) -> Result<(), (PathBuf, i32)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err((dir.into(), e.raw_os_error().unwrap_or(-1))),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => return Err((path, e.raw_os_error().unwrap_or(-1))),
        };
        if meta.is_dir() && !meta.file_type().is_symlink() {
            try_delete(&path)?;
        } else {
            clear_readonly(&path);
            if let Err(e) = std::fs::remove_file(&path) {
                return Err((path, e.raw_os_error().unwrap_or(-1)));
            }
        }
    }
    clear_readonly(dir);
    std::fs::remove_dir(dir).map_err(|e| (dir.into(), e.raw_os_error().unwrap_or(-1)))
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Deleted,
    Quarantined,
    /// Neither deleted nor renamed. The workspace is still on disk, in place.
    Stuck,
}

#[derive(Debug)]
struct Teardown {
    attempts: usize,
    elapsed: Duration,
    outcome: Outcome,
    last_error: Option<(PathBuf, i32)>,
}

/// The proposed supervised teardown: retry with backoff, then quarantine by rename.
///
/// Quarantine is a rename, not a delete, and a rename succeeds even when the directory
/// still holds open handles - as long as nothing has the *directory itself* open.
/// That is what makes it a usable last resort rather than a second failure.
fn supervised_teardown(dir: &Path, quarantine_root: &Path) -> Teardown {
    let t0 = Instant::now();
    let mut last_error = None;
    let schedule: &[u64] = if std::env::var("WSSPIKE_NO_BACKOFF").is_ok() { &[0] } else { BACKOFF_MS };
    for (i, wait) in schedule.iter().enumerate() {
        if *wait > 0 {
            std::thread::sleep(Duration::from_millis(*wait));
        }
        match try_delete(dir) {
            Ok(()) => {
                // last_error is kept even on success: the reason the first attempt
                // failed is the whole point of the spike.
                return Teardown {
                    attempts: i + 1,
                    elapsed: t0.elapsed(),
                    outcome: Outcome::Deleted,
                    last_error,
                };
            }
            Err(e) => last_error = Some(e),
        }
    }
    let _ = std::fs::create_dir_all(quarantine_root);
    let target = quarantine_root.join(format!(
        "{}-{}",
        dir.file_name().unwrap().to_string_lossy(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let rename_err = std::fs::rename(dir, &target).err();
    Teardown {
        attempts: schedule.len(),
        elapsed: t0.elapsed(),
        outcome: match &rename_err {
            None => Outcome::Quarantined,
            Some(_) => Outcome::Stuck,
        },
        last_error,
    }
}

fn git(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git not found on PATH")
}

/// Build the origin repo once: a small tree with a package.json and a src dir.
fn ensure_origin(origin: &Path) {
    if origin.join(".git").exists() {
        return;
    }
    std::fs::create_dir_all(origin.join("src")).unwrap();
    std::fs::write(
        origin.join("package.json"),
        r#"{"name":"wsspike-fixture","version":"1.0.0","private":true,"dependencies":{"chokidar":"4.0.3"}}"#,
    )
    .unwrap();
    std::fs::write(origin.join("src/index.js"), "module.exports = 1;\n").unwrap();
    std::fs::write(origin.join(".gitignore"), "node_modules/\ndev.log\nREADY\n").unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture/server.js"),
        origin.join("server.js"),
    )
    .unwrap();
    git(&["init", "-q"], origin);
    git(&["config", "user.email", "spike@local"], origin);
    git(&["config", "user.name", "spike"], origin);
    git(&["add", "-A"], origin);
    git(&["commit", "-qm", "fixture"], origin);
}

/// Spawn the dev server inside a job object.
///
/// `workspace` is always what gets watched and written to. `cwd` is the process's
/// current directory, which is normally the workspace but is deliberately set
/// elsewhere in `cwdout` mode - a process's cwd is itself an open directory handle,
/// and that is a candidate for what blocks the delete.
fn spawn_server(workspace: &Path, cwd: &Path) -> (HANDLE, u32) {
    let node = which("node").expect("node not on PATH");
    let script = workspace.join("server.js");
    let mut cmdline = wide(&format!("\"{}\" \"{}\"", node.display(), script.display()));
    let app = wide(&node.to_string_lossy().replace('/', "\\"));
    let cwd_w = wide(&cwd.to_string_lossy().replace('/', "\\"));

    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()).unwrap() };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .unwrap();
    }

    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    let mut env_block: Vec<u16> = Vec::new();
    for (k, v) in std::env::vars() {
        env_block.extend(wide(&format!("{k}={v}")));
    }
    env_block.extend(wide(&format!("WSSPIKE_WATCH_DIR={}", workspace.display())));
    env_block.push(0);

    unsafe {
        CreateProcessW(
            PCWSTR(app.as_ptr()),
            Some(PWSTR(cmdline.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT,
            Some(env_block.as_ptr() as *const _),
            PCWSTR(cwd_w.as_ptr()),
            &si,
            &mut pi,
        )
        .unwrap();
        // Suspended, assign, resume - the ordering fixed by the job objects spike.
        AssignProcessToJobObject(job, pi.hProcess).unwrap();
        ResumeThread(pi.hThread);
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }
    (job, pi.dwProcessId)
}

/// PATHEXT-first resolution, per the job objects spike.
fn which(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect();
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        for ext in &exts {
            let c = dir.join(format!("{name}{ext}"));
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

fn wait_ready(cwd: &Path, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cwd.join("READY").exists() {
            std::thread::sleep(Duration::from_millis(300));
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn err_name(code: i32) -> &'static str {
    match code {
        5 => "ACCESS_DENIED",
        32 => "SHARING_VIOLATION",
        145 => "DIR_NOT_EMPTY",
        -1 => "unknown",
        _ => "other",
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "cycles".into());
    let n: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);

    let root = PathBuf::from(ROOT);
    let origin = root.join("origin");
    let quarantine = root.join(".quarantine");
    std::fs::create_dir_all(&root).unwrap();
    ensure_origin(&origin);

    if mode.starts_with("npm") {
        let out = Command::new(which("npm").expect("npm"))
            .args(["install", "--no-audit", "--no-fund", "--ignore-scripts"])
            .current_dir(&origin)
            .output()
            .unwrap();
        println!(
            "npm install in origin: {} ({} bytes stderr)",
            out.status,
            out.stderr.len()
        );
    }

    let reap = mode != "naive";
    println!(
        "mode={mode} cycles={n} reap_before_delete={reap} root={}",
        root.display()
    );
    println!("backoff schedule: {:?} ms\n", BACKOFF_MS);

    let mut results: Vec<Teardown> = Vec::new();
    let mut errors: std::collections::HashMap<String, usize> = Default::default();

    for i in 0..n {
        let wt = root.join(format!("wt{i:03}"));
        let branch = format!("s{i:03}");
        let out = git(
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &branch,
                wt.to_str().unwrap(),
                "HEAD",
            ],
            &origin,
        );
        if !out.status.success() {
            println!("worktree add failed: {}", String::from_utf8_lossy(&out.stderr));
            break;
        }

        // node_modules is not tracked, so a worktree starts without it. Link the one
        // from origin in for the npm mode, which is what a real workspace would have.
        // npm  - node_modules as a junction to origin's. Deleting the workspace only
        //        removes the link, so this measures the junction strategy, not a real tree.
        // npmreal - node_modules copied in for real. This is the honest heavy case.
        if mode == "npm" && origin.join("node_modules").exists() {
            let _ = Command::new("cmd")
                .args([
                    "/s",
                    "/c",
                    &format!(
                        "mklink /J \"{}\" \"{}\"",
                        wt.join("node_modules").display(),
                        origin.join("node_modules").display()
                    ),
                ])
                .output();
        }
        if mode == "npmreal" && origin.join("node_modules").exists() {
            let _ = Command::new("robocopy")
                .args([
                    origin.join("node_modules").to_str().unwrap(),
                    wt.join("node_modules").to_str().unwrap(),
                    "/E",
                    "/NFL",
                    "/NDL",
                    "/NJH",
                    "/NJS",
                    "/NP",
                    "/MT:16",
                ])
                .output();
        }

        let server_cwd = if mode == "cwdout" { root.clone() } else { wt.clone() };
        let (job, pid) = spawn_server(&wt, &server_cwd);
        if !wait_ready(&wt, 30) {
            println!("cycle {i}: dev server never signalled READY (pid {pid})");
        }

        if reap {
            unsafe {
                let _ = CloseHandle(job);
            }
        }

        let t = supervised_teardown(&wt, &quarantine);
        // In naive mode the server was never reaped. Did the teardown itself kill it,
        // by deleting the files out from under a running process?
        let survived = process_alive(pid);
        if !reap {
            println!(
                "cycle {i}: server pid {pid} {} the teardown",
                if survived { "SURVIVED" } else { "died during" }
            );
        }
        if let Some((p, code)) = &t.last_error {
            *errors
                .entry(format!(
                    "{} on {}",
                    err_name(*code),
                    if p.parent() == Some(&root) {
                        "the workspace root directory"
                    } else if p.is_dir() {
                        "a subdirectory"
                    } else {
                        "a file"
                    }
                ))
                .or_default() += 1;
        }
        println!(
            "cycle {i:>3}: attempts={:<2} {:>8.1?} {}{}",
            t.attempts,
            t.elapsed,
            match t.outcome {
                Outcome::Deleted => "deleted",
                Outcome::Quarantined => "QUARANTINED",
                Outcome::Stuck => "STUCK - still on disk",
            },
            match &t.last_error {
                Some((p, c)) if t.outcome != Outcome::Deleted => format!(
                    "  blocked by {} on {}",
                    err_name(*c),
                    p.file_name().unwrap_or(p.as_os_str()).to_string_lossy()
                ),
                _ => String::new(),
            }
        );

        if !reap {
            unsafe {
                let _ = CloseHandle(job);
            }
        }
        let _ = git(&["worktree", "prune"], &origin);
        let _ = git(&["branch", "-qD", &branch], &origin);
        results.push(t);
    }

    // --- summary ---------------------------------------------------------------------
    let total = results.len();
    if total == 0 {
        return;
    }
    let quarantined = results
        .iter()
        .filter(|r| r.outcome == Outcome::Quarantined)
        .count();
    let stuck = results.iter().filter(|r| r.outcome == Outcome::Stuck).count();
    let first_try = results
        .iter()
        .filter(|r| r.attempts == 1 && r.outcome == Outcome::Deleted)
        .count();
    let mut times: Vec<u128> = results.iter().map(|r| r.elapsed.as_micros()).collect();
    times.sort_unstable();
    let pct = |p: usize| times[(times.len() * p / 100).min(times.len() - 1)];

    println!("\n=== {mode}: {total} cycles ===");
    println!(
        "deleted first attempt : {first_try}/{total} ({:.0}%)",
        first_try as f64 * 100.0 / total as f64
    );
    println!(
        "quarantined           : {quarantined}/{total} ({:.0}%)",
        quarantined as f64 * 100.0 / total as f64
    );
    println!(
        "stuck on disk         : {stuck}/{total} ({:.0}%)",
        stuck as f64 * 100.0 / total as f64
    );
    println!(
        "teardown p50 / p95 / max : {:.1}ms / {:.1}ms / {:.1}ms",
        pct(50) as f64 / 1000.0,
        pct(95) as f64 / 1000.0,
        times[times.len() - 1] as f64 / 1000.0
    );
    if !errors.is_empty() {
        println!("blocking errors seen:");
        let mut e: Vec<_> = errors.into_iter().collect();
        e.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        for (k, c) in e.iter().take(8) {
            println!("  {c:>4}x  {k}");
        }
    }
}

/// Cheap liveness probe. Pid alone is unsafe as an identity - see the job objects
/// spike - but here the pid was created moments ago and is only being asked
/// "did you already exit", so reuse cannot produce a false negative.
fn process_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::STILL_ACTIVE;
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
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
