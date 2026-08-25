//! Getting a farseer to talk to.
//!
//! `28 operator surface` made the desktop shell own the runtime, which is what
//! demotes `farseer serve` from the way in to an option. Two cases, and the
//! order matters: **attach to one that is already running**, then spawn.
//!
//! Attaching first is not politeness. `09 store decision` gives the record one
//! writer by construction, and a second daemon on the same record would break
//! that quietly - the operator would see two windows disagreeing rather than an
//! error.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Deserialize;

/// What `farseer serve` writes on startup, and `farseer where` prints.
#[derive(Debug, Clone, Deserialize)]
pub struct Runtime {
    pub port: u16,
    pub token: String,
}

/// The daemon this shell is talking to, and whether it owns it.
pub struct Attached {
    pub runtime: Runtime,
    /// `Some` only when this shell started it. A daemon the operator started
    /// outlives the window, which is `01 cell primitive`'s durability
    /// requirement: the runtime outlives any UI restart.
    child: Option<Child>,
}

impl Drop for Attached {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            // Only ever the one this shell spawned. Reaping a daemon the
            // operator started would take their fleet down with a window.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Read the runtime file, and check the daemon it names is actually answering.
///
/// A stale file outlives a crashed daemon, so the file alone is a claim rather
/// than a fact - and believing it produces the same connection-refused nobody
/// can act on.
pub fn attach_existing() -> Option<Runtime> {
    let path = farseer_api::security::runtime_file_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let runtime: Runtime = serde_json::from_str(&text).ok()?;
    std::net::TcpStream::connect_timeout(
        &(std::net::Ipv4Addr::LOCALHOST, runtime.port).into(),
        Duration::from_millis(300),
    )
    .ok()?;
    Some(runtime)
}

/// Start a daemon and wait for it to write its runtime file.
pub fn spawn(binary: &Path, cells: &Path, repo: &Path) -> Result<Attached> {
    let path = farseer_api::security::runtime_file_path();
    let before = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

    let child = Command::new(binary)
        .arg("serve")
        .arg("--port")
        .arg("0")
        .arg("--cells")
        .arg(cells)
        .arg("--repo")
        .arg(repo)
        .spawn()
        .with_context(|| format!("starting {}", binary.display()))?;

    // Port 0 means the OS chooses, so the file is the only place the real port
    // appears - and it is written after the listener binds, which is exactly
    // the moment there is something to connect to.
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(runtime) = serde_json::from_str::<Runtime>(&text)
        {
            let fresh = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .zip(before)
                .is_none_or(|(now, then)| now > then);
            if fresh
                && std::net::TcpStream::connect_timeout(
                    &(std::net::Ipv4Addr::LOCALHOST, runtime.port).into(),
                    Duration::from_millis(300),
                )
                .is_ok()
            {
                return Ok(Attached {
                    runtime,
                    child: Some(child),
                });
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(Attached {
        runtime: Runtime {
            port: 0,
            token: String::new(),
        },
        child: Some(child),
    })
}

/// Where the farseer binary is, next to this executable in an installed build
/// and in the same target directory during development.
pub fn sidecar_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(if cfg!(windows) {
        "farseer.exe"
    } else {
        "farseer"
    });
    candidate.exists().then_some(candidate)
}
