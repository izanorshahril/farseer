//! Settings: the operator changing a cell definition, from a window.
//!
//! **The API cannot do this, and that is on purpose.** `16 local api surface`
//! gave `/v1` read, validate and reload for definitions and **no edit path**,
//! because `01 cell primitive` made a cell definition data in git rather than a
//! row in a database.
//!
//! So the shell writes the file and asks the runtime to reload it - the same
//! split as widget code, where the shell owns the filesystem and the runtime
//! owns the record. Two things follow, both wanted:
//!
//! - **The change leaves a git diff.** `22 cell addressing` already relied on
//!   that: editing a definition and reloading "takes about ten seconds and
//!   leaves a git commit", which an in-conversation override would not.
//! - **Validation stays in one place.** The shell writes, then calls reload, and
//!   reports what the runtime says about the result rather than judging it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A runner the operator could put in front of a cell.
#[derive(Debug, Clone, Serialize)]
pub struct RunnerChoice {
    pub name: String,
    /// Whether it resolves on this machine right now.
    ///
    /// `10 runner inventory`'s rule is that reach is **observed, never
    /// advertised**, and the same applies to presence: offering a runner that is
    /// not installed produces a run that fails at spawn, which is a worse way to
    /// find out.
    pub installed: bool,
    pub path: Option<String>,
    /// What farseer knows how to drive it as.
    pub note: &'static str,
    /// What farseer **cannot** do with this runner, in the operator's words.
    ///
    /// Empty for a runner farseer has proven all four verbs against. Nothing is
    /// hidden or refused on the strength of this - `13 harness build kit` found
    /// the inventory is a menu rather than a survey, and a menu that silently
    /// drops entries teaches less than one that says why an entry is dimmer
    /// than its neighbour. Every runner stays choosable; the ones farseer holds
    /// loosely say so.
    pub cannot: Vec<&'static str>,
}

/// What farseer cannot do with this runner, worded as consequences rather than
/// as missing fields - the operator picks a manager, not a feature matrix.
fn cannot(runner: &str) -> Vec<&'static str> {
    let control = farseer_manager::control_of(runner);
    [
        (!control.steer, "cannot be steered once a run starts"),
        (
            !control.quota,
            "reports no quota, so exhaustion arrives as a failure",
        ),
        (
            !control.context,
            "reports no context window, so there is no denominator",
        ),
        (
            !control.compaction,
            "never says when it compacted, so a degraded answer looks like any other",
        ),
    ]
    .into_iter()
    .filter_map(|(missing, why)| missing.then_some(why))
    .collect()
}

/// The runners farseer has verified **native** stream-json dialects for.
///
/// Not a survey of what exists - `13 harness build kit` found the inventory is a
/// menu rather than a survey - but the list this build can actually launch.
/// Each entry is `(runner name, executable, what farseer knows how to drive it as)`.
///
/// The runner and its executable are **not the same string** - `claude-code` is
/// driven by a binary called `claude` - and resolving the runner name instead of
/// the executable reports a runner as missing while farseer launches it happily.
const KNOWN: [(&str, &str, &str); 5] = [
    (
        "claude-code",
        "claude",
        "steers mid-run, reports cost, carries farseer's MCP face",
    ),
    (
        "codex",
        "codex",
        "one-shot per run; resumption is a new process, not a steer",
    ),
    (
        "codex-app-server",
        "codex",
        "names its context window, its compaction and two quota windows",
    ),
    (
        "cursor-agent",
        "cursor-agent",
        "one-shot; tokens but no cost in currency",
    ),
    (
        "goose",
        "goose",
        "one-shot; terminal line carries no failure field",
    ),
];

/// What every ACP runner is, in the operator's terms.
///
/// One note for all of them because the protocol is what they have in common:
/// `29 harness protocol` found that an ACP agent reports a **context window**,
/// which no native runner does, and reports **no subscription window**, which is
/// what `27 quota accounting` runs on. That trade is the same whichever agent is
/// behind it, so saying it once is honest rather than lazy.
const ACP_NOTE: &str =
    "speaks ACP: names its context window, steers as a manager, reports no quota";

