//! One binary ships both runtime and CLI, per `01 cell primitive`.
//!
//! That is what makes the compatibility promise in `16 local api surface` section 9 exist purely
//! for third-party UIs: the CLI can never skew from the runtime it talks to,
//! because it is the same build.

use std::path::PathBuf;
use std::sync::Arc;

mod acp_server;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use farseer_api::{AppState, RuntimeToken, serve, validate_dir};
use farseer_core::RunnerConfig;
use farseer_store::Store;

#[derive(Parser)]
#[command(name = "farseer", version, about, long_about = None)]
struct Cli {
    /// Directory of cell definitions. Files, in git, edited in your own editor.
    #[arg(long, global = true, default_value = "cells")]
    cells: PathBuf,

    /// Where the record lives. Defaults to the per-user application data
    /// directory so a run from any working directory finds the same log.
    #[arg(long, global = true)]
    record: Option<PathBuf>,

    /// The git repository a `Worktree`-strategy cell's runs are worktrees
    /// of. `13 harness build kit` keeps no git flag on `CellDefinition`, so this has to come
    /// from the CLI or default to the current directory - the common case,
    /// since cell zero is farseer's own builder harness. Only matters when
    /// running `serve`; ignored by `validate` and `where`.
    #[arg(long, global = true)]
    repo: Option<PathBuf>,

    /// Machine-wide runner facts: which account each runner signs in with, so
    /// `27 quota accounting` can key a subscription window by the thing that
    /// owns it. Absent is fine - every runner is then its own account, which
    /// declines to merge rather than guessing that two share a login.
    #[arg(long, global = true, default_value = "runners.toml")]
    runners: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the local API on 127.0.0.1 until interrupted.
    Serve {
        /// 0 asks the OS for a free port; the chosen one lands in the runtime
        /// file where the CLI looks for it.
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Parse and check every definition, then exit non-zero if any is broken.
    Validate,
    /// Print where the runtime writes its port and token.
    Where,
    /// Speak ACP on stdio, so an editor can drive a running farseer.
    ///
    /// `16 local api surface` made this an adapter on top of the HTTP surface
    /// rather than a second transport, so it attaches to a farseer that is
    /// already serving rather than starting one - and it declares at the
    /// handshake that it is an orchestrator, per `06 cell transport`.
    Acp,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate => validate(&cli.cells),
        Command::Where => {
            println!("{}", farseer_api::runtime_file_path().display());
            Ok(())
        }
        Command::Acp => {
            let runtime = acp_server::Runtime::attach()?;
            // Multi-threaded, because a prompt runs as its own task while the
            // read loop keeps serving - which is what lets `session/cancel`
            // overtake the turn it cancels.
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(acp_server::serve_stdio(runtime))
        }
        Command::Serve { port } => {
            let record = cli.record.map(Ok).unwrap_or_else(default_record_path)?;
            let repo_root = cli.repo.map(Ok).unwrap_or_else(|| {
                std::env::current_dir().context("reading the current directory")
            })?;
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(run(cli.cells, cli.runners, record, repo_root, port))
        }
    }
}

fn validate(cells: &std::path::Path) -> Result<()> {
    let report = validate_dir(cells);
    for cell in &report.loaded {
        println!("ok       {cell}");
    }
    for advisory in &report.advisories {
        println!("note     {}: {}", advisory.file, advisory.message);
    }
    for error in &report.errors {
        eprintln!("broken   {}: {}", error.file, error.message);
    }
    if report.errors.is_empty() {
        Ok(())
    } else {
        std::process::exit(1)
    }
}

async fn run(
    cells: PathBuf,
    runners: PathBuf,
    record: PathBuf,
    repo_root: PathBuf,
    port: u16,
) -> Result<()> {
    if let Some(parent) = record.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let store = Store::open(&record).with_context(|| format!("opening {}", record.display()))?;
    let runs_dir = record
        .parent()
        .context("record path has no parent directory")?
        .join("runs");
    std::fs::create_dir_all(&runs_dir)
        .with_context(|| format!("creating {}", runs_dir.display()))?;
    let runner_config = match std::fs::read_to_string(&runners) {
        Ok(text) => {
            RunnerConfig::load(&text).with_context(|| format!("reading {}", runners.display()))?
        }
        // Absent is the common case and not an error: accounts are how the
        // operator sharpens accounting, never a precondition for running.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => RunnerConfig::default(),
        Err(error) => return Err(error).with_context(|| format!("reading {}", runners.display())),
    };
    let state = Arc::new(
        AppState::new(
            store,
            &cells,
            RuntimeToken::generate(),
            runs_dir,
            &repo_root,
        )
        .with_runner_config(runner_config),
    );

    // Close what the last process left open, before anything reads the fleet.
    //
    // `17 cell lifecycle` chose no orphan survival, so a run this process did
    // not start has no handle, no liveness and no cancel - but its row still
    // said `running`, and every fleet surface believed it. Reaped once here
    // rather than filtered at each surface: a row that is wrong is wrong once,
    // and three widgets each working around it is three places to forget.
    match state.reap_orphaned_runs() {
        Ok(0) => {}
        Ok(reaped) => println!("reaped:  {reaped} run(s) left open by a previous process"),
        Err(error) => eprintln!("reaping runs left open by a previous process: {error}"),
    }

    let report = state.reload();
    for error in &report.errors {
        eprintln!("broken   {}: {}", error.file, error.message);
    }
    println!(
        "farseer: {} cell(s) loaded from {}",
        report.loaded.len(),
        cells.display()
    );
    println!("record:  {}", record.display());
    println!("repo:    {}", repo_root.display());
    println!("runtime: {}", farseer_api::runtime_file_path().display());

    serve(state, port).await.map_err(Into::into)
}

fn default_record_path() -> Result<PathBuf> {
    let base = farseer_api::runtime_file_path();
    let dir = base
        .parent()
        .context("runtime path has no parent directory")?;
    Ok(dir.join("record.sqlite3"))
}
