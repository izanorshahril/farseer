//! The Codex CLI runner: invocation and stream-json mapping.
//!
//! `20 worker control channel` scored `codex exec --json` against `05 run state model`'s contract: activity **pass**
//! (`item.*` including reasoning), progress **pass** (`turn.*` and `item.*`
//! including command execution, file changes, plan updates), follow-up
//! **fail** (`codex exec resume` replays into a **new process**, per `10 runner inventory` -
//! not steering), cancel **weak** (`turn.failed` does not distinguish a
//! cancel from an error, which does not matter here since farseer's own Job
//! Object kill never depended on Codex agreeing about why it died).
//!
//! `10 runner inventory`: **`codex exec` refuses a fresh directory** without
//! `--skip-git-repo-check`, printing a trust-gate error and exiting. `04 spike workspace teardown`
//! gives every run a fresh worktree, so every run hits this without the flag,
//! and the exact failure mode `10 runner inventory` warns reads as a hang to anything
//! watching only for activity.
//!
//! Progress mapping remains intentionally shallow.
//! `10 runner inventory` records the 2026-08-25 local Codex probe which captured one answer shape exactly: `{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"codex-ok"}}`.
//! That shape becomes terminal text for a supervising manager.
//! Other `item.*` lines, including tool calls, remain activity-only until a literal payload is captured.

use farseer_core::run::{Outcome, WorkerContractSpec};
use serde_json::Value;

use crate::claude_code::{FinishedSignal, ParseError, RunnerSignal};

/// `10 runner inventory`'s own tested invocation, plus `--json` for machine-readable output
/// and the goal as the trailing prompt. Unlike `invocation::build_args`,
/// this keeps the positional goal: Codex has no steering path (`codex exec
/// resume` starts a new process rather than continuing this one, per `10 runner inventory`),
/// so there is no frame for the goal to travel as instead.
pub fn build_args(contract: &WorkerContractSpec) -> Vec<String> {
    vec![
        "exec".into(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        contract.goal.clone(),
    ]
}

pub fn parse_line(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
    let v: Value = serde_json::from_str(line).map_err(|e| ParseError(e.to_string()))?;
    let Some(kind) = v.get("type").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let signal = match kind {
        // Codex names a thread and never a model, so that is what farseer
        // reports. Filling the gap with the model somebody configured would be
        // farseer answering a question the runner declined.
        "thread.started" => v.get("thread_id").and_then(Value::as_str).map(|thread| {
            RunnerSignal::Session(crate::claude_code::SessionInfo {
                model: None,
                session_id: Some(thread.to_string()),
                provider: None,
                configured: None,
            })
        }),
        "turn.completed" => Some(RunnerSignal::Finished(finished(&v, Outcome::Ok))),
        "turn.failed" => Some(RunnerSignal::Finished(finished(&v, Outcome::Failed))),
        "item.completed" => agent_message(&v).map(RunnerSignal::Output),
        // Other `item.*` shapes are activity only, per the module doc comment.
        _ => None,
    };
    Ok(signal.into_iter().collect())
}

fn agent_message(v: &Value) -> Option<String> {
    let item = v.get("item")?;
    if item.get("type").and_then(Value::as_str) != Some("agent_message") {
        return None;
    }
    item.get("text").and_then(Value::as_str).map(str::to_string)
}

/// `10 runner inventory`: Codex reports tokens only, never cost - `total_cost_usd` has no
/// equivalent here, so a Codex run's cost must be priced by farseer itself
/// from these fields, not read off the wire.
fn finished(v: &Value, outcome: Outcome) -> FinishedSignal {
    let tokens = v.get("usage").map(|usage| {
        [
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
        ]
        .iter()
        .filter_map(|field| usage.get(*field).and_then(Value::as_i64))
        .sum()
    });
    FinishedSignal {
        outcome,
        cost_usd_micros: None,
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
            runner: "codex".into(),
            tool_grants: vec![],
            tool_level: Default::default(),
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "".into(),
        }
    }

    #[test]
    fn the_fresh_directory_trust_gate_is_always_disarmed() {
        // `10 runner inventory`: without this flag, every run fails on a fresh worktree while
        // looking like a hang to anything watching only for activity.
        let args = build_args(&contract("anything"));
        assert!(args.contains(&"--skip-git-repo-check".to_string()));
    }

    #[test]
    fn the_goal_arrives_as_the_trailing_positional_prompt() {
        let args = build_args(&contract("fix the failing test"));
        assert_eq!(args.last().unwrap(), "fix the failing test");
    }

    #[test]
    fn a_completed_turn_is_ok_with_summed_token_usage() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":10,"cached_input_tokens":2,"output_tokens":5,"reasoning_output_tokens":3}}"#;
        let signals = parse_line(line).unwrap();
        assert_eq!(
            signals,
            [RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Ok,
                cost_usd_micros: None,
                tokens: Some(20),
            })]
        );
    }

    #[test]
    fn a_failed_turn_is_failed_not_cancelled() {
        // `05 run state model`: only a human choosing not to proceed is `Cancelled`. Codex's
        // own `turn.failed` cannot tell that apart from a real error - `20 worker control channel`
        // scored this "weak" - so it must never be read as more than `Failed`.
        let line = r#"{"type":"turn.failed"}"#;
        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Finished(f)] = signals.as_slice() else {
            panic!("expected one Finished signal, got {signals:?}");
        };
        assert_eq!(f.outcome, Outcome::Failed);
        assert_eq!(
            f.cost_usd_micros, None,
            "Codex never reports cost, only tokens"
        );
    }

    #[test]
    fn a_completed_agent_message_carries_the_text_the_manager_must_relay() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"codex-ok"}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            [RunnerSignal::Output("codex-ok".into())]
        );
    }

    #[test]
    fn an_unverified_item_shape_is_activity_only() {
        let line = r#"{"type":"item.command_execution","command":"ls"}"#;
        assert_eq!(parse_line(line).unwrap(), Vec::new());
    }

    #[test]
    fn malformed_json_is_an_error_not_silence() {
        assert!(parse_line("not json").is_err());
    }
}
