//! Builds the argv for a Claude Code invocation.
//!
//! `--input-format stream-json` puts Claude Code in the mode [`crate::claude_code::steer_frame`]'s 2026-08-23 probe verified.
//! The goal travels as the first stdin frame and later steer messages use the same live process.
//!
//! A manager launch may also receive farseer's generated MCP config through `--mcp-config` and `--strict-mcp-config`.
//! `10 runner inventory` records the disposable Claude Code 2.1.233 probe which generated the exact HTTP shape: `{"mcpServers":{"farseer":{"type":"http","url":"http://127.0.0.1:<port>/v1/mcp","headers":{"Authorization":"Bearer <token>"}}}}`.
//! Worker launches receive no such config, so only an active manager can reach `delegate_to_worker` through this path.
//!
//! `10 runner inventory` measured that `--max-budget-usd 0.000001` still reported `$0.131195` of spend before returning `error_max_budget_usd`.
//! The flag is therefore not a pre-spend contract boundary and is deliberately absent here; the API refuses bounded currency budgets for native runners instead.
//!
//! Mapping arbitrary `tool_grants` onto Claude Code's built-in permission flags remains open.
//! The API launches the current native LLM runners only when the pinned cell explicitly grants a shell-capable tool, because `12 autonomy and deny list` says that grant means all tools.

use std::path::Path;

use farseer_core::run::WorkerContractSpec;

/// Process-local options which are not part of the immutable worker contract.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeCodeLaunch<'a> {
    /// Managers stay open for later steer messages; workers exit after one goal.
    pub live_input: bool,
    /// A config generated for this process after the API knows its bound port.
    pub mcp_config: Option<&'a Path>,
    /// Manager identity and roster guidance, kept out of the operator's goal.
    pub append_system_prompt: Option<&'a str>,
}

/// The captured stream-json flags plus optional manager-only MCP wiring.
/// The MCP tools a farseer manager may call without a permission prompt.
///
/// `10 runner inventory` recorded that `--allowedTools` is **not** an exclusive
/// allowlist - a manager keeps its built-in tools regardless - so this list
/// grants rather than restricts, and a tool missing from it is a manager that
/// hangs waiting for an answer.
pub const MANAGER_ALLOWED_TOOLS: [&str; 4] = [
    "mcp__farseer__delegate_to_worker",
    "mcp__farseer__delegate_to_cell",
    "mcp__farseer__read_memory",
    "mcp__farseer__write_memory",
];

pub fn build_args(contract: &WorkerContractSpec, launch: ClaudeCodeLaunch<'_>) -> Vec<String> {
    let mut args = vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    if launch.live_input {
        args.extend(["--input-format".into(), "stream-json".into()]);
    }

    if let Some(config) = launch.mcp_config {
        args.extend([
            "--mcp-config".into(),
            config.to_string_lossy().into_owned(),
            "--strict-mcp-config".into(),
            "--allowedTools".into(),
            // Every tool the MCP face exposes has to be named here or the
            // manager stalls on a permission prompt no operator is watching -
            // observed live on 2026-08-25 when `delegate_to_cell` shipped
            // without it: "Claude requested permissions to use
            // mcp__farseer__delegate_to_cell, but you haven't granted it yet."
            MANAGER_ALLOWED_TOOLS.join(","),
        ]);
    }
    if let Some(prompt) = launch.append_system_prompt {
        args.extend(["--append-system-prompt".into(), prompt.into()]);
    }
    if !launch.live_input {
        args.push(contract.goal.clone());
    }
    args
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
        let args = build_args(
            &contract("post a haiku about ferrous rust"),
            ClaudeCodeLaunch {
                live_input: true,
                ..ClaudeCodeLaunch::default()
            },
        );
        assert!(
            !args.contains(&"post a haiku about ferrous rust".to_string()),
            "the goal travels as the first stdin frame, not argv"
        );
    }

    #[test]
    fn a_worker_gets_one_positional_goal_and_no_live_stdin_mode() {
        let args = build_args(&contract("finish once"), ClaudeCodeLaunch::default());
        assert_eq!(args.last().unwrap(), "finish once");
        assert!(!args.contains(&"--input-format".to_string()));
    }

    #[test]
    fn the_stream_json_flags_match_what_the_runner_inventory_actually_ran_plus_input_format() {
        let args = build_args(
            &contract("anything"),
            ClaudeCodeLaunch {
                live_input: true,
                ..ClaudeCodeLaunch::default()
            },
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--output-format", "stream-json"])
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--input-format", "stream-json"]),
            "steer needs the process listening on stdin, per claude_code::steer_frame"
        );
        assert!(args.contains(&"--verbose".to_string()));
    }

    #[test]
    fn a_currency_budget_is_not_misrepresented_as_pre_spend_enforcement() {
        let mut contract = contract("bounded");
        contract.budget.usd_micros = Some(2_500_001);
        let args = build_args(&contract, ClaudeCodeLaunch::default());
        assert!(
            !args.contains(&"--max-budget-usd".to_string()),
            "the installed Claude version reports this cap only after overspending it"
        );
    }

    #[test]
    fn a_manager_launch_uses_only_its_generated_mcp_config() {
        let args = build_args(
            &contract("delegate this"),
            ClaudeCodeLaunch {
                live_input: true,
                mcp_config: Some(Path::new(r"C:\runs\manager\farseer-mcp.json")),
                append_system_prompt: Some("manager context"),
            },
        );
        assert!(
            args.windows(2)
                .any(|w| { w == ["--mcp-config", r"C:\runs\manager\farseer-mcp.json",] })
        );
        assert!(args.contains(&"--strict-mcp-config".to_string()));
        assert!(
            args.windows(2)
                .any(|w| w == ["--append-system-prompt", "manager context"])
        );
    }
}
