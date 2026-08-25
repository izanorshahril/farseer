//! The `goose` CLI runner (block/goose): invocation and stream-json mapping.
//!
//! Verified 2026-08-24 against the real, installed `goose` 1.47.0, run twice
//! with `goose run --no-session -q --output-format stream-json -t "reply
//! with just the word ok"` - once in this repo, once in a fresh `git init`
//! directory, to check for a Codex/cursor-agent-style fresh-workspace gate.
//! **None exists**: both runs succeeded with no extra flag, exit code 0.
//! goose's own configured provider on this machine (`chatgpt_codex`)
//! delegates through the already-authenticated `codex` CLI, so this probe
//! spent no new credential - same subscription `codex.rs` already uses.
//!
//! Captured lines, verbatim:
//!
//! ```text
//! {"type":"message","message":{"id":"...","role":"assistant","created":...,"content":[{"type":"text","text":"ok"}],"metadata":{"userVisible":true,"agentVisible":true,"inference":{"provider":"chatgpt_codex","requestedModel":"gpt-5.6-luna"}}}}
//! {"type":"complete","total_tokens":6774,"input_tokens":6769,"output_tokens":5,"cache_read_input_tokens":0,"cache_write_input_tokens":0,"cost_usd":0.0013598000000000002}
//! ```
//!
//! **`complete` carries no `is_error`/`subtype` field at all** - unlike the
//! other three runners, there is nothing on this line to read a failure
//! from, and only the success shape has ever been observed. Guessing a
//! failure payload would violate this project's own rule against it, so
//! `finished` below maps `complete` to `Outcome::Ok` unconditionally. This
//! is still safe: `farseer-manager`'s existing rule for end-of-stream with
//! no `Finished` signal (mid-run crash, indistinguishable from `cancel`
//! without `was_cancelled`) already covers a run that fails before
//! `complete` ever arrives, so a genuine failure is not silently reported
//! as `Ok` - it is reported the same way any other runner's crash is.
//!
//! No `--input-format`-style flag exists on this CLI (per `--help`); `-r,
//! --resume` restarts into a new process rather than continuing a live one,
//! same shape `10 runner inventory` found for Codex and cursor-agent, so there is no steering
//! path here either.

use farseer_core::run::{Outcome, WorkerContractSpec};
use serde_json::Value;

use crate::claude_code::{FinishedSignal, ParseError, RunnerSignal};

/// The flags this module's own probe verified: `run` to execute
/// non-interactively, `--no-session` since farseer gives every run its own
/// fresh worktree and has no use for goose's own session/resume file,
/// `-q` to suppress goose's own banner/status noise, `--output-format
/// stream-json`, and the goal via `-t` - goose has no positional prompt
/// argument, unlike the other three runners.
pub fn build_args(contract: &WorkerContractSpec) -> Vec<String> {
    vec![
        "run".into(),
        "--no-session".into(),
        "-q".into(),
        "--output-format".into(),
        "stream-json".into(),
        "-t".into(),
        contract.goal.clone(),
    ]
}

pub fn parse_line(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
    let v: Value = serde_json::from_str(line).map_err(|e| ParseError(e.to_string()))?;
    let Some(kind) = v.get("type").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let signal = match kind {
        "complete" => Some(RunnerSignal::Finished(finished(&v))),
        "message" => assistant_text(&v).map(RunnerSignal::Output),
        _ => None,
    };
    Ok(signal.into_iter().collect())
}

fn assistant_text(v: &Value) -> Option<String> {
    let message = v.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn finished(v: &Value) -> FinishedSignal {
    let tokens = v.get("total_tokens").and_then(Value::as_i64);
    let cost_usd_micros = v
        .get("cost_usd")
        .and_then(Value::as_f64)
        .map(|dollars| (dollars * 1_000_000.0).round() as i64);
    FinishedSignal {
        outcome: Outcome::Ok,
        cost_usd_micros,
        tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use farseer_core::policy::{Budget, Irreversibility};
    use farseer_core::run::WorkspaceStrategy;
    use farseer_core::{CellId, RunId, TaskId};

    fn contract(goal: &str) -> WorkerContractSpec {
        WorkerContractSpec {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            goal: goal.into(),
            workspace: WorkspaceStrategy::Worktree,
            runner: "goose".into(),
            tool_grants: vec![],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "".into(),
        }
    }

    #[test]
    fn the_goal_arrives_via_the_text_flag_not_a_positional() {
        let args = build_args(&contract("fix the failing test"));
        let i = args.iter().position(|a| a == "-t").unwrap();
        assert_eq!(args[i + 1], "fix the failing test");
    }

    #[test]
    fn no_session_file_is_always_requested() {
        // Every run gets a fresh worktree; a goose session file tied to it
        // would outlive the workspace's own teardown for nothing.
        let args = build_args(&contract("anything"));
        assert!(args.contains(&"--no-session".to_string()));
    }

    #[test]
    fn a_complete_line_reports_ok_with_tokens_and_cost() {
        // This module's own 2026-08-24 probe, transcribed verbatim.
        let line = r#"{"type":"complete","total_tokens":6774,"input_tokens":6769,"output_tokens":5,"cache_read_input_tokens":0,"cache_write_input_tokens":0,"cost_usd":0.0013598000000000002}"#;
        let signals = parse_line(line).unwrap();
        assert_eq!(
            signals,
            [RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Ok,
                cost_usd_micros: Some(1_360),
                tokens: Some(6774),
            })]
        );
    }

    #[test]
    fn an_assistant_message_carries_the_text_the_manager_must_relay() {
        let line = r#"{"type":"message","message":{"id":"x","role":"assistant","content":[{"type":"text","text":"ok"}]}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            [RunnerSignal::Output("ok".into())]
        );
    }

    #[test]
    fn malformed_json_is_an_error_not_silence() {
        assert!(parse_line("not json").is_err());
    }
}
