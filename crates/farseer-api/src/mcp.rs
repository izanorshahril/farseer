//! [Record scope] fixes the MCP face as query and memory-write, never raw event append: "An agent that can forge events can rewrite its own history."
//! The memory half therefore offers [`FarseerMcp::read_memory`] and [`FarseerMcp::write_memory`], and nothing that appends an event.
//!
//! [`FarseerMcp::delegate_to_worker`] is the manager loop from [Which cells may a manager call, and does an instruction route or delegate?].
//! A live manager identifies itself with the `manager_run_id` injected into its system prompt.
//! The runtime requires an active manager plus its per-run capability, resolves the worker against the pinned roster, enforces the cell-wide worker cap, keeps the parent task and policy, caps and draws down budget, links cancellation to the owning manager, and returns terminal text plus outcome and spend.
//! `POST /v1/cells/{id}/instruct` gives a Claude Code manager a generated strict MCP config naming this endpoint after the listener binds.
//! Cross-cell delegation through a `kind = "cell"` roster entry and equivalent verified launch wiring for non-Claude managers remain open.
//!
//! The service is nested into the existing router, not a second process.
//! [Is the cell the right primitive?] gives farseer one API and [Store: SQLite edge tables and CTEs, or an embedded graph engine?] gives the record one writer by construction: one process and one `Store`.
//! A stdio MCP server would normally be spawned per client and would create a second process opening the same SQLite file.
//! The streamable-HTTP transport instead nests into [`crate::router`] at `/v1/mcp`, sharing `AppState`'s `Store` and the same loopback/token guard as every other route.
//!
//! Every tool is manager-scoped: `manager_run_id` plus its per-run capability resolves runtime-owned identity and a pinned definition rather than trusting a caller-supplied cell.
//!
//! [Record scope]: ../../../.scratch/farseer/issues/02-record-scope.md
//! [Which cells may a manager call, and does an instruction route or delegate?]: ../../../.scratch/farseer/issues/22-cell-addressing.md
//! [Is the cell the right primitive?]: ../../../.scratch/farseer/issues/01-cell-primitive.md
//! [Store: SQLite edge tables and CTEs, or an embedded graph engine?]: ../../../.scratch/farseer/issues/09-store-decision.md

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};

use farseer_core::policy::Budget;
use farseer_core::run::{WorkerContract, WorkerContractSpec};
use farseer_core::{MemoryTier, RosterEntry, RunId, Spend};
use farseer_store::{MemoryScope, NewMemory, StoreError};

use crate::{
    AppState, create_workspace, ensure_runner_authority, execute_run, now_ms,
    unenforceable_budget_dimension,
};

