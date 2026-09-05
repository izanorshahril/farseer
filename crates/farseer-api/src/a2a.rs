//! The inbound A2A endpoint and its Agent Card.
//!
//! `06 cell transport` section 2 made this **off by default**: local cells take
//! the in-process path and never traverse it, so the endpoint serves foreign
//! callers exclusively, and turning it on is the moment the card becomes a
//! public commitment that is hard to walk back. It answers only when
//! `runners.toml` names at least one peer.
//!
//! `21 A2A conformance` mapped the protocol and found the mapping lossy in
//! exactly one direction. Three consequences are built in here rather than
//! discovered later:
//!
//! - **The card is generated from the cell definitions**, per section 4, so the
//!   card and the definition cannot drift. Nothing about a cell is written down
//!   twice.
//! - **Four of the eight envelope fields have no native A2A home** - autonomy
//!   ceiling, budget, definition of done and deadline - so they ride in message
//!   metadata, and a foreign caller will silently ignore all four. Farseer
//!   enforces them on its own side, in the run it starts, because a foreign
//!   callee is unbounded by construction and so is a foreign *caller*'s idea of
//!   what it asked for.
//! - **`from_cell` is derived from auth**, not carried on the wire: the caller's
//!   identity is which token authenticated the request, which is both what the
//!   protocol offers and the safer construction.
//!
//! What is deliberately absent is a stream. A2A's `SubscribeToTask` is a state
//! snapshot followed by live events, where `16 local api surface` made replay
//! and attach one call with a cursor. Rather than ship a subscription that
//! silently loses the backlog, this exposes `tasks/get` and leaves the record's
//! own cursored stream as the way to read history - which `21` section 3 already
//! concluded: farseer's record is the source of truth, and a peer's history is
//! not recoverable through A2A.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use farseer_core::{
    A2aPeer, Budget, CellId, RunId, TaskId, WorkerContract, WorkerContractSpec, policy,
};
use serde::{Deserialize, Serialize};

use crate::{AppState, RunRole, now_ms, spawn_run};

/// The JSON-RPC method names of A2A's JSON-RPC binding.
///
/// Named here rather than matched inline so the three farseer answers and the
/// one it refuses are visible in one place.
const SEND: &str = "message/send";
const GET: &str = "tasks/get";
const CANCEL: &str = "tasks/cancel";
const SUBSCRIBE: &str = "tasks/resubscribe";

/// Where a client looks. A2A discovery is a well-known URI and plain HTTP
/// caching - `21` section 5: there is no health check and no liveness concept,
/// so a dead agent is a URL that stopped answering.
pub(crate) async fn agent_card(State(state): State<Arc<AppState>>) -> Response {
    let config = &state.runner_config().a2a;
    if !config.is_on() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "this farseer has no A2A peers, so it publishes no card - see `[a2a]` in runners.toml"
            })),
        )
            .into_response();
    }
    // One skill per exposed cell, described by the definition itself. `21`
    // section 4: generate the card from the definition rather than maintaining
    // it separately, so drift is structurally impossible rather than merely
    // discouraged.
    let cells = state.cells();
    let skills: Vec<serde_json::Value> = config
        .expose
        .iter()
        .filter_map(|name| cells.get(&CellId::new(name.clone())))
        .map(|cell| {
            serde_json::json!({
                "id": cell.cell_id.to_string(),
                "name": cell.name,
                // What this cell may hand work to, which is the honest
                // description of an orchestrator's capability: it is not one
                // agent's skill list, it is a roster.
                "description": format!(
                    "an orchestrator cell whose roster is: {}",
                    roster_summary(cell)
                ),
                "tags": ["orchestrator"],
                "inputModes": ["text/plain"],
                "outputModes": ["text/plain"],
            })
        })
        .collect();

    let card = serde_json::json!({
        "protocolVersion": "1.0",
        "name": config.name.clone().unwrap_or_else(|| "farseer".into()),
        // `06` section 3: farseer says what it is. A caller that believes it is
        // driving a single agent when it is driving a fleet has quietly wrong
        // timeout, cancellation and progress assumptions - so the description
        // leads with the one fact that changes how it should be driven.
        "description": "An orchestrator, not a single agent. A message starts a run in a cell, which may delegate to workers and to other cells; the reply is the task, not the answer.",
        "version": env!("CARGO_PKG_VERSION"),
        "preferredTransport": "JSONRPC",
        "capabilities": {
            // Neither is offered, and saying so is the point: `21` section 3
            // found A2A's subscription cannot express `16`'s cursored replay,
            // and advertising one that loses the backlog would be worse than
            // advertising none.
            "streaming": false,
            "pushNotifications": false,
            "stateTransitionHistory": false,
        },
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "securitySchemes": {
            // `06` section 7: a bearer per peer, so revocation is per peer.
            "peerToken": { "type": "http", "scheme": "bearer" }
        },
        "security": [{ "peerToken": [] }],
        "skills": skills,
    });
    // The freshness story A2A actually has, per `21` section 5: cache lifetime
    // on a static document, and nothing else.
    ([(header::CACHE_CONTROL, "public, max-age=300")], Json(card)).into_response()
}

