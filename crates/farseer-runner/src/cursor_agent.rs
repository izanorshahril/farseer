//! The `cursor-agent` CLI runner: invocation and stream-json mapping.
//!
//! `10 runner inventory` scored it against `05 run state model`'s contract: activity **pass** (`thinking`
//! deltas, each with `timestamp_ms`), progress **pass** (`tool_call` with
//! `subtype: started`/`completed`), follow-up **fail** (no `--input-format`
//! flag exists; `--resume`/`--continue` start a new process, same shape as
//! Codex), cancel **not evaluated**. **Free-tier account on this machine** -
//! `10 runner inventory` found real quota is spent per run, so this runner's own tests below
//! use a captured fixture rather than a live process, same discipline as
//! `claude_code`'s and `codex`'s tests.
//!
//! `10 runner inventory` names the trust gate (`--trust`, same shape as Codex's
//! `--skip-git-repo-check`) and the shapes of `thinking`/`tool_call`/`result`
//! but captured no literal payload. This module's own probe, run
//! 2026-08-24 against the real, installed `cursor-agent` 2026.08.11-e8db854
//! with `--print --output-format stream-json --trust`, supplies that:
//!
//! ```text
//! {"type":"system","subtype":"init","apiKeySource":"login","cwd":"...","session_id":"...","model":"Auto","permissionMode":"default"}
//! {"type":"user","message":{"role":"user","content":[{"type":"text","text":"..."}]},"session_id":"..."}
//! {"type":"thinking","subtype":"delta","text":"...","session_id":"...","timestamp_ms":...}
//! {"type":"thinking","subtype":"completed","session_id":"...","timestamp_ms":...}
//! {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]},"session_id":"..."}
//! {"type":"result","subtype":"success","duration_ms":...,"duration_api_ms":...,"is_error":false,"result":"ok","session_id":"...","request_id":"...","usage":{"inputTokens":14228,"outputTokens":45,"cacheReadTokens":4992,"cacheWriteTokens":0}}
//! ```
//!
//! **`tool_call` stayed out of that probe** - the trivial prompt used to
//! capture it never triggered one - so, per `10 runner inventory`'s own rule and matching
//! `codex.rs`'s precedent, it counts as activity only until a real payload
//! is captured to map it to `TOOL_CALL_STARTED`/`TOOL_RESULT` properly.
//! `system` and `user`/`assistant` echo lines are activity only too: none of
//! them are one of `05 run state model`'s three progress kinds.
//!
//! **`is_error` is the terminal signal, not `subtype`** - `claude_code.rs`
//! found `subtype: "success"` can appear alongside `is_error: true` on that
//! runner, and cursor-agent's `result` event carries the same two fields, so
//! the same authority order applies here rather than assuming this probe's
//! one successful sample generalises.

use farseer_core::run::{Outcome, WorkerContractSpec};
use serde_json::Value;

use crate::claude_code::{FinishedSignal, ParseError, RunnerSignal};

/// The flags this module's own probe verified: `--print` for headless output,
/// `--output-format stream-json`, `--trust` to disarm `10 runner inventory`'s fresh-workspace
/// gate (`04 spike workspace teardown` gives every run a fresh worktree, so every run would hit it
/// without this flag), and the goal as the trailing positional prompt - no
/// steering path exists here, so there is no frame for it to travel as
/// instead, same reasoning as `codex::build_args`.
pub fn build_args(contract: &WorkerContractSpec) -> Vec<String> {
    vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--trust".into(),
        contract.goal.clone(),
    ]
}

pub fn parse_line(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
    let v: Value = serde_json::from_str(line).map_err(|e| ParseError(e.to_string()))?;
    let Some(kind) = v.get("type").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    if kind == "result" {
        let mut signals = Vec::new();
        if let Some(result) = v.get("result").and_then(Value::as_str) {
            signals.push(RunnerSignal::Output(result.to_string()));
        }
        signals.push(RunnerSignal::Finished(finished(&v)));
        return Ok(signals);
    }
    // `system`, `user`, `assistant`, `thinking`, and any `tool_call` are
    // activity only, per the module doc comment.
    Ok(Vec::new())
}

