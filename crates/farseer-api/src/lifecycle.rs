//! `17 cell lifecycle`'s verbs: pause, resume, archive, restore, delete, purge.
//!
//! Four things this module exists to keep true.
//!
//! **Pause is a policy flag, never a process operation.** A paused cell starts
//! no new run; runs already in flight finish. `17` refused to suspend an agent
//! mid-API-call - the socket times out, the provider's side of the conversation
//! does not pause, and resuming lands in a session that is already broken - and
//! `03 spike job objects` measured cancel at 300 microseconds, so the honest
//! alternative is cheap.
//!
//! **Three verbs increasing in violence.** Archive removes the running cell and
//! keeps the definition and the record. Delete additionally removes the
//! definition binding and keeps the record, per `02 record scope`, because a
//! definition is a file in git and deleting it is reversible while its history
//! is not. Purge removes the record slice and keeps nothing - the only
//! irreversible verb farseer owns, which is why it is separate and says what it
//! destroyed.
//!
//! **Cell zero can be archived and never deleted.** `01 cell primitive` made it
//! the address the operator talks to, and a runtime with no cell has nothing to
//! receive an instruction. Promoting another cell on delete was considered and
//! rejected in `17`: complexity in the most dangerous verb, for no benefit.
//!
//! **Lifecycle is not an edit to the definition.** `16 local api surface` gave
//! the API read, validate and reload and no edit path, and none of this writes
//! a `.toml`. The state lives in the store, so a `reload` cannot resurrect a
//! deleted cell by re-reading the file git still has.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use farseer_core::{Actor, CellId, EventKind, NewEvent, RunId};
use farseer_store::{Lifecycle, Purged};
use serde::{Deserialize, Serialize};

use crate::{ApiError, ApiResult, AppState, now_ms};

/// The cell `01 cell primitive` made the operator's address.
const CELL_ZERO: &str = "zero";

