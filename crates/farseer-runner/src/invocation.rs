//! Builds the argv for a Claude Code invocation.
//!
//! **Now includes `--input-format stream-json`.** The goal no longer arrives
//! as a positional prompt: `--input-format stream-json` puts Claude Code in
//! the mode [`crate::claude_code::steer_envelope`]'s 2026-08-23 probe
//! verified, so `farseer-manager` sends the goal as that same envelope's
//! first line on stdin, then can send further envelopes as steer messages
//! into the same live process - the seam this file's doc comment used to
//! call the remaining blocker.
//!
//! Also not yet mapped: `tool_grants` onto whatever permission flags Claude
//! Code exposes, and the workspace `cwd` trust prompt `10` found on two of
//! three runners. Both belong with the workspace lifecycle and the manager
//! loop that will call this.

use farseer_core::run::WorkerContractSpec;

/// `10`'s captured flags - `--output-format stream-json --verbose` - plus
/// `--input-format stream-json` for the now-verified steer envelope. No
/// positional prompt: the goal travels as that envelope's first line
/// instead, written by whichever caller drives the spawned process.
pub fn build_args(_contract: &WorkerContractSpec) -> Vec<String> {
    vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ]
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
            runner: "claude-code".into(),
            tool_grants: vec![],
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "".into(),
        }
    }

    #[test]
    fn the_goal_is_not_a_positional_argument() {
        let args = build_args(&contract("post a haiku about ferrous rust"));
        assert!(
            !args.contains(&"post a haiku about ferrous rust".to_string()),
            "the goal travels as the first stdin envelope, not argv"
        );
    }

    #[test]
    fn the_stream_json_flags_match_what_10_actually_ran_plus_input_format() {
        let args = build_args(&contract("anything"));
        assert!(
            args.windows(2)
                .any(|w| w == ["--output-format", "stream-json"])
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--input-format", "stream-json"]),
            "steer needs the process listening on stdin, per claude_code::steer_envelope"
        );
        assert!(args.contains(&"--verbose".to_string()));
    }
}