fn roster_summary(cell: &farseer_core::CellDefinition) -> String {
    let names: Vec<String> = cell
        .roster
        .iter()
        .map(|entry| entry.name().to_string())
        .collect();
    if names.is_empty() {
        "nothing - it does the work itself".into()
    } else {
        names.join(", ")
    }
}

#[derive(Debug, Deserialize)]
pub struct Rpc {
    #[serde(default)]
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

fn ok(id: serde_json::Value, result: serde_json::Value) -> Response {
    Json(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn err(id: serde_json::Value, status: StatusCode, code: i64, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": RpcError { code, message: message.to_string() },
        })),
    )
        .into_response()
}

/// One JSON-RPC call from a foreign orchestrator.
pub(crate) async fn rpc(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(call): Json<Rpc>,
) -> Response {
    let config = state.runner_config().a2a.clone();
    if !config.is_on() {
        return err(
            call.id,
            StatusCode::NOT_FOUND,
            -32601,
            "this farseer has no A2A peers - see `[a2a]` in runners.toml",
        );
    }
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let Some(peer) = config.peer_for(presented) else {
        // No body detail. A refusal that says which part was wrong is a hint
        // for guessing the other part.
        return err(
            call.id,
            StatusCode::UNAUTHORIZED,
            -32001,
            "unknown peer token",
        );
    };
    if !config.expose.iter().any(|name| name == &peer.cell) {
        return err(
            call.id,
            StatusCode::FORBIDDEN,
            -32002,
            "this peer is bound to a cell the card does not expose",
        );
    }

    match call.method.as_str() {
        SEND => send(&state, peer, call.id, call.params),
        GET => get(&state, peer, call.id, call.params),
        CANCEL => cancel(&state, peer, call.id, call.params).await,
        SUBSCRIBE => err(
            call.id,
            StatusCode::NOT_IMPLEMENTED,
            -32004,
            // Said rather than silently unsupported: `21` section 3 found the
            // subscription cannot express a cursored replay, so a client that
            // reconnected would lose what happened while it was away and never
            // learn that it had.
            "farseer does not stream over A2A - its own record is cursored and A2A's subscription is not, so poll tasks/get",
        ),
        other => err(
            call.id,
            StatusCode::NOT_IMPLEMENTED,
            -32601,
            &format!("farseer's A2A face has no `{other}`"),
        ),
    }
}

/// `message/send`: start a run in the peer's cell and answer with the task.
///
/// Always returns immediately, whatever `configuration.returnImmediately` says.
/// `06` made a cell call fire-and-forget for reasons that do not change with the
/// transport: a call can run for hours, and welding two orchestrators' lifetimes
/// together means a callee that hangs takes its caller down with it.
fn send(
    state: &Arc<AppState>,
    peer: &A2aPeer,
    id: serde_json::Value,
    params: serde_json::Value,
) -> Response {
    let goal = params
        .pointer("/message/parts")
        .and_then(|parts| parts.as_array())
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if goal.trim().is_empty() {
        return err(
            id,
            StatusCode::BAD_REQUEST,
            -32602,
            "the message carries no text part to act on",
        );
    }

    let cell_id = CellId::new(peer.cell.clone());
    let Some(cell) = state.cells().get(&cell_id).cloned() else {
        return err(
            id,
            StatusCode::NOT_FOUND,
            -32003,
            "this peer's cell is not loaded",
        );
    };
    if let Err(error) = crate::lifecycle::ensure_accepts_work(state, &cell_id) {
        return err(id, StatusCode::FORBIDDEN, -32005, &error.to_string());
    }

    // The four fields with no native home, read from metadata and **narrowed
    // against the cell's own policy**. `21` section 7: a caller cannot raise
    // what the definition allows, and a foreign caller cannot be trusted to
    // have read those fields at all - so nothing here widens anything.
    let metadata = params.pointer("/message/metadata");
    let asked = metadata
        .and_then(|m| m.get("autonomy_ceiling"))
        .and_then(|c| c.as_str())
        .and_then(policy::Irreversibility::parse);
    let ceiling = match asked {
        Some(asked) => asked.min(cell.policy.autonomy_ceiling),
        None => cell.policy.autonomy_ceiling,
    };
    let budget = metadata
        .and_then(|m| m.get("budget"))
        .and_then(|b| serde_json::from_value::<Budget>(b.clone()).ok())
        .map(|asked| asked.cap_to(cell.budget))
        .unwrap_or(cell.budget);
    let definition_of_done = metadata
        .and_then(|m| m.get("definition_of_done"))
        .and_then(|d| d.as_str())
        .unwrap_or_default()
        .to_string();

    let run_id = RunId::new();
    let contract = WorkerContract::seal(WorkerContractSpec {
        run_id,
        task_id: TaskId::new(),
        cell_id: cell.cell_id.clone(),
        // The caller is named in the goal because it is named nowhere else a
        // manager can read, and a manager that cannot tell an operator's
        // instruction from a foreign orchestrator's is missing the one fact
        // that should change how it answers.
        goal: format!("[from the A2A peer `{}`]\n{goal}", peer.name),
        workspace: cell.workspace_strategy,
        runner: cell.manager.runner().to_string(),
        tool_grants: cell.tool_grants(),
        tool_level: cell.manager.tools,
        autonomy_ceiling: ceiling,
        budget,
        definition_of_done,
    });
    match spawn_run(state, contract, RunRole::Manager, cell, None, None) {
        Ok(run_id) => ok(id, task_view(state, run_id)),
        Err(error) => err(id, StatusCode::BAD_REQUEST, -32006, &error.to_string()),
    }
}

