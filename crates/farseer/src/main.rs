//! One binary ships both runtime and CLI, per `01 cell primitive`.
//!
//! That is what makes the compatibility promise in `16 local api surface` section 9 exist purely
//! for third-party UIs: the CLI can never skew from the runtime it talks to,
//! because it is the same build.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

mod acp_server;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use farseer_api::{AppState, RuntimeToken, serve, validate_dir};
use farseer_core::RunnerConfig;
use farseer_store::Store;
use farseer_store::maintenance::{
    BackupIdentity, CommandObservation, CommandSpec, FixturePromotion, HealthObservation,
    PromotionPlan, read_runtime_identity,
};

struct FixtureCommands {
    migration_program: Option<String>,
    migration_args: Vec<String>,
    startup_program: Option<String>,
    startup_args: Vec<String>,
}

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
    /// Stop admitting new work and finish active runs.
    Drain,
    /// Cancel active runs and stop the runtime after supervised cleanup.
    Force {
        /// Why the operator chose force.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Show the runtime lifecycle state and active run count.
    Status,
    /// Parse and check every definition, then exit non-zero if any is broken.
    Validate,
    /// Print where the runtime writes its port and token.
    Where,
    /// Write a consistent record and attachment backup directory.
    Backup { destination: PathBuf },
    /// Restore a backup directory into the configured record path.
    Restore { backup: PathBuf },
    /// Promote a versioned disposable fixture through drain, backup, health,
    /// and rollback gates. This never replaces a live installation.
    PromoteFixture {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long)]
        backup: PathBuf,
        #[arg(long, default_value_t = 0)]
        active_runs: usize,
        #[arg(long)]
        health: bool,
        #[arg(long)]
        smoke: bool,
        /// Optional migration executable, run from the staged candidate.
        #[arg(long)]
        migration_program: Option<String>,
        /// Arguments passed directly to the migration executable.
        #[arg(long)]
        migration_arg: Vec<String>,
        /// Optional bounded candidate startup/smoke executable.
        #[arg(long)]
        startup_program: Option<String>,
        /// Arguments passed directly to the startup executable.
        #[arg(long)]
        startup_arg: Vec<String>,
    },
    /// Roll back the durable fixture promotion journal without starting the
    /// candidate runtime.
    RollbackFixture {
        #[arg(long)]
        root: PathBuf,
    },
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
        Command::Backup { destination } => {
            let record = cli.record.map(Ok).unwrap_or_else(default_record_path)?;
            let attachments = record
                .parent()
                .context("record path has no parent directory")?
                .join("transcripts");
            let manifest = Store::open(&record)
                .with_context(|| format!("opening {}", record.display()))?
                .backup_to(&destination, &attachments)
                .with_context(|| format!("backing up to {}", destination.display()))?;
            println!(
                "backup: {} attachment reference(s) written to {}",
                manifest.attachments.len(),
                destination.display()
            );
            Ok(())
        }
        Command::Restore { backup } => {
            let record = cli.record.map(Ok).unwrap_or_else(default_record_path)?;
            let attachments = record
                .parent()
                .context("record path has no parent directory")?
                .join("transcripts");
            let manifest = Store::restore_from(&backup, &record, &attachments)
                .with_context(|| format!("restoring {}", backup.display()))?;
            println!(
                "restore: {} attachment reference(s) validated at {}",
                manifest.attachments.len(),
                record.display()
            );
            Ok(())
        }
        Command::PromoteFixture {
            root,
            candidate,
            backup,
            active_runs,
            health,
            smoke,
            migration_program,
            migration_arg,
            startup_program,
            startup_arg,
        } => promote_fixture(
            root,
            candidate,
            backup,
            active_runs,
            health,
            smoke,
            FixtureCommands {
                migration_program,
                migration_args: migration_arg,
                startup_program,
                startup_args: startup_arg,
            },
        ),
        Command::RollbackFixture { root } => rollback_fixture(root),
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
        Command::Drain => control_runtime("drain", serde_json::json!({})),
        Command::Force { reason } => {
            control_runtime("force", serde_json::json!({ "reason": reason }))
        }
        Command::Status => control_runtime("", serde_json::Value::Null),
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

