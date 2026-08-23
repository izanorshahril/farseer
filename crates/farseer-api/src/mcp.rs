//! `02` section 8: **the MCP face - query and memory-write, never raw event
//! append.** "An agent that can forge events can rewrite its own history."
//! So this offers exactly two tools - [`FarseerMcp::read_memory`] and
//! [`FarseerMcp::write_memory`] - and nothing that appends an event.
//!
//! **Nested into the existing router, not a second process.** `01` gives
//! farseer one API and `09` gives the record one writer, by construction:
//! one process, one `Store`. A stdio-transport MCP server is normally its
//! own process, spawned per client - which here would mean a second process
//! opening the same SQLite file to write memory, precisely what `09`'s "one
//! process into one writer" was framed to rule out. The streamable-HTTP
//! transport (`rmcp`'s `transport-streamable-http-server`) instead nests
//! into [`crate::router`] at `/v1/mcp`, sharing `AppState`'s own `Store` and
//! the same loopback/token guard as every other route.
//!
//! `cell_id` and (optionally) `run_id` are tool arguments, not fixed at
//! connection time - one MCP endpoint serves every cell and run, the same
//! way one HTTP API does.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};

use farseer_core::{CellId, MemoryTier, RunId};
use farseer_store::{MemoryScope, NewMemory, StoreError};

use crate::{AppState, now_ms};

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
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ReadMemoryArgs {
    /// Which cell's memory to read: its own `cell_local` tier plus every
    /// `global` claim, per `02`.
    cell_id: String,
    /// Also reads this run's own `run_local` tier, and is who
    /// `memory_consulted` attributes each returned claim to, per `02`'s
    /// "Carried from 11".
    run_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct WriteMemoryArgs {
    cell_id: String,
    body: String,
    /// `cell_local` (the default, per `25`) or `run_local`. `global` is
    /// refused here - `25` gates it on the operator, a promotion this face
    /// does not offer.
    tier: Option<String>,
    /// Required when `tier` is `run_local` - a claim with nothing to scope
    /// it to is not writable.
    run_id: Option<String>,
}

#[tool_router]
impl FarseerMcp {
    #[tool(
        description = "Read this cell's memory: its own cell-local claims plus every global claim, and (with run_id) this run's own run-local claims. Marks each returned claim as consulted by the run."
    )]
    fn read_memory(
        &self,
        Parameters(args): Parameters<ReadMemoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let run_id = parse_run_id(args.run_id.as_deref())?;
        let cell_id = CellId::new(args.cell_id);
        // Built from the loaded definition, not a bare `MemoryScope::new`,
        // so a cell's own `also_read` opt-in reaches this face too - `02`:
        // cross-cell reads beyond `global` are opt-in via the *reader's*
        // definition, and skipping that lookup would silently drop them.
        let definition = self
            .state
            .cells()
            .get(&cell_id)
            .cloned()
            .ok_or_else(|| McpError::invalid_params(format!("no cell `{cell_id}`"), None))?;
        let scope = MemoryScope::from_definition(&definition, run_id);
        let claims = self
            .state
            .store()
            .read_memory(&scope)
            .map_err(store_error)?;
        if let Some(run_id) = run_id {
            let store = self.state.store();
            for claim in &claims {
                store
                    .record_consulted(run_id, claim.memory_id, now_ms())
                    .map_err(store_error)?;
            }
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
        description = "Write a memory claim for this cell. Defaults to the cell-local tier; the global tier is not writable here - promotion needs the operator, per ticket 25."
    )]
    fn write_memory(
        &self,
        Parameters(args): Parameters<WriteMemoryArgs>,
    ) -> Result<CallToolResult, McpError> {
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
        let run_id = parse_run_id(args.run_id.as_deref())?;
        if tier == MemoryTier::RunLocal && run_id.is_none() {
            return Err(McpError::invalid_params(
                "run_local needs run_id - there is nothing to scope the claim to otherwise",
                None,
            ));
        }
        let memory_id = self
            .state
            .store()
            .write_memory(&NewMemory {
                tier,
                cell_id: CellId::new(args.cell_id),
                run_id,
                body: args.body,
                supersedes: Vec::new(),
                ts: now_ms(),
            })
            .map_err(store_error)?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            memory_id.to_string(),
        )]))
    }
}

fn parse_run_id(s: Option<&str>) -> Result<Option<RunId>, McpError> {
    s.map(|s| {
        s.parse::<RunId>()
            .map_err(|_| McpError::invalid_params(format!("`{s}` is not a valid run id"), None))
    })
    .transpose()
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
                "farseer's MCP face, per ticket 02: read_memory and write_memory only. \
                 Raw events are never appended through here."
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
