//! Builds the argv for a one-shot Claude Code invocation from a worker
//! contract's goal.
//!
//! **Deliberately narrower than `10`'s full contract-test finding.** `10`
//! measured that `--input-format stream-json` accepts follow-up turns into a
//! live process, session intact - the thing that makes Claude Code pass `20`'s
//! corrected steering test. That flag is **not** included here, because
//! neither `10` nor `20` captured the JSON envelope an *initial* message takes
//! under `--input-format stream-json`, and guessing a wire format contradicts
//! `10`'s own rule: **what a runner exposes must be observed, not read off a
//! page.** Until that envelope is observed, this builds the well-documented
//! one-shot form instead - the goal as a positional prompt - which cannot
//! steer mid-run and does not need to.
//!
//! Also not yet mapped: `tool_grants` onto whatever permission flags Claude
//! Code exposes, and the workspace `cwd` trust prompt `10` found on two of
//! three runners. Both belong with the workspace lifecycle and the manager
//! loop that will call this.

use farseer_core::run::WorkerContractSpec;

/// The flags `10` itself ran with to capture the payloads `claude_code`
/// parses: `claude -p --output-format stream-json --verbose`, plus the
/// documented `--print` long form and the goal as the trailing prompt.
pub fn build_args(contract: &WorkerContractSpec) -> Vec<String> {
    vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        contract.goal.clone(),
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
    fn the_goal_arrives_as_the_trailing_positional_prompt() {
        let args = build_args(&contract("post a haiku about ferrous rust"));
        assert_eq!(args.last().unwrap(), "post a haiku about ferrous rust");
    }

    #[test]
    fn the_output_format_flag_matches_what_10_actually_ran() {
        let args = build_args(&contract("anything"));
        assert!(
            args.windows(2)
                .any(|w| w == ["--output-format", "stream-json"])
        );
        assert!(args.contains(&"--verbose".to_string()));
    }
}
