//! Spike: does a Win32 Job Object with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE reap a
//! real `.cmd`-rooted agent process tree on Windows, and does killing the root
//! process alone leave orphans behind?
//!
//! Usage: jobspike job | jobspike naive

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, STILL_ACTIVE};
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, GetProcessTimes, OpenProcess, ResumeThread, TerminateProcess,
    CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    InitializeProcThreadAttributeList, UpdateProcThreadAttribute, EXTENDED_STARTUPINFO_PRESENT,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTUPINFOEXW, STARTUPINFOW,
};

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Explicit executable resolution. Never hand a bare command name to CreateProcessW:
/// it does not consult PATHEXT, so `npm` alone silently fails to find `npm.cmd`.
/// This is a named root cause of herdr's Windows spawn failures.
fn resolve_on_path(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect();

    let has_exec_ext = |p: &PathBuf| {
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| exts.contains(&format!(".{}", e.to_ascii_lowercase())))
            .unwrap_or(false)
    };

    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        // PATHEXT candidates FIRST. A bare `npm` file with no extension exists next to
        // `npm.cmd` in the Node install - it is the sh script, and Windows cannot execute
        // it. Matching the extensionless file first is a real and silent trap.
        for ext in &exts {
            let cand = dir.join(format!("{name}{ext}"));
            if cand.is_file() {
                return Some(cand);
            }
        }
        let base = dir.join(name);
        if base.is_file() && has_exec_ext(&base) {
            return Some(base);
        }
    }
    None
}

/// Every live process as (pid, parent_pid, name).
fn snapshot() -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).expect("snapshot");
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                out.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    String::from_utf16_lossy(&entry.szExeFile[..len]),
                ));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Process creation time as a raw FILETIME, or None if the process is gone.
fn created_at(pid: u32) -> Option<u64> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut c = FILETIME::default();
        let (mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        let ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        Some(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
    }
}

/// Transitive descendants of `root`, including `root` itself, as (pid, name, created_at).
///
/// PID REUSE IS THE HAZARD HERE. Windows recycles pids aggressively, so a stale pid in a
/// parent-pid chain can point at a brand new, unrelated process, which then looks like a
/// descendant. A descendant cannot predate its ancestor, so every candidate is checked
/// against the root's creation time before being admitted.
fn descendants(root: u32) -> Vec<(u32, String, u64)> {
    let all = snapshot();
    let Some(root_created) = created_at(root) else {
        return Vec::new();
    };
    let mut keep: Vec<(u32, String, u64)> = all
        .iter()
        .filter(|(pid, _, _)| *pid == root)
        .map(|(pid, _, name)| (*pid, name.clone(), root_created))
        .collect();
    loop {
        let before = keep.len();
        for (pid, ppid, name) in &all {
            if !keep.iter().any(|(k, _, _)| k == ppid) || keep.iter().any(|(k, _, _)| k == pid) {
                continue;
            }
            match created_at(*pid) {
                Some(c) if c >= root_created => keep.push((*pid, name.clone(), c)),
                _ => {}
            }
        }
        if keep.len() == before {
            break;
        }
    }
    keep
}

/// Every live console-host process as (pid, created_at). ConPTY starts one of these
/// out-of-band, so it never appears as a descendant of the process we spawned.
fn console_hosts() -> Vec<(u32, u64)> {
    snapshot()
        .into_iter()
        .filter(|(_, _, name)| {
            let n = name.to_ascii_lowercase();
            n == "conhost.exe" || n == "openconsole.exe"
        })
        .filter_map(|(pid, _, _)| created_at(pid).map(|c| (pid, c)))
        .collect()
}

/// True only if this exact process is still running - same pid AND same creation time.
fn still_running(pid: u32, created: u64) -> bool {
    alive(pid) && created_at(pid) == Some(created)
}

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