#[derive(Clone)]
pub struct FarseerMcp {
    state: Arc<AppState>,
    // `#[tool_handler]` dispatches through this field via macro-generated
    // code the dead-code lint does not trace back to a use, which is why
    // this needs the allow - `the_mcp_face_writes_reads_back_and_refuses_
    // the_global_tier`'s real `rmcp` client round trip is what actually
    // proves dispatch works, not this attribute.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl FarseerMcp {
    fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    fn authorize_manager(
        &self,
        manager_run_id: &str,
        manager_token: &str,
    ) -> Result<crate::ManagerContext, McpError> {
        let run_id = manager_run_id
            .parse::<RunId>()
            .map_err(|_| McpError::invalid_params("manager_run_id must be a UUID", None))?;
        let manager = self.state.manager(run_id).ok_or_else(|| {
            McpError::invalid_params(
                "manager_run_id is not an active manager run in this runtime",
                None,
            )
        })?;
        if !manager.manager_token.matches(manager_token) {
            return Err(McpError::invalid_params(
                "manager_token does not authorize this manager run",
                None,
            ));
        }
        Ok(manager)
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ReadMemoryArgs {
    /// The active manager identity injected into this manager's system prompt.
    manager_run_id: String,
    /// The per-manager capability injected beside `manager_run_id`.
    manager_token: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct WriteMemoryArgs {
    /// The active manager identity injected into this manager's system prompt.
    manager_run_id: String,
    /// The per-manager capability injected beside `manager_run_id`.
    manager_token: String,
    body: String,
    /// `cell_local` (the default, per `25 memory lifecycle`) or `run_local`. `global` is
    /// refused here - `25 memory lifecycle` gates it on the operator, a promotion this face
    /// does not offer.
    tier: Option<String>,
}

#[tool_router]
impl FarseerMcp {
    #[tool(
        description = "Read this manager's scoped memory: its pinned cell-local and opted-in claims, every global claim, and this run's run-local claims. Pass manager_run_id and manager_token. Marks each returned claim as consulted by the manager run."
    )]
    fn read_memory(
        &self,
        Parameters(args): Parameters<ReadMemoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let manager = self.authorize_manager(&args.manager_run_id, &args.manager_token)?;
        // `02 record scope`: identity comes from the active manager's pinned
        // definition, never from a caller-supplied cell id that could impersonate
        // another reader and silently gain its `also_read` grants.
        let scope = MemoryScope::from_definition(&manager.cell, Some(manager.contract.run_id));
        let claims = self
            .state
            .store()
            .read_memory(&scope)
            .map_err(store_error)?;
        let store = self.state.store();
        for claim in &claims {
            store
                .record_consulted(manager.contract.run_id, claim.memory_id, now_ms())
                .map_err(store_error)?;
        }
        let body = serde_json::json!(
            claims
                .iter()
                .map(|c| serde_json::json!({
                    "memory_id": c.memory_id.to_string(),
                    "tier": c.tier.as_str(),
                    "body": c.body,
                    "ts": c.ts,
                }))
                .collect::<Vec<_>>()
        );
        Ok(CallToolResult::success(vec![ContentBlock::text(
            body.to_string(),
        )]))
    }

    #[tool(
        description = "Write a memory claim for this manager's pinned cell or active run. Pass manager_run_id and manager_token. Defaults to cell-local; global promotion needs the operator and is refused here."
    )]
    fn write_memory(
        &self,
        Parameters(args): Parameters<WriteMemoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let manager = self.authorize_manager(&args.manager_run_id, &args.manager_token)?;
        let tier = match args.tier.as_deref() {
            None | Some("cell_local") => MemoryTier::CellLocal,
            Some("run_local") => MemoryTier::RunLocal,
            Some("global") => {
                return Err(McpError::invalid_params(
                    "the global tier is gated on the operator and cannot be written through this face - see ticket 25",
                    None,
                ));
            }
            Some(other) => {
                return Err(McpError::invalid_params(
                    format!("unknown memory tier `{other}`"),
                    None,
                ));
            }
        };
        let memory_id = self
            .state
            .store()
            .write_memory(&NewMemory {
                tier,
                cell_id: manager.contract.cell_id.clone(),
                run_id: (tier == MemoryTier::RunLocal).then_some(manager.contract.run_id),
                body: args.body,
                supersedes: Vec::new(),
                ts: now_ms(),
            })
            .map_err(store_error)?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            memory_id.to_string(),
        )]))
    }

    #[tool(
        description = "Delegate a precise sub-goal to a named worker in this manager's pinned roster and wait for it to finish. Pass manager_run_id and manager_token. Returns terminal text, outcome, cost and tokens."
    )]
    fn delegate_to_worker(
        &self,
        Parameters(args): Parameters<DelegateToWorkerArgs>,
    ) -> Result<CallToolResult, McpError> {
        let manager = self.authorize_manager(&args.manager_run_id, &args.manager_token)?;
        if manager
            .cancel_requested
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(McpError::invalid_params(
                "the owning manager is being cancelled",
                None,
            ));
        }
        if args.goal.trim().is_empty() {
            return Err(McpError::invalid_params("goal must not be empty", None));
        }
        let (runner, roster_budget) = manager
            .cell
            .roster
            .iter()
            .find_map(|entry| match entry {
                RosterEntry::Worker {
                    name,
                    runner,
                    max_budget,
                } if *name == args.worker => Some((runner.clone(), *max_budget)),
                _ => None,
            })
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!(
                        "`{}` is not a worker in `{}`'s pinned roster",
                        args.worker, manager.cell.cell_id
                    ),
                    None,
                )
            })?;
        ensure_runner_authority(&manager.cell, &runner).map_err(api_error)?;
        let _worker_permit = self
            .state
            .acquire_worker(&manager.cell.cell_id, manager.cell.policy.worker_cap)
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!(
                        "cell `{}` already has its maximum {} delegated workers in flight",
                        manager.cell.cell_id, manager.cell.policy.worker_cap
                    ),
                    None,
                )
            })?;
        let mut remaining_budget = manager
            .remaining_budget
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if manager
            .cancel_requested
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(McpError::invalid_params(
                "the owning manager is being cancelled",
                None,
            ));
        }
        if let Some(dimension) = exhausted_dimension(*remaining_budget) {
            return Err(McpError::invalid_params(
                format!("manager delegation budget is exhausted: {dimension}"),
                None,
            ));
        }
        let requested_budget = args.budget.map(Budget::from).unwrap_or_default();
        let effective_budget =
            effective_delegation_budget(*remaining_budget, roster_budget, requested_budget);
        if let Some(dimension) = unenforceable_budget_dimension(&runner, effective_budget) {
            return Err(McpError::invalid_params(
                format!(
                    "runner `{runner}` cannot enforce the delegated {dimension} budget before spending"
                ),
                None,
            ));
        }
        let run_id = RunId::new();
        manager
            .child_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(run_id);
        let _child_registration = ChildRunRegistration {
            child_runs: Arc::clone(&manager.child_runs),
            run_id,
        };
        let contract = WorkerContract::seal(WorkerContractSpec {
            run_id,
            task_id: manager.contract.task_id,
            cell_id: manager.contract.cell_id.clone(),
            goal: args.goal,
            workspace: manager.cell.workspace_strategy,
            runner,
            tool_grants: manager.contract.tool_grants.clone(),
            autonomy_ceiling: manager.contract.autonomy_ceiling,
            budget: effective_budget,
            definition_of_done: args.definition_of_done.unwrap_or_default(),
        });

        // The whole worker lifecycle blocks here - real minutes, not a
        // request/response tick - so `block_in_place` tells the multi-thread
        // runtime this task is stepping out of the async pool rather than
        // starving it, matching `main.rs`'s `new_multi_thread` builder.
        let started = std::time::Instant::now();
        let report = match tokio::task::block_in_place(|| {
            self.run_delegated_worker(contract, &manager.cell, &manager.cancel_requested)
        }) {
            Ok(report) => report,
            Err(error) => {
                exhaust_bounded_dimensions(&mut remaining_budget);
                return Err(error);
            }
        };
        if effective_budget.usd_micros.is_some() && report.cost_usd_micros.is_none() {
            exhaust_bounded_dimensions(&mut remaining_budget);
            return Err(McpError::internal_error(
                "the delegated runner did not report spend for its bounded currency budget",
                None,
            ));
        }
        let mut after = *remaining_budget;
        let spend = Spend {
            usd_micros: report.cost_usd_micros.unwrap_or(0).max(0) as u64,
            tokens: report.tokens.unwrap_or(0).max(0) as u64,
            wall_secs: started.elapsed().as_secs(),
        };
        if let Err(error) = after.draw(spend) {
            exhaust_bounded_dimensions(&mut remaining_budget);
            return Err(McpError::internal_error(error.to_string(), None));
        }
        *remaining_budget = after;

        let body = serde_json::json!({
            "run_id": run_id.to_string(),
            "task_id": manager.contract.task_id.to_string(),
            "outcome": outcome_str(report.outcome),
            "result": report.result,
            "cost_usd_micros": report.cost_usd_micros,
            "tokens": report.tokens,
        });
        Ok(CallToolResult::success(vec![ContentBlock::text(
            body.to_string(),
        )]))
    }

    /// Creates the workspace, runs the contract to completion, and tears the workspace down synchronously because the manager's turn is waiting for the answer.
    fn run_delegated_worker(
        &self,
        contract: WorkerContract,
        pinned_cell: &farseer_core::CellDefinition,
        cancel_requested: &std::sync::atomic::AtomicBool,
    ) -> Result<farseer_manager::RunReport, McpError> {
        let run_id = contract.run_id;
        let (cwd, repo_for_teardown) =
            create_workspace(&self.state, contract.workspace, run_id).map_err(api_error)?;

        let result = execute_run(
            &self.state,
            &contract,
            &cwd,
            &farseer_manager::RunOptions {
                actor: farseer_core::Actor::Manager,
                role: farseer_manager::RunRole::Worker,
                manager_cell: Some(pinned_cell.clone()),
                claude_mcp_config: None,
                claude_append_system_prompt: None,
            },
            Some(cancel_requested),
        );

        // `04 spike workspace teardown`: teardown starts only after
        // `run_worker` returns and the process has closed its stdout pipe, so
        // the cwd handle that blocks deletion is already gone.
        if let Err(e) =
            farseer_runner::workspace::teardown_workspace(&cwd, repo_for_teardown.as_deref())
        {
            eprintln!("workspace teardown for delegated run {run_id} did not complete: {e}");
        }

        preserve_cancelled_report(result)
    }
}