/// Every runner this build can launch: the native dialects above, then the ACP
/// agents from [`farseer_manager::ACP_RUNNERS`].
///
/// Read from there rather than repeated here, so the settings list cannot come
/// to disagree with what `start_worker` will actually accept - a menu offering a
/// runner the dispatch refuses is a worse failure than a missing one, because it
/// fails after the operator has committed a definition.
fn known() -> Vec<(&'static str, &'static str, &'static str)> {
    KNOWN
        .into_iter()
        .chain(
            farseer_manager::ACP_RUNNERS
                .into_iter()
                .map(|(name, executable, _)| (name, executable, ACP_NOTE)),
        )
        .collect()
}

pub fn runners() -> Vec<RunnerChoice> {
    known()
        .into_iter()
        .map(|(name, executable, note)| {
            let path = farseer_runner::resolve::resolve(executable);
            RunnerChoice {
                cannot: cannot(name),
                name: name.to_string(),
                installed: path.is_some(),
                path: path.map(|p| p.display().to_string()),
                note,
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct TopManagerRequest {
    pub runner: String,
}

#[derive(Debug, Serialize)]
pub struct TopManager {
    pub cell_id: String,
    pub runner: String,
    pub file: String,
}

/// Which cell the operator talks to. `01 cell primitive` made it the address.
const TOP_CELL: &str = "zero";

pub fn top_manager(cells: &Path) -> anyhow::Result<TopManager> {
    let file = definition_path(cells);
    let text = std::fs::read_to_string(&file)?;
    Ok(TopManager {
        cell_id: TOP_CELL.to_string(),
        runner: current_runner(&text).unwrap_or_default(),
        file: file.display().to_string(),
    })
}

/// Rewrite `manager.runner`, touching nothing else in the file.
///
/// A whole-file rewrite through a TOML serializer would reformat the operator's
/// own comments and ordering out of existence, and `01 cell primitive` made
/// these files something a human edits. So this replaces one line.
pub fn set_top_manager(cells: &Path, runner: &str) -> anyhow::Result<TopManager> {
    if !known().iter().any(|(name, _, _)| *name == runner) {
        anyhow::bail!("`{runner}` is not a runner this build knows how to drive");
    }
    let file = definition_path(cells);
    let text = std::fs::read_to_string(&file)?;
    let Some(current) = current_runner(&text) else {
        anyhow::bail!("no `runner` under `[manager]` in {}", file.display());
    };
    if current == runner {
        return top_manager(cells);
    }

    // Keep the file's own line endings. Rewriting a Windows file with `\n`
    // touches every line, which is the reformatting this function exists to
    // avoid - and it hides the one line that actually changed inside a diff
    // claiming the whole file did.
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing = text.ends_with('\n');

    let mut in_manager = false;
    let mut out = String::with_capacity(text.len() + 8);
    let mut replaced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_manager = trimmed.starts_with("[manager]");
        }
        if in_manager && !replaced && trimmed.starts_with("runner") && trimmed.contains('=') {
            let indent = &line[..line.len() - trimmed.len()];
            out.push_str(&format!("{indent}runner = \"{runner}\"{newline}"));
            replaced = true;
            continue;
        }
        out.push_str(line);
        out.push_str(newline);
    }
    if !trailing {
        out.truncate(out.len() - newline.len());
    }
    std::fs::write(&file, out)?;
    top_manager(cells)
}

fn definition_path(cells: &Path) -> PathBuf {
    cells.join(format!("{TOP_CELL}.toml"))
}

fn current_runner(text: &str) -> Option<String> {
    let mut in_manager = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_manager = trimmed.starts_with("[manager]");
        }
        if in_manager
            && let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == "runner"
        {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_offers_exactly_what_dispatch_will_accept() {
        let offered: Vec<String> = runners().into_iter().map(|r| r.name).collect();
        for (name, _, _) in farseer_manager::ACP_RUNNERS {
            assert!(
                offered.iter().any(|o| o == name),
                "an ACP runner dispatch accepts must be offerable: {name}"
            );
        }
        // And nothing is offered that `set_top_manager` would then refuse.
        for name in &offered {
            assert!(known().iter().any(|(known, _, _)| known == name));
        }
    }

    #[test]
    fn a_runner_farseer_holds_loosely_says_so_and_is_still_offered() {
        let offered = runners();
        // Nothing is hidden: every runner this build can launch is choosable,
        // whatever farseer can or cannot do with it.
        assert_eq!(offered.len(), known().len());

        let goose = offered
            .iter()
            .find(|choice| choice.name == "goose")
            .expect("goose is offerable");
        assert_eq!(
            goose.cannot.len(),
            4,
            "a one-shot runner farseer cannot steer, meter, size or catch compacting"
        );

        let codex_app_server = offered
            .iter()
            .find(|choice| choice.name == "codex-app-server")
            .expect("codex-app-server is offerable");
        assert!(
            codex_app_server
                .cannot
                .iter()
                .all(|warning| warning.contains("steered")),
            "only steering is missing here: {:?}",
            codex_app_server.cannot
        );
    }

    #[test]
    fn an_acp_runner_is_reported_against_its_executable_rather_than_its_name() {
        // `goose-acp` is driven by a binary called `goose`. Resolving the runner
        // name would report an installed runner as missing, which is the bug
        // this file's `KNOWN` table already exists to prevent.
        let goose_acp = runners()
            .into_iter()
            .find(|choice| choice.name == "goose-acp")
            .expect("goose-acp is offerable");
        let goose = runners()
            .into_iter()
            .find(|choice| choice.name == "goose")
            .expect("goose is offerable");
        assert_eq!(goose_acp.installed, goose.installed);
        assert_eq!(goose_acp.path, goose.path);
    }

    const DEFINITION: &str = r#"# Cell #0 - the builder harness.
cell_id = "zero"
name = "Cell Zero"

[manager]
runner = "claude-code"
prompt = """
multi
line
"""

[[roster]]
kind = "worker"
name = "coder"
runner = "codex"
"#;

    fn written(runner: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zero.toml"), DEFINITION).unwrap();
        set_top_manager(dir.path(), runner).unwrap();
        std::fs::read_to_string(dir.path().join("zero.toml")).unwrap()
    }

    #[test]
    fn only_the_managers_runner_changes_and_the_file_survives_intact() {
        let after = written("goose");
        assert!(after.contains("runner = \"goose\""));
        assert!(
            after.contains("# Cell #0 - the builder harness."),
            "a whole-file rewrite would lose the operator's own comments"
        );
        assert!(
            after.contains("name = \"coder\"\nrunner = \"codex\""),
            "a roster worker's runner is not the manager's and must not move"
        );
        assert!(after.contains("multi\nline"));
    }

    #[test]
    fn a_windows_file_keeps_its_own_line_endings() {
        // `01 cell primitive` made these files something a human edits. A
        // rewrite that flips every line ending shows up as a whole-file diff and
        // buries the one line that actually changed.
        let dir = tempfile::tempdir().unwrap();
        let crlf = DEFINITION.replace('\n', "\r\n");
        std::fs::write(dir.path().join("zero.toml"), &crlf).unwrap();
        set_top_manager(dir.path(), "goose").unwrap();

        let after = std::fs::read_to_string(dir.path().join("zero.toml")).unwrap();
        assert_eq!(
            after.matches("\r\n").count(),
            crlf.matches("\r\n").count(),
            "every line ending must survive, not just most of them"
        );
        assert!(after.contains("runner = \"goose\"\r\n"));
    }

    #[test]
    fn an_unknown_runner_is_refused_before_the_file_is_touched() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("zero.toml"), DEFINITION).unwrap();
        assert!(set_top_manager(dir.path(), "gpt-9").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("zero.toml")).unwrap(),
            DEFINITION,
            "a refused change leaves no half-edit behind"
        );
    }

    #[test]
    fn every_known_runner_is_offered_with_its_presence_observed() {
        let offered = runners();
        assert_eq!(offered.len(), known().len());
        // `10 runner inventory`: observed, never advertised. Whether any of
        // these is installed here is a fact about the machine, so the test
        // checks the shape rather than the answer.
        assert!(offered.iter().all(|r| r.installed == r.path.is_some()));
    }
}