#[derive(Debug, Serialize)]
pub struct StateView {
    pub cell_id: String,
    pub lifecycle: &'static str,
    /// Whether a new run may start here. Derived, so a surface never has to
    /// re-decide which of four words means "yes".
    pub accepts_work: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct PurgeBody {
    /// Milliseconds since the epoch, inclusive. `None` is unbounded.
    #[serde(default)]
    pub from: Option<i64>,
    #[serde(default)]
    pub to: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PurgeView {
    pub cell_id: String,
    pub events: usize,
    pub runs: usize,
    pub memories: usize,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

pub(crate) async fn pause(
    State(state): State<Arc<AppState>>,
    UrlPath(cell_id): UrlPath<String>,
) -> ApiResult<Json<StateView>> {
    move_to(&state, &cell_id, Lifecycle::Paused)
}

pub(crate) async fn resume(
    State(state): State<Arc<AppState>>,
    UrlPath(cell_id): UrlPath<String>,
) -> ApiResult<Json<StateView>> {
    move_to(&state, &cell_id, Lifecycle::Active)
}

pub(crate) async fn archive(
    State(state): State<Arc<AppState>>,
    UrlPath(cell_id): UrlPath<String>,
) -> ApiResult<Json<StateView>> {
    move_to(&state, &cell_id, Lifecycle::Archived)
}

/// Bring an archived or deleted cell back.
///
/// The same handler as `resume` because they are the same move - back to
/// active - and giving each verb its own path would be three names for one
/// transition that the record already distinguishes by what it moved *from*.
pub(crate) async fn restore(
    State(state): State<Arc<AppState>>,
    UrlPath(cell_id): UrlPath<String>,
) -> ApiResult<Json<StateView>> {
    move_to(&state, &cell_id, Lifecycle::Active)
}

pub(crate) async fn delete(
    State(state): State<Arc<AppState>>,
    UrlPath(cell_id): UrlPath<String>,
) -> ApiResult<Json<StateView>> {
    if cell_id == CELL_ZERO {
        return Err(ApiError::Policy(
            "cell zero is the address the operator talks to and cannot be deleted; archive it instead".into(),
        ));
    }
    move_to(&state, &cell_id, Lifecycle::Deleted)
}

/// Destroy part or all of a cell's record, and say so in the record.
///
/// Unlike the other verbs this does not require the cell to be in any
/// particular state: purge is a retention operation, and a cell being actively
/// worked in is exactly the one whose oldest months an operator wants gone.
pub(crate) async fn purge(
    State(state): State<Arc<AppState>>,
    UrlPath(cell_id): UrlPath<String>,
    body: Option<Json<PurgeBody>>,
) -> ApiResult<Json<PurgeView>> {
    let scope = body.map(|Json(body)| body).unwrap_or_default();
    if let (Some(from), Some(to)) = (scope.from, scope.to)
        && from > to
    {
        return Err(ApiError::BadRequest("from is after to"));
    }
    let cell = CellId::new(cell_id.clone());
    let purged: Purged = {
        let mut store = state.store();
        store.purge_cell(&cell, scope.from, scope.to)?
    };

    // The tombstone, per `17 cell lifecycle` section 5. Appended **after** the
    // delete, so it survives the purge that produced it, and it names the scope
    // rather than merely marking that something happened - `02 record scope`
    // made the record evidence, and a hole that cannot say where it came from
    // makes the rest of the evidence worth less.
    state.store().append(&NewEvent::new(
        cell.clone(),
        RunId::none(),
        EventKind::new(EventKind::CELL_PURGED),
        Actor::Operator,
        now_ms(),
        serde_json::json!({
            "from": scope.from,
            "to": scope.to,
            "events": purged.events,
            "runs": purged.runs,
            "memories": purged.memories,
            // What a reader crossing the hole should conclude. `void` is
            // permanent where `gap` is refetchable, which is the distinction
            // `09 store decision` asked for and the reason a client does not
            // retry forever on a hole that is never coming back.
            "hole": EventKind::VOID,
        }),
    ))?;

    Ok(Json(PurgeView {
        cell_id,
        events: purged.events,
        runs: purged.runs,
        memories: purged.memories,
        from: scope.from,
        to: scope.to,
    }))
}

/// The move itself, recorded.
///
/// Refuses a move to where the cell already is. Not pedantry: every one of these
/// appends an event, and a surface that fires twice would otherwise write a
/// history of transitions that did not happen.
fn move_to(state: &Arc<AppState>, cell_id: &str, to: Lifecycle) -> ApiResult<Json<StateView>> {
    let cell = CellId::new(cell_id.to_string());
    let known = state.cells().contains_key(&cell);
    let from = state.store().cell_state(&cell)?;
    // A deleted cell is not in the loaded registry, so "unknown" has to admit
    // the ones this module removed - otherwise nothing could ever restore one.
    if !known && from == Lifecycle::Active {
        return Err(ApiError::NotFound("cell"));
    }
    if from == to {
        return Err(ApiError::BadRequest("the cell is already in that state"));
    }
    state.store().set_cell_state(&cell, to, now_ms())?;
    state.store().append(&NewEvent::new(
        cell.clone(),
        RunId::none(),
        EventKind::new(EventKind::CELL_LIFECYCLE),
        Actor::Operator,
        now_ms(),
        serde_json::json!({ "from": from.as_str(), "to": to.as_str() }),
    ))?;
    // The registry follows the store. A deleted cell stops being addressable
    // now rather than at the next reload; anything else comes back on reload,
    // which reads the same directory the file never left.
    if to == Lifecycle::Deleted {
        state.cells().remove(&cell);
    } else {
        state.reload();
    }
    Ok(Json(StateView {
        cell_id: cell_id.to_string(),
        lifecycle: to.as_str(),
        accepts_work: to.accepts_work(),
    }))
}

/// Refuse a run in a cell that is not taking work.
///
/// Called on every path that starts one - the operator's instruction and both
/// delegation verbs - because `17`'s pause is only real if it is checked where
/// runs begin rather than where the operator happens to be looking.
pub(crate) fn ensure_accepts_work(state: &AppState, cell_id: &CellId) -> ApiResult<()> {
    let state_of = state.store().cell_state(cell_id)?;
    if state_of.accepts_work() {
        return Ok(());
    }
    Err(ApiError::Policy(format!(
        "cell `{cell_id}` is {} and starts no new runs",
        state_of.as_str()
    )))
}

/// Every cell that has moved, for a surface that wants to show it.
pub(crate) async fn list_states(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<StateView>>> {
    Ok(Json(
        state
            .store()
            .cell_states()?
            .into_iter()
            .map(|(cell_id, lifecycle)| StateView {
                cell_id: cell_id.to_string(),
                lifecycle: lifecycle.as_str(),
                accepts_work: lifecycle.accepts_work(),
            })
            .collect(),
    ))
}