/// `10 runner inventory`: cursor-agent reports tokens only, never cost - there is no
/// `total_cost_usd` equivalent here, so a run's cost must be priced by
/// farseer itself from these fields, not read off the wire.
fn finished(v: &Value) -> FinishedSignal {
    let outcome = match v.get("is_error").and_then(Value::as_bool) {
        Some(true) => Outcome::Failed,
        Some(false) => Outcome::Ok,
        None => match v.get("subtype").and_then(Value::as_str) {
            Some("success") => Outcome::Ok,
            _ => Outcome::Failed,
        },
    };
    let tokens = v.get("usage").map(|usage| {
        [
            "inputTokens",
            "outputTokens",
            "cacheReadTokens",
            "cacheWriteTokens",
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
            runner: "cursor-agent".into(),
            tool_grants: vec![],
            tool_level: Default::default(),
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "".into(),
        }
    }

    #[test]
    fn the_fresh_workspace_trust_gate_is_always_disarmed() {
        // `10 runner inventory`: without this flag, every run fails on a fresh worktree while
        // looking like a hang to anything watching only for activity.
        let args = build_args(&contract("anything"));
        assert!(args.contains(&"--trust".to_string()));
    }

    #[test]
    fn the_goal_arrives_as_the_trailing_positional_prompt() {
        let args = build_args(&contract("fix the failing test"));
        assert_eq!(args.last().unwrap(), "fix the failing test");
    }

    #[test]
    fn a_successful_result_reports_ok_and_the_summed_token_usage() {
        // This module's own 2026-08-24 probe, transcribed verbatim.
        let line = r#"{"type":"result","subtype":"success","duration_ms":8318,"duration_api_ms":8318,"is_error":false,"result":"ok","session_id":"750eceea-1bcc-4482-b7ab-697cd7928325","request_id":"43d0263c-1f0c-4552-ac0c-f8b04ed616d9","usage":{"inputTokens":14228,"outputTokens":45,"cacheReadTokens":4992,"cacheWriteTokens":0}}"#;
        let signals = parse_line(line).unwrap();
        assert_eq!(
            signals,
            [
                RunnerSignal::Output("ok".into()),
                RunnerSignal::Finished(FinishedSignal {
                    outcome: Outcome::Ok,
                    cost_usd_micros: None,
                    tokens: Some(14228 + 45 + 4992),
                })
            ]
        );
    }

    #[test]
    fn is_error_overrides_a_misleading_success_subtype() {
        // Same authority order `claude_code.rs` found necessary: `subtype`
        // names the output format, `is_error` is the real outcome.
        let line = r#"{"type":"result","subtype":"success","is_error":true,"usage":{}}"#;
        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Finished(f)] = signals.as_slice() else {
            panic!("expected one Finished signal, got {signals:?}");
        };
        assert_eq!(f.outcome, Outcome::Failed);
    }

    #[test]
    fn a_thinking_delta_is_activity_only_and_yields_no_signal() {
        let line =
            r#"{"type":"thinking","subtype":"delta","text":"reasoning...","timestamp_ms":1}"#;
        assert_eq!(parse_line(line).unwrap(), Vec::new());
    }

    #[test]
    fn an_unrecognised_tool_call_is_activity_only_and_yields_no_signal() {
        // The exact case the module doc comment names: `10 runner inventory` describes the
        // shape but no ticket captured a literal payload, so this must not
        // be guessed at.
        let line = r#"{"type":"tool_call","subtype":"started","tool":"shell"}"#;
        assert_eq!(parse_line(line).unwrap(), Vec::new());
    }

    #[test]
    fn malformed_json_is_an_error_not_silence() {
        assert!(parse_line("not json").is_err());
    }
}