fn control_runtime(action: &str, body: serde_json::Value) -> Result<()> {
    let runtime = acp_server::Runtime::attach()?;
    let path = if action.is_empty() {
        "/v1/runtime".to_string()
    } else {
        format!("/v1/runtime/{action}")
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let client = reqwest::Client::new();
            let url = format!("{}{path}", runtime.base);
            let request = if action.is_empty() {
                client.get(url)
            } else {
                client.post(url).json(&body)
            };
            let response = request
                .bearer_auth(runtime.token)
                .send()
                .await
                .context("calling the farseer runtime")?;
            let status = response.status();
            let text = response
                .text()
                .await
                .context("reading the runtime response")?;
            println!("{text}");
            if status.is_success() {
                Ok(())
            } else {
                Err(anyhow::anyhow!("runtime returned HTTP {status}"))
            }
        })
}

fn promote_fixture(
    root: PathBuf,
    candidate_path: PathBuf,
    backup_path: PathBuf,
    active_runs: usize,
    authenticated_health: bool,
    smoke_ok: bool,
    commands: FixtureCommands,
) -> Result<()> {
    let active_path = root.join("active");
    let previous = read_runtime_identity(&active_path).with_context(|| {
        format!(
            "reading previous fixture identity from {}",
            active_path.display()
        )
    })?;
    let candidate = read_runtime_identity(&candidate_path).with_context(|| {
        format!(
            "reading candidate fixture identity from {}",
            candidate_path.display()
        )
    })?;
    let plan = PromotionPlan {
        candidate: candidate.clone(),
        previous: previous.clone(),
        backup: BackupIdentity {
            runtime: previous,
            path: backup_path,
        },
        candidate_path,
        migration: commands.migration_program.map(|program| CommandSpec {
            program,
            args: commands.migration_args,
            timeout_ms: 5_000,
        }),
        startup: commands.startup_program.map(|program| CommandSpec {
            program,
            args: commands.startup_args,
            timeout_ms: 5_000,
        }),
    };
    let controller = FixturePromotion::new(&root);
    let mut journal = controller.stage(plan)?;
    controller.save_journal(&journal)?;
    controller.record_drain(&mut journal, active_runs)?;
    controller.save_journal(&journal)?;
    controller.record_backup(&mut journal)?;
    controller.save_journal(&journal)?;
    if let Some(migration) = journal.plan.migration.clone() {
        let observation = match run_fixture_command(&journal.staged_path, &migration) {
            Ok(observation) => observation,
            Err(error) => {
                let failed = CommandObservation {
                    command: migration.label(),
                    status: None,
                    timed_out: false,
                };
                let _ = controller.record_migration(&mut journal, &migration, failed);
                journal.recovery = Some(format!(
                    "migration command could not execute in {}; previous runtime remains active: {error}",
                    journal.staged_path.display()
                ));
                let _ = controller.save_journal(&journal);
                return Err(error);
            }
        };
        if let Err(error) = controller.record_migration(&mut journal, &migration, observation) {
            let _ = controller.save_journal(&journal);
            return Err(error.into());
        }
        controller.save_journal(&journal)?;
    }
    controller.activate(&mut journal)?;
    controller.save_journal(&journal)?;
    if let Some(startup) = journal.plan.startup.clone() {
        let observation = match run_fixture_command(&journal.active_path, &startup) {
            Ok(observation) => observation,
            Err(error) => {
                let failed = CommandObservation {
                    command: startup.label(),
                    status: None,
                    timed_out: false,
                };
                let _ = controller.record_startup(&mut journal, &startup, failed);
                journal.recovery = Some(format!(
                    "candidate startup command could not execute in {}; restore the previous runtime before admitting work: {error}",
                    journal.active_path.display()
                ));
                let _ = controller.save_journal(&journal);
                let _ = controller.rollback(&mut journal);
                let _ = controller.save_journal(&journal);
                return Err(error);
            }
        };
        if let Err(error) = controller.record_startup(&mut journal, &startup, observation) {
            let _ = controller.save_journal(&journal);
            let _ = controller.rollback(&mut journal);
            let _ = controller.save_journal(&journal);
            return Err(error.into());
        }
        controller.save_journal(&journal)?;
    }
    if let Err(error) = controller.verify_health(
        &mut journal,
        HealthObservation {
            runtime: candidate,
            authenticated: authenticated_health,
            smoke_ok,
        },
    ) {
        let _ = controller.rollback(&mut journal);
        let _ = controller.save_journal(&journal);
        return Err(error.into());
    }
    controller.save_journal(&journal)?;
    println!(
        "promotion: healthy fixture is active; phase={}",
        journal.phase
    );
    Ok(())
}