fn get(
    state: &Arc<AppState>,
    peer: &A2aPeer,
    id: serde_json::Value,
    params: serde_json::Value,
) -> Response {
    match task_of(state, peer, &params) {
        Ok(run_id) => ok(id, task_view(state, run_id)),
        Err(response) => response(id),
    }
}

async fn cancel(
    state: &Arc<AppState>,
    peer: &A2aPeer,
    id: serde_json::Value,
    params: serde_json::Value,
) -> Response {
    let run_id = match task_of(state, peer, &params) {
        Ok(run_id) => run_id,
        Err(response) => return response(id),
    };
    // The same path the operator's cancel takes, so a peer cancelling and an
    // operator cancelling cannot come to mean two different things.
    match crate::cancel_run_inner(state, run_id).await {
        Ok(()) => ok(id, task_view(state, run_id)),
        Err(error) => err(id, StatusCode::BAD_REQUEST, -32007, &error.to_string()),
    }
}

/// The task id a request names, checked against the peer that is asking.
///
/// A peer sees its own cell's runs and nothing else. The token is bound to a
/// cell, so this is the same fence the send path uses, applied on the way back.
#[allow(clippy::type_complexity)]
fn task_of(
    state: &Arc<AppState>,
    peer: &A2aPeer,
    params: &serde_json::Value,
) -> Result<RunId, Box<dyn Fn(serde_json::Value) -> Response>> {
    let raw = params
        .get("id")
        .or_else(|| params.get("taskId"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let Ok(run_id) = raw.parse::<RunId>() else {
        return Err(Box::new(|id| {
            err(id, StatusCode::BAD_REQUEST, -32602, "no such task id")
        }));
    };
    match cell_of(state, run_id) {
        Some(cell) if cell == peer.cell => Ok(run_id),
        // A run in another cell answers the same as a run that does not exist.
        // Distinguishing them tells a peer what else this farseer is running.
        _ => Err(Box::new(|id| {
            err(id, StatusCode::NOT_FOUND, -32001, "no such task")
        })),
    }
}

/// Which cell a run belongs to, from the row **or** from the record.
///
/// The row is written when the run starts and the record entry when it is
/// queued, so a task read immediately after `message/send` - which is the first
/// thing a well-behaved A2A client does - has an event and no row yet. Asking
/// only the row made that read answer "no such task" about a task farseer had
/// just handed out an id for.
fn cell_of(state: &Arc<AppState>, run_id: RunId) -> Option<String> {
    // The manager context first, because it is the only one of the three that
    // exists the moment `spawn_run` returns. The row is written when the run
    // starts and `run_queued` when the manager loop reaches it, so a peer that
    // reads its task back on the next line - which is what a well-behaved
    // client does - was being told there was no such task, about a task farseer
    // had just handed it the id for.
    if let Some(manager) = state.manager(run_id) {
        return Some(manager.cell.cell_id.to_string());
    }
    if let Ok(Some(row)) = state.store().run(run_id) {
        return Some(row.cell_id.to_string());
    }
    let events = state
        .store()
        .scan(0, 50, &farseer_store::ScanFilter::run(run_id))
        .ok()?;
    events.first().map(|event| event.cell_id.to_string())
}

/// `21` section 1's state table, which maps better than the ticket assumed:
/// `CANCELED` is distinct from `FAILED` natively, so `05 run state model`'s
/// "cancelled is never failed" needs no convention here.
fn task_view(state: &Arc<AppState>, run_id: RunId) -> serde_json::Value {
    let row = state.store().run(run_id).ok().flatten();
    let (state_name, task_id) = match &row {
        Some(row) => (
            match row.outcome.as_deref() {
                None => "TASK_STATE_WORKING",
                Some("ok") => "TASK_STATE_COMPLETED",
                Some("cancelled") => "TASK_STATE_CANCELED",
                Some("abandoned") => "TASK_STATE_CANCELED",
                Some(_) => "TASK_STATE_FAILED",
            },
            row.task_id.to_string(),
        ),
        None => ("TASK_STATE_SUBMITTED", String::new()),
    };
    serde_json::json!({
        // The run id is the task id: `06` made the `call_id` the A2A task id,
        // and a farseer run is what a call becomes.
        "id": run_id.to_string(),
        "contextId": task_id,
        "status": {
            "state": state_name,
            "timestamp": now_ms(),
        },
        // No artifacts yet. `08 generalization test` made artifact the unit of
        // reviewable change and `21` section 2 noted A2A agrees; what farseer
        // does not yet have is a step that turns a finished run's diff into
        // one, and inventing an empty artifact would be a claim.
        "artifacts": [],
    })
}
