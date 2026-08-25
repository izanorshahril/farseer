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

mod runtime;
mod serve;
mod settings;

use std::path::PathBuf;

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
    tauri::Builder::default()
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(app, "canvas", url.clone())
                .title("farseer")
                .inner_size(1280.0, 820.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())?;

    // Held until the window closes: dropping it reaps a daemon this shell
    // started, and leaves alone one it merely attached to.
    drop(attached.owned);
    drop(tokio);
    Ok(())
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