/// Execute one bounded promotion fixture command through the runner's
/// Windows Job Object and PATHEXT seams, as required by `18 safe staged
/// runtime promotion` and `03 spike job objects`.
fn run_fixture_command(cwd: &Path, spec: &CommandSpec) -> Result<CommandObservation> {
    spec.validate()?;
    let program = resolve_fixture_program(cwd, spec)?;
    let label = spec.label();
    let deadline = Instant::now() + Duration::from_millis(spec.timeout_ms);

    #[cfg(windows)]
    {
        use farseer_runner::spawn::{StdinMode, SupervisedProcess};

        let mut process =
            SupervisedProcess::spawn(&program, &spec.args, cwd, &[], StdinMode::Closed)
                .map_err(|error| anyhow::anyhow!("{label}: {error}"))?;
        loop {
            if let Some(status) = process.try_wait()? {
                return Ok(CommandObservation {
                    command: label,
                    status: status.code(),
                    timed_out: false,
                });
            }
            if Instant::now() >= deadline {
                process.kill();
                return Ok(CommandObservation {
                    command: label,
                    status: None,
                    timed_out: true,
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(not(windows))]
    {
        use std::process::{Command, Stdio};

        let mut child = Command::new(program)
            .args(&spec.args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| anyhow::anyhow!("{label}: {error}"))?;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(CommandObservation {
                    command: label,
                    status: status.code(),
                    timed_out: false,
                });
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(CommandObservation {
                    command: label,
                    status: None,
                    timed_out: true,
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Resolve a fixture executable without a shell; Windows bare names use the
/// `03 spike job objects` PATHEXT ordering from `farseer-runner`.
fn resolve_fixture_program(cwd: &Path, spec: &CommandSpec) -> Result<PathBuf> {
    let requested = Path::new(&spec.program);
    let local = cwd.join(requested);
    if requested.is_absolute() || requested.components().count() > 1 || local.is_file() {
        let resolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            local
        };
        let root = cwd
            .canonicalize()
            .map_err(|error| anyhow::anyhow!("{}: {error}", cwd.display()))?;
        let canonical = resolved
            .canonicalize()
            .map_err(|error| anyhow::anyhow!("{}: {error}", resolved.display()))?;
        if !requested.is_absolute() && !canonical.starts_with(&root) {
            return Err(anyhow::anyhow!(
                "maintenance command `{}` escapes its fixture directory",
                spec.program
            ));
        }
        return Ok(canonical);
    }

    #[cfg(windows)]
    {
        farseer_runner::resolve::resolve(&spec.program)
            .ok_or_else(|| anyhow::anyhow!("maintenance command `{}` was not found", spec.program))
    }
    #[cfg(not(windows))]
    {
        Ok(requested.to_path_buf())
    }
}

fn rollback_fixture(root: PathBuf) -> Result<()> {
    let controller = FixturePromotion::new(&root);
    let mut journal = controller.load_journal()?;
    controller.rollback(&mut journal)?;
    controller.save_journal(&journal)?;
    println!("promotion: rolled back fixture; phase={}", journal.phase);
    Ok(())
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
    let data_dir = record
        .parent()
        .context("record path has no parent directory")?;
    let _lease = farseer_api::security::acquire_data_directory_lease(data_dir)
        .with_context(|| format!("acquiring data-directory lease for {}", data_dir.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn success_command() -> CommandSpec {
        #[cfg(windows)]
        {
            CommandSpec {
                program: "cmd".into(),
                args: vec!["/C".into(), "exit 0".into()],
                timeout_ms: 1_000,
            }
        }
        #[cfg(not(windows))]
        {
            CommandSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "exit 0".into()],
                timeout_ms: 1_000,
            }
        }
    }

    fn timeout_command() -> CommandSpec {
        #[cfg(windows)]
        {
            CommandSpec {
                program: "cmd".into(),
                args: vec!["/C".into(), "ping -n 5 127.0.0.1 > nul".into()],
                timeout_ms: 20,
            }
        }
        #[cfg(not(windows))]
        {
            CommandSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "sleep 1".into()],
                timeout_ms: 20,
            }
        }
    }

    #[test]
    fn fixture_command_uses_the_runner_resolution_and_supervision_seam() {
        let directory = tempfile::tempdir().unwrap();
        let observation = run_fixture_command(directory.path(), &success_command()).unwrap();
        assert_eq!(observation.status, Some(0));
        assert!(!observation.timed_out);
    }

    #[test]
    fn fixture_command_timeout_is_observed_and_supervised() {
        let directory = tempfile::tempdir().unwrap();
        let observation = run_fixture_command(directory.path(), &timeout_command()).unwrap();
        assert_eq!(observation.status, None);
        assert!(observation.timed_out);
    }
}
