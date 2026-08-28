//! [Record scope] fixes the MCP face as query and memory-write, never raw event append: "An agent that can forge events can rewrite its own history."
//! The memory half therefore offers [`FarseerMcp::read_memory`] and [`FarseerMcp::write_memory`], and nothing that appends an event.
//!
//! [`FarseerMcp::delegate_to_worker`] is the manager loop from [Which cells may a manager call, and does an instruction route or delegate?].
//! A live manager identifies itself with the `manager_run_id` injected into its system prompt.
//! The runtime requires an active manager plus its per-run capability, resolves the worker against the pinned roster, enforces the cell-wide worker cap, keeps the parent task and policy, caps and draws down budget, links cancellation to the owning manager, and returns terminal text plus outcome and spend.
//! `POST /v1/cells/{id}/instruct` gives a Claude Code manager a generated strict MCP config naming this endpoint after the listener binds.
//! [`FarseerMcp::delegate_to_cell`] is the cross-cell half, through a `kind = "cell"` roster entry.
//! It is fire-and-forget per [What transport carries a cell-to-cell call?]: the caller gets a `call_id` and the callee's `run_id` at once, the callee's own manager runs in the callee's cell with the callee's workspace, runner and tool grants, and the caller keeps the task id so cost nests instead of detaching.
//! The caller's budget is **reserved** rather than drawn, because a fire-and-forget call has no terminal spend to draw when it returns.
//! Equivalent verified launch wiring for non-Claude managers remains open.
//!
//! [What transport carries a cell-to-cell call?]: ../../../.scratch/farseer/issues/06-cell-transport.md
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

use farseer_core::policy::{Budget, Irreversibility};
use farseer_core::run::{WorkerContract, WorkerContractSpec};
use farseer_core::{
    Actor, CallId, CellCall, EventKind, MemoryTier, NewEvent, RosterEntry, RunId, Spend,
};
use farseer_store::{MemoryScope, NewMemory, StoreError};

