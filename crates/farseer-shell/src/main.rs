//! The farseer desktop shell.
//!
//! `28 operator surface` chose Tauri and demoted `farseer serve` to optional:
//! the shell attaches to a running daemon or starts one itself, serves the
//! canvas on a loopback port of its own, and proxies `/v1` with the operator
//! token attached on this side.
//!
//! What the window loads is therefore **one origin**, and the page inside it
//! never holds a credential.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod geometry;
mod runtime;
mod serve;
mod settings;
mod tray;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn main() {
    if let Err(error) = run() {
        eprintln!("farseer-shell: {error}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let canvas = serve::canvas_dir().ok_or_else(|| {
        anyhow::anyhow!("no canvas build found - run `bun run --cwd ui build` first")
    })?;

    // Attach before spawning. `09 store decision` gives the record one writer
    // by construction, and a second daemon on the same record would break that
    // quietly rather than loudly.
    let attached = match runtime::attach_existing() {
        Some(existing) => {
            println!("farseer-shell: attached to farseer on {}", existing.port);
            AttachedRuntime {
                port: existing.port,
                token: existing.token,
                owned: None,
            }
        }
        None => {
            let binary = runtime::sidecar_path().ok_or_else(|| {
                anyhow::anyhow!("no farseer binary beside this executable, and none running")
            })?;
            let owned = runtime::spawn(&binary, &repo_relative("cells"), &repo_root())?;
            println!("farseer-shell: started farseer on {}", owned.runtime.port);
            AttachedRuntime {
                port: owned.runtime.port,
                token: owned.runtime.token.clone(),
                owned: Some(owned),
            }
        }
    };

    let tokio = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let origin = tokio.block_on(serve::start(
        canvas,
        repo_relative("cells"),
        attached.port,
        attached.token.clone(),
    ))?;
    println!("farseer-shell: canvas on {origin}");

    let url = tauri::WebviewUrl::External(origin.parse()?);
    let (tray_port, tray_token) = (attached.port, attached.token.clone());

    // Where the window was last time, from the same store the canvas keeps its
    // widget order in. Read before the window exists, so it opens in the right
    // place rather than appearing and then jumping.
    let farseer = format!("http://127.0.0.1:{}", attached.port);
    let restored = tokio.block_on(geometry::load(&farseer, &attached.token));
    // Shared with the window's event handler, which needs the last **restored**
    // size to answer what a maximized window is: maximizing overwrites the size
    // with the screen's, and un-maximizing has to give back the operator's.
    let held = Arc::new(Mutex::new(restored));

    tauri::Builder::default()
        .setup(move |app| {
            let window = tauri::WebviewWindowBuilder::new(app, "canvas", url.clone())
                .title("farseer")
                .inner_size(restored.width, restored.height)
                .build()?;
            geometry::apply(&window, restored);
            watch_geometry(&window, farseer.clone(), tray_token.clone(), held.clone());
            // The tray asks the daemon directly rather than the canvas origin:
            // it is a second reader of one surface, not a second surface.
            tray::install(
                app.handle(),
                format!("http://127.0.0.1:{tray_port}"),
                tray_token.clone(),
            )?;
            Ok(())
        })
        .run(tauri::generate_context!())?;

    // Held until the window closes: dropping it reaps a daemon this shell
    // started, and leaves alone one it merely attached to.
    drop(attached.owned);
    drop(tokio);
    Ok(())
}

/// How long to wait after a window event before believing what the window says.
///
/// **Not a debounce for write volume; a wait for the truth.** Measured on
/// Windows: maximizing emits `Resized` with the screen's dimensions *before*
/// `is_maximized()` starts answering `true`, so reading at the instant of the
/// event records the screen as the size the operator chose. It then reopens
/// maximized - correct - and un-maximizes to the screen size, which is the bug
/// wearing the fix's clothes.
///
/// Coalescing a drag is the incidental benefit, not the reason.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

/// Keep the store in step with the window.
///
/// **Saved on move and resize rather than only on close**, because a shell that
/// records its geometry only on a clean exit records nothing at all the one time
/// it matters - a crash, a forced quit, a machine that slept and did not wake.
fn watch_geometry<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    farseer: String,
    token: String,
    held: Arc<Mutex<geometry::Geometry>>,
) {
    let handle = window.clone();
    // Which settle is the current one. A drag emits events at frame rate, and
    // every one of them starts a wait; only the last still matters by the time
    // the waits expire, so the earlier ones stand down rather than each writing
    // a frame of the drag.
    let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
    window.on_window_event(move |event| {
        use std::sync::atomic::Ordering;
        use tauri::WindowEvent;
        if !matches!(
            event,
            WindowEvent::Resized(_) | WindowEvent::Moved(_) | WindowEvent::CloseRequested { .. }
        ) {
            return;
        }
        let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (handle, farseer, token) = (handle.clone(), farseer.clone(), token.clone());
        let (held, generation) = (held.clone(), generation.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(SETTLE).await;
            if generation.load(Ordering::SeqCst) != mine {
                return;
            }
            let last = *held.lock().unwrap_or_else(|e| e.into_inner());
            // `None` while minimized: Windows reports a minimized window at
            // -32000, and storing that is how an app comes back invisible.
            let Some(now) = geometry::current(&handle, last) else {
                return;
            };
            if now == last {
                return;
            }
            *held.lock().unwrap_or_else(|e| e.into_inner()) = now;
            geometry::save(farseer, token, now).await;
        });
    });
}

struct AttachedRuntime {
    port: u16,
    token: String,
    owned: Option<runtime::Attached>,
}

fn repo_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn repo_relative(name: &str) -> PathBuf {
    repo_root().join(name)
}