/// Cancellation is a terminal worker report, not an MCP transport failure.
/// The preserved spend is drawn from the manager's remaining pool by the same
/// path as every other terminal outcome.
fn preserve_cancelled_report(
    result: Result<farseer_manager::RunReport, farseer_manager::ManagerError>,
) -> Result<farseer_manager::RunReport, McpError> {
    match result {
        Ok(report) | Err(farseer_manager::ManagerError::Cancelled(report)) => Ok(report),
        Err(error) => Err(McpError::internal_error(error.to_string(), None)),
    }
}

fn outcome_str(outcome: farseer_core::run::Outcome) -> &'static str {
    match outcome {
        farseer_core::run::Outcome::Ok => "ok",
        farseer_core::run::Outcome::Failed => "failed",
        farseer_core::run::Outcome::Cancelled => "cancelled",
        farseer_core::run::Outcome::Abandoned => "abandoned",
    }
}

fn api_error(e: crate::ApiError) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct DelegateToWorkerArgs {
    /// The active manager identity injected into this manager's system prompt.
    manager_run_id: String,
    /// The per-manager capability injected beside `manager_run_id`.
    manager_token: String,
    /// Must name a `kind = "worker"` entry in the manager's pinned roster.
    worker: String,
    goal: String,
    definition_of_done: Option<String>,
    /// Optional requested child cap, always narrowed by the parent contract.
    budget: Option<DelegationBudgetArgs>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct DelegationBudgetArgs {
    usd_micros: Option<u64>,
    tokens: Option<u64>,
    wall_secs: Option<u64>,
}

impl From<DelegationBudgetArgs> for Budget {
    fn from(value: DelegationBudgetArgs) -> Self {
        Self {
            usd_micros: value.usd_micros,
            tokens: value.tokens,
            wall_secs: value.wall_secs,
        }
    }
}

struct ChildRunRegistration {
    child_runs: Arc<std::sync::Mutex<std::collections::HashSet<RunId>>>,
    run_id: RunId,
}

impl Drop for ChildRunRegistration {
    fn drop(&mut self) {
        self.child_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.run_id);
    }
}