use crate::{
    AppState, RunRole, create_workspace, ensure_runner_authority, execute_run, now_ms, spawn_run,
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
    pub(crate) fn new(state: Arc<AppState>) -> Self {
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
        Ok(CallToolResult::success(vec![ContentBlock::text(
            self.delegate_to_worker_json(args)?.to_string(),
        )]))
    }

    #[tool(
        description = "Call another cell named in this manager's pinned roster. Fire-and-forget: returns a call_id and the callee's run_id immediately, and the result arrives on the event stream. The callee owns its own workspace, runner and tool grants; you state the goal, the ceiling and the budget."
    )]
    fn delegate_to_cell(
        &self,
        Parameters(args): Parameters<DelegateToCellArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text(
            self.delegate_to_cell_json(args)?.to_string(),
        )]))
    }

    /// Creates the workspace, runs the contract to completion, and tears the workspace down synchronously because the manager's turn is waiting for the answer.
    fn run_delegated_worker(
        &self,
        contract: WorkerContract,
        pinned_cell: &farseer_core::CellDefinition,
        cancel_requested: &std::sync::atomic::AtomicBool,
        // The roster entry's own skill directories, already resolved and
        // checked by the caller - this function has the contract but not the
        // roster entry it came from.
        skill_dirs: &[std::path::PathBuf],
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
                append_system_prompt: None,
        runner_env: Vec::new(),
        mcp: None,
        extensions: Vec::new(),
                account: Some(self.state.runner_config().account_for(&contract.runner)),
                usd_micros_per_mtok: self.state.runner_config().price_for(&contract.runner),
                skills: skill_dirs.to_vec(),
                // What the operator pinned, or nothing at all. `30 codex app
                // server`: farseer passes a model or an effort only when a
                // person wrote one down, so an unpinned runner keeps whatever
                // its own config says.
                model: self.state.runner_config().launch_of(&contract.runner).0.map(str::to_string),
                effort: self.state.runner_config().launch_of(&contract.runner).1.map(str::to_string),
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

/// The two delegation verbs, as plain JSON in and plain JSON out.
///
/// Lifted out of the `#[tool]` wrappers by `31 manager delegation reach`: the
/// MCP face is one way to ask for a delegation and it is not the only one, so
/// the logic cannot live inside the transport that happened to be first.
/// `/v1/manager/delegate/*` calls exactly these, which is what lets a runner
/// with no MCP client delegate on the same terms as one that has it.
impl FarseerMcp {
    pub(crate) fn delegate_to_worker_json(
        &self,
        args: DelegateToWorkerArgs,
    ) -> Result<serde_json::Value, McpError> {
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
        let (candidates, roster_budget, skill_names, tool_level) = manager
            .cell
            .roster
            .iter()
            .find_map(|entry| match entry {
                RosterEntry::Worker {
                    name,
                    runners,
                    max_budget,
                    skills,
                    tools,
                } if *name == args.worker => {
                    // The roster entry's own skills and tool level, not the
                    // manager's: `22 cell addressing` made the roster the grant,
                    // and a reviewer has no business holding what a coder needs.
                    Some((runners.clone(), *max_budget, skills.clone(), *tools))
                }
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
        // `26 routing policy`: the author's order, first candidate whose window
        // is not spent. Every runner farseer cannot see a window for reports
        // `unknown`, which behaves as **available until proven otherwise** -
        // most runners will never report one, so that is the permanent normal
        // case rather than a shim.
        let Some(runner) = crate::first_available_runner(&self.state, &candidates) else {
            // `26 routing policy` section 3 asked for `failed` with reason
            // `runner_exhausted`, on the precedent of `17 cell lifecycle`'s
            // orphaned run. **This departs from that, and deliberately.**
            //
            // `17`'s run had started - a process existed and its outcome was
            // genuinely unknown. Nothing starts here: no contract is sealed, no
            // workspace is made, no process is spawned. Writing a run row would
            // put a run in `11 analytics questions`'s denominators that never
            // ran, to record an event that is not about a run at all.
            //
            // So it is an event, on the manager's own run, and the operator's
            // question - how often did exhaustion block work - is a scan of
            // these rather than an outcome filter over phantom rows.
            //
            // The consequence `26` wanted falls out for free: with no run and no
            // rescope edge, `runner_exhausted` cannot reach rework depth, so the
            // exclusion it asked for is true by construction rather than by a
            // rule somebody has to remember.
            let _ = self.state.store().append(&NewEvent::new(
                manager.cell.cell_id.clone(),
                manager.contract.run_id,
                EventKind::new(EventKind::STATUS_CHANGED),
                Actor::System,
                now_ms(),
                serde_json::json!({
                    "routing": "runner_exhausted",
                    "worker": args.worker,
                    "candidates": candidates,
                }),
            ));
            return Err(McpError::internal_error(
                format!(
                    "runner_exhausted: every runner for worker `{}` has a spent window ({candidates:?})",
                    args.worker
                ),
                None,
            ));
        };
        if candidates.first() != Some(&runner) {
            // `26` section 4: **a reorder emits an event.** Without it,
            // `11 analytics questions`'s cost-by-runner cannot explain why a
            // non-preferred runner ran.
            let _ = self.state.store().append(&NewEvent::new(
                manager.cell.cell_id.clone(),
                manager.contract.run_id,
                EventKind::new(EventKind::STATUS_CHANGED),
                Actor::System,
                now_ms(),
                serde_json::json!({
                    // Which of `26 routing policy`'s two pressures moved it is
                    // not distinguishable here, and saying so is better than
                    // naming one: `11 analytics questions` needs to know a
                    // reorder happened and what it chose, and a confident wrong
                    // reason would be worse than an honest vague one.
                    "routing": "preferred_runner_unavailable",
                    "worker": args.worker,
                    "preferred": candidates.first(),
                    "chosen": runner,
                }),
            ));
        }
        // Resolved here, where the roster entry is in hand: a worker that names
        // a skill farseer cannot find is a refusal at delegation time rather
        // than a worse answer later for a reason nobody can see.
        if tool_level != farseer_core::ToolLevel::Shell
            && !farseer_runner::pi::takes_tool_allowlist(&runner)
        {
            return Err(McpError::invalid_params(
                format!(
                    "worker `{}` runs on `{runner}`, which farseer cannot hold to a tool                      allowlist, and its roster entry asks for `{}`",
                    args.worker,
                    tool_level.as_str()
                ),
                None,
            ));
        }
        if !skill_names.is_empty() && !crate::runner_loads_skills(&runner) {
            return Err(McpError::invalid_params(
                format!(
                    "worker `{}` runs on `{runner}`, which farseer cannot hand a skill by name, \
                     and its roster entry declares {skill_names:?}",
                    args.worker
                ),
                None,
            ));
        }
        let skill_dirs = skill_names
            .iter()
            .map(|name| crate::skill_dir(self.state.repo_root(), name))
            .collect::<Vec<_>>();
        if let Some(missing) = skill_dirs.iter().find(|dir| !dir.join("SKILL.md").is_file()) {
            return Err(McpError::invalid_params(
                format!("skill `{}` is not in this repository", missing.display()),
                None,
            ));
        }
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
            tool_level,
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
            self.run_delegated_worker(
                contract,
                &manager.cell,
                &manager.cancel_requested,
                &skill_dirs,
            )
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
        Ok(body)
    }

    pub(crate) fn delegate_to_cell_json(
        &self,
        args: DelegateToCellArgs,
    ) -> Result<serde_json::Value, McpError> {
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

        // `22 cell addressing` section 3: an ungranted cell stays ungranted even when
        // the operator names it, because a mechanism that yields to
        // conversational pressure is not a mechanism. The fix is to edit the
        // definition and reload, which takes ten seconds and leaves a commit.
        let (to_cell, roster_budget, peer) = manager
            .cell
            .roster
            .iter()
            .find_map(|entry| match entry {
                RosterEntry::Cell {
                    name,
                    cell_id,
                    max_budget,
                    peer,
                    ..
                } if *name == args.cell => Some((cell_id.clone(), *max_budget, *peer)),
                _ => None,
            })
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!(
                        "`{}` is not a callable cell in `{}`'s pinned roster",
                        args.cell, manager.cell.cell_id
                    ),
                    None,
                )
            })?;
        if peer {
            // `06 cell transport` section 2 keeps the A2A endpoint off by default and
            // nothing has turned it on. Refusing here beats mapping a call onto
            // a boundary that is not listening.
            return Err(McpError::invalid_params(
                format!(
                    "`{}` is a foreign A2A peer, and the A2A endpoint is off",
                    args.cell
                ),
                None,
            ));
        }
        let callee = self.state.cells().get(&to_cell).cloned().ok_or_else(|| {
            McpError::invalid_params(
                format!("roster names cell `{to_cell}`, which no definition declares"),
                None,
            )
        })?;

        // `22 cell addressing` section 4 caps the grant at the roster entry, `06 cell
        // transport` lets a caller cap but never raise, and the callee's own
        // policy is the floor neither of them can lift.
        let offered = match args.autonomy_ceiling.as_deref() {
            Some(level) => parse_ceiling(level)?,
            None => manager.contract.autonomy_ceiling,
        };
        let ceiling = manager
            .cell
            .ceiling_for_cell_call(&args.cell, offered)
            .ok_or_else(|| {
                McpError::invalid_params(format!("`{}` is not a callable cell", args.cell), None)
            })?
            .min(callee.policy.autonomy_ceiling);

        let mut remaining_budget = manager
            .remaining_budget
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(dimension) = exhausted_dimension(*remaining_budget) {
            return Err(McpError::invalid_params(
                format!("manager delegation budget is exhausted: {dimension}"),
                None,
            ));
        }
        let requested = args.budget.map(Budget::from).unwrap_or_default();
        let budget = effective_delegation_budget(*remaining_budget, roster_budget, requested)
            .cap_to(callee.budget);

        let call_id = CallId::new();
        let call = CellCall {
            call_id,
            from_cell: manager.cell.cell_id.clone(),
            to_cell: to_cell.clone(),
            goal: args.goal.clone(),
            autonomy_ceiling: ceiling,
            budget,
            definition_of_done: args.definition_of_done.clone().unwrap_or_default(),
            deadline_ms: args.deadline_ms,
        };

        let contract = cell_call_contract(&call, &callee, manager.contract.task_id);

        // Reserved, not drawn: `06 cell transport` made this fire-and-forget, so
        // there is no terminal spend to draw when the call returns. Reserving
        // the cap up front is the only way the caller's pool still bounds a
        // second call, and over-reserving fails closed where under-reserving
        // does not.
        reserve(&mut remaining_budget, budget);
        drop(remaining_budget);

        let run_id = match spawn_run(&self.state, contract, RunRole::Manager, callee) {
            Ok(run_id) => run_id,
            Err(error) => {
                let mut pool = manager
                    .remaining_budget
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                release(&mut pool, budget);
                return Err(api_error(error));
            }
        };

        // The caller's own record entry for the call, per `06 cell transport`
        // section 6, and the link `02 record scope` left open between a
        // caller's entry and its callee's.
        self.state
            .store()
            .append(&NewEvent::new(
                manager.cell.cell_id.clone(),
                manager.contract.run_id,
                EventKind::CELL_CALLED,
                Actor::Manager,
                now_ms(),
                serde_json::json!({
                    "call": call,
                    "callee_run_id": run_id.to_string(),
                }),
            ))
            .map_err(store_error)?;

        let body = serde_json::json!({
            "call_id": call_id.to_string(),
            "run_id": run_id.to_string(),
            "to_cell": to_cell.as_str(),
            "autonomy_ceiling": ceiling.as_str(),
        });
        Ok(body)
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
pub(crate) struct DelegateToWorkerArgs {
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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct DelegateToCellArgs {
    /// The active manager identity injected into this manager's system prompt.
    manager_run_id: String,
    /// The per-manager capability injected beside `manager_run_id`.
    manager_token: String,
    /// Must name a `kind = "cell"` entry in the manager's pinned roster.
    cell: String,
    goal: String,
    definition_of_done: Option<String>,
    /// `reversible`, `undoable` or `irreversible`. A ceiling only ever
    /// narrows: naming a level above the roster entry or the callee's own policy
    /// lowers to theirs rather than raising to yours.
    autonomy_ceiling: Option<String>,
    /// Optional requested cap, always narrowed by the roster entry and the
    /// caller's remaining pool.
    budget: Option<DelegationBudgetArgs>,
    /// Unix milliseconds. Absent means the caller set no deadline.
    deadline_ms: Option<i64>,
}

/// What the callee actually runs, built from the call plus the callee's own definition.
///
/// The asymmetry against `delegate_to_worker` is the whole of `06 cell transport`
/// section 4: **a manager-to-worker contract names the workspace, the runner and
/// the tool grants, and a manager-to-cell contract must not.** All three come
/// from the callee here, and that ownership is what makes it a cell rather than
/// a worker.
fn cell_call_contract(
    call: &CellCall,
    callee: &farseer_core::CellDefinition,
    task_id: farseer_core::TaskId,
) -> WorkerContract {
    WorkerContract::seal(WorkerContractSpec {
        run_id: RunId::new(),
        // `22 cell addressing` section 2: one task, one owner. The task stays
        // with the caller so `11 analytics questions`'s cost nests under it
        // rather than detaching into a second task nobody asked for.
        task_id,
        cell_id: callee.cell_id.clone(),
        goal: call.goal.clone(),
        workspace: callee.workspace_strategy,
        runner: callee.manager.runner.clone(),
        tool_grants: callee.tool_grants(),
        // The callee's own, like the runner and the grants beside it: a caller
        // does not get to widen what the cell it is calling may touch.
        tool_level: callee.manager.tools,
        autonomy_ceiling: call.autonomy_ceiling,
        budget: call.budget,
        definition_of_done: call.definition_of_done.clone(),
    })
}

fn parse_ceiling(level: &str) -> Result<Irreversibility, McpError> {
    Irreversibility::parse(level).ok_or_else(|| {
        McpError::invalid_params(
            format!(
                "`{level}` is not an autonomy ceiling; use reversible, undoable or irreversible"
            ),
            None,
        )
    })
}

/// Hold the whole cap against the caller's pool at call time.
///
/// An unbounded dimension in the call stays unbounded in the pool: reserving
/// against `None` would invent a limit the definition did not set.
fn reserve(pool: &mut Budget, held: Budget) {
    fn hold(remaining: &mut Option<u64>, amount: Option<u64>) {
        if let (Some(left), Some(amount)) = (remaining.as_mut(), amount) {
            *left = left.saturating_sub(amount);
        }
    }
    hold(&mut pool.usd_micros, held.usd_micros);
    hold(&mut pool.tokens, held.tokens);
    hold(&mut pool.wall_secs, held.wall_secs);
}

/// Give a reservation back when the call never started.
fn release(pool: &mut Budget, held: Budget) {
    fn give(remaining: &mut Option<u64>, amount: Option<u64>) {
        if let (Some(left), Some(amount)) = (remaining.as_mut(), amount) {
            *left = left.saturating_add(amount);
        }
    }
    give(&mut pool.usd_micros, held.usd_micros);
    give(&mut pool.tokens, held.tokens);
    give(&mut pool.wall_secs, held.wall_secs);
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
                "farseer's MCP face: read_memory, write_memory, and the manager-scoped \
                 delegate_to_worker and delegate_to_cell. Raw events are never appended \
                 through here."
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
    fn a_reservation_holds_the_cap_and_gives_it_back_when_the_call_never_started() {
        // `06 cell transport` made a cell call fire-and-forget, so there is no
        // terminal spend to draw later. The pool has to be held at call time or
        // it bounds nothing.
        let mut pool = Budget {
            usd_micros: Some(5_000_000),
            tokens: None,
            wall_secs: Some(600),
        };
        let held = Budget {
            usd_micros: Some(2_000_000),
            tokens: Some(40_000),
            wall_secs: Some(100),
        };

        reserve(&mut pool, held);
        assert_eq!(pool.usd_micros, Some(3_000_000));
        assert_eq!(pool.wall_secs, Some(500));
        assert_eq!(
            pool.tokens, None,
            "an unbounded dimension must not gain a limit the definition never set"
        );

        release(&mut pool, held);
        assert_eq!(pool.usd_micros, Some(5_000_000));
        assert_eq!(pool.wall_secs, Some(600));
        assert_eq!(pool.tokens, None);
    }

    #[test]
    fn a_cell_call_contract_takes_workspace_runner_and_tools_from_the_callee() {
        let (callee, _advisories) = farseer_core::CellDefinition::load(
            r#"
cell_id = "social"
name = "Social"
workspace_strategy = "plain_directory"

[manager]
runner = "goose"

[[roster]]
kind = "tool"
name = "post"
irreversibility = "undoable"
"#,
        )
        .unwrap();
        let task_id = farseer_core::TaskId::new();
        let call = CellCall {
            call_id: CallId::new(),
            from_cell: farseer_core::CellId::new("zero"),
            to_cell: callee.cell_id.clone(),
            goal: "post the changelog".into(),
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: "the post is live".into(),
            deadline_ms: None,
        };

        let contract = cell_call_contract(&call, &callee, task_id);

        assert_eq!(contract.runner, "goose", "the callee names its own runner");
        assert_eq!(contract.tool_grants, ["post"]);
        assert_eq!(
            contract.workspace,
            farseer_core::WorkspaceStrategy::PlainDirectory
        );
        assert_eq!(
            contract.task_id, task_id,
            "`22 cell addressing`: one task, one owner"
        );
        assert_eq!(contract.cell_id, callee.cell_id);
        assert_eq!(contract.autonomy_ceiling, Irreversibility::Reversible);
    }

    #[test]
    fn an_unknown_autonomy_level_is_refused_rather_than_defaulted() {
        assert!(parse_ceiling("undoable").is_ok());
        assert!(parse_ceiling("irreversible_gated").is_err());
    }

    #[test]
    fn a_cancelled_worker_keeps_its_terminal_report_for_mcp_drawdown() {
        let report = farseer_manager::RunReport {
            outcome: Outcome::Cancelled,
            cost_usd_micros: Some(123_456),
            tokens: Some(789),
            result: Some("terminal text".into()),
            window: None,
            windows: Vec::new(),
            session: None,
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