fn main() -> windows::core::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "job".into());
    let tree_dir = std::env::current_dir().unwrap().join("tree");
    let pidfile = tree_dir.join(format!("pids-{mode}.txt"));
    let _ = std::fs::remove_file(&pidfile);

    // --- 1. Explicit .cmd resolution -------------------------------------------------
    let npm = resolve_on_path("npm").expect("npm not found on PATH");
    println!("resolved npm -> {}", npm.display());
    assert_eq!(
        npm.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase),
        Some("cmd".into()),
        "expected the .cmd shim, not a bare name"
    );

    // A .cmd is a batch script, not an image. CreateProcessW cannot execute it directly,
    // so it must be run through the command interpreter explicitly.
    let comspec =
        std::env::var("ComSpec").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".into());
    let mut cmdline = wide(&format!(
        "\"{}\" /s /c \"\"{}\" run spawner\"",
        comspec,
        npm.display()
    ));
    let app = wide(&comspec);
    let cwd = wide(tree_dir.to_str().unwrap());

    // --- 2. Job object with kill-on-close --------------------------------------------
    let job: HANDLE = unsafe { CreateJobObjectW(None, PCWSTR::null())? };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;
    }

    // --- 3. Spawn suspended, assign, then resume -------------------------------------
    // Suspended-then-assign is the only race-free order: a process that runs even
    // briefly before assignment can spawn a child outside the job.
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    let mut env_block: Vec<u16> = Vec::new();
    for (k, v) in std::env::vars() {
        env_block.extend(wide(&format!("{k}={v}")));
    }
    env_block.extend(wide(&format!("JOBSPIKE_PIDFILE={}", pidfile.display())));
    env_block.push(0);

    // conpty mode hosts the same tree behind a pseudoconsole, which is what a
    // terminal-only seat adapter owns. The open question is whether the console host
    // that ConPTY starts is inside the job, or leaks when the job closes.
    let mut pty: Option<(HPCON, HANDLE, HANDLE)> = None;
    let mut attr_buf: Vec<u8> = Vec::new();

    unsafe {
        if mode == "conpty" {
            let (mut in_r, mut in_w) = (HANDLE::default(), HANDLE::default());
            let (mut out_r, mut out_w) = (HANDLE::default(), HANDLE::default());
            CreatePipe(&mut in_r, &mut in_w, None, 0)?;
            CreatePipe(&mut out_r, &mut out_w, None, 0)?;
            let hpc = CreatePseudoConsole(COORD { X: 120, Y: 40 }, in_r, out_w, 0)?;
            let _ = CloseHandle(in_r);
            let _ = CloseHandle(out_w);

            // Drain the pty output, or the child blocks once the pipe buffer fills.
            let raw = out_r.0 as usize;
            std::thread::spawn(move || {
                let h = HANDLE(raw as *mut _);
                let mut buf = [0u8; 4096];
                loop {
                    let mut n = 0u32;
                    if ReadFile(h, Some(&mut buf), Some(&mut n), None).is_err() || n == 0 {
                        break;
                    }
                }
            });

            let mut size = 0usize;
            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
            attr_buf = vec![0u8; size];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buf.as_mut_ptr() as *mut _);
            InitializeProcThreadAttributeList(Some(list), 1, None, &mut size)?;
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(hpc.0 as *const _),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            )?;

            let six = STARTUPINFOEXW {
                StartupInfo: STARTUPINFOW {
                    cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
                    ..Default::default()
                },
                lpAttributeList: list,
            };
            CreateProcessW(
                PCWSTR(app.as_ptr()),
                Some(PWSTR(cmdline.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                Some(env_block.as_ptr() as *const _),
                PCWSTR(cwd.as_ptr()),
                &six.StartupInfo,
                &mut pi,
            )?;
            pty = Some((hpc, in_w, out_r));
        } else {
            CreateProcessW(
                PCWSTR(app.as_ptr()),
                Some(PWSTR(cmdline.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_NEW_PROCESS_GROUP
                    | CREATE_UNICODE_ENVIRONMENT,
                Some(env_block.as_ptr() as *const _),
                PCWSTR(cwd.as_ptr()),
                &si,
                &mut pi,
            )?;
        }
        // The negative case is "no job at all, kill the parent" - herdr's actual situation -
        // so naive mode must not be assigned to the job either.
        if mode != "naive" {
            AssignProcessToJobObject(job, pi.hProcess)?;
        }
        ResumeThread(pi.hThread);
        let _ = CloseHandle(pi.hThread);
    }
    let root = pi.dwProcessId;
    println!("root pid {root} spawned suspended (mode={mode}), resumed");

    // --- 4. Wait for the tree to build itself ----------------------------------------
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if std::fs::read_to_string(&pidfile)
            .map(|s| s.contains("grandchild "))
            .unwrap_or(false)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "tree never reached grandchild depth"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_millis(500));

    // A ConPTY's console host is started by the system, not by us, so it is not a
    // descendant and the tree walk cannot see it. Census it globally instead.
    let hosts_before = console_hosts();
    let tree = descendants(root);
    println!("\ntree before kill ({} processes):", tree.len());
    for (pid, name, _) in &tree {
        println!("  {pid:>7}  {name}");
    }

    // --- 5. Kill ---------------------------------------------------------------------
    let t0 = Instant::now();
    match mode.as_str() {
        "job" | "conpty" => {
            println!("\nmode={mode}: closing the job handle");
            unsafe { CloseHandle(job)? };
        }
        "naive" => {
            println!("\nmode=naive: TerminateProcess on the root only");
            unsafe {
                TerminateProcess(pi.hProcess, 1)?;
            }
        }
        other => panic!("unknown mode {other}"),
    }

    // --- 6. Measure release latency ---------------------------------------------------
    let mut settle = None;
    let watch_until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < watch_until {
        if tree.iter().all(|(pid, _, c)| !still_running(*pid, *c)) {
            settle = Some(t0.elapsed());
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(1500));

    let survivors: Vec<_> = tree
        .iter()
        .filter(|(pid, _, c)| still_running(*pid, *c))
        .collect();
    println!("\nsurvivors after kill: {}", survivors.len());
    for (pid, name, _) in &survivors {
        println!("  ORPHAN {pid:>7}  {name}");
    }
    match settle {
        Some(d) => println!("whole tree dead in {:.1?}", d),
        None => println!("tree NOT fully dead within 10s"),
    }

    if let Some((hpc, in_w, out_r)) = pty {
        let leaked: Vec<_> = console_hosts()
            .into_iter()
            .filter(|(pid, c)| !hosts_before.contains(&(*pid, *c)))
            .collect();
        println!("console hosts leaked by the pty after job close: {}", leaked.len());
        for (pid, _) in &leaked {
            println!("  LEAKED HOST {pid}");
        }
        unsafe {
            ClosePseudoConsole(hpc);
            let _ = CloseHandle(in_w);
            let _ = CloseHandle(out_r);
        }
        let after_close: Vec<_> = console_hosts()
            .into_iter()
            .filter(|(pid, c)| !hosts_before.contains(&(*pid, *c)))
            .collect();
        println!(
            "console hosts still present after ClosePseudoConsole: {}",
            after_close.len()
        );
    }
    drop(attr_buf);

    // Survivors are reported, never auto-terminated. Terminating on an inferred pid list
    // is exactly the mistake this spike exists to prove unnecessary.
    if !survivors.is_empty() {
        println!(
            "\nclean up manually: Stop-Process -Id {} -Force",
            survivors
                .iter()
                .map(|(p, _, _)| p.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    unsafe {
        let _ = CloseHandle(pi.hProcess);
    }
    Ok(())
}