/// `23 prototype loose ends`: definition root -> roster cap -> explicit request -> caller remaining, with every layer able only to narrow.
fn effective_delegation_budget(remaining: Budget, roster_cap: Budget, requested: Budget) -> Budget {
    remaining.cap_to(roster_cap).cap_to(requested)
}

fn exhausted_dimension(budget: Budget) -> Option<&'static str> {
    if budget.usd_micros == Some(0) {
        Some("usd")
    } else if budget.tokens == Some(0) {
        Some("tokens")
    } else if budget.wall_secs == Some(0) {
        Some("wall_secs")
    } else {
        None
    }
}

fn exhaust_bounded_dimensions(budget: &mut Budget) {
    budget.usd_micros = budget.usd_micros.map(|_| 0);
    budget.tokens = budget.tokens.map(|_| 0);
    budget.wall_secs = budget.wall_secs.map(|_| 0);
}

fn store_error(e: StoreError) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

#[tool_handler]
impl ServerHandler for FarseerMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "farseer's MCP face: read_memory, write_memory, and manager-scoped \
                 delegate_to_worker. Raw events are never appended through here."
                    .to_string(),
            )
    }
}

/// Nested at `/v1/mcp` by [`crate::router`], behind the same loopback/token
/// guard as the rest of the API - the `.layer(...)` there wraps everything
/// built before it, this route included.
pub fn service(state: Arc<AppState>) -> StreamableHttpService<FarseerMcp, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(FarseerMcp::new(Arc::clone(&state))),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use farseer_core::run::Outcome;

    #[test]
    fn a_worker_budget_is_narrowed_by_roster_request_and_remaining_pool() {
        let effective = effective_delegation_budget(
            Budget {
                usd_micros: Some(5_000_000),
                tokens: Some(100_000),
                wall_secs: None,
            },
            Budget {
                usd_micros: Some(2_000_000),
                tokens: None,
                wall_secs: Some(600),
            },
            Budget {
                usd_micros: Some(3_000_000),
                tokens: Some(40_000),
                wall_secs: Some(300),
            },
        );

        assert_eq!(effective.usd_micros, Some(2_000_000));
        assert_eq!(effective.tokens, Some(40_000));
        assert_eq!(effective.wall_secs, Some(300));
    }

    #[test]
    fn a_cancelled_worker_keeps_its_terminal_report_for_mcp_drawdown() {
        let report = farseer_manager::RunReport {
            outcome: Outcome::Cancelled,
            cost_usd_micros: Some(123_456),
            tokens: Some(789),
            result: Some("terminal text".into()),
        };

        let preserved =
            preserve_cancelled_report(Err(farseer_manager::ManagerError::Cancelled(report)))
                .unwrap();

        assert_eq!(preserved.outcome, Outcome::Cancelled);
        assert_eq!(preserved.cost_usd_micros, Some(123_456));
        assert_eq!(preserved.tokens, Some(789));
        assert_eq!(preserved.result.as_deref(), Some("terminal text"));
    }
}
