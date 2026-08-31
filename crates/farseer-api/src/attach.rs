//! `07 attach semantics`: observe, take over, release, intervene.
//!
//! **Attach targets a run, not a worker.** A run may or may not have a live
//! process behind it, and the same address answers either way - live output
//! when there is a process, replay when there is not. That is `07` section 1,
//! and it is why nothing here holds a process handle: the subscription is the
//! event stream, which `16 local api surface` already serves.
//!
//! **Read-only by default.** Watching is the common case and stray keystrokes
//! into a live agent are destructive, so control is a separate, explicit step
//! that leaves an event carrying who, when and what was sent. The cost to the
//! operator is one call.
//!
//! **A lease, not a lock.** `07` section 7 requires that detaching without
//! releasing auto-releases after a timeout, because a closed terminal that
//! freezes a worker waiting on a human forever is exactly the class of hang the
//! brief catalogues. The lease is renewed by any control call and by an explicit
//! heartbeat, and it lapses **on read**: there is no timer, so a farseer that is
//! busy or asleep cannot hold a run hostage on behalf of somebody who left.
//!
//! **Intervention tells the manager and does not void the contract.** The
//! message is appended to the run as `operator_intervened`; the manager reads it
//! on its next wake and decides for itself whether the goal changed. The run
//! carries `operator_touched` permanently, so neither the manager nor the record
//! mistakes the outcome for autonomous.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use farseer_core::{Actor, Control, EventKind, NewEvent, RunId};
use serde::{Deserialize, Serialize};

use crate::{ApiError, ApiResult, AppState, now_ms};

/// How long a control state survives without being renewed.
///
/// Long enough that an operator reading output is not dropped mid-thought, short
/// enough that a closed laptop is not a held worker. `18 hang detection prior
/// art` is where a measured number would come from; until there is one, this is
/// a stated default rather than a tuned one.
pub const LEASE_MS: i64 = 90_000;

/// Who holds a run, and until when.
#[derive(Debug, Clone, Copy)]
pub struct Attachment {
    pub control: Control,
    /// When the lease was last renewed.
    pub seen_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct ControlView {
    pub run_id: String,
    pub control: &'static str,
    /// Milliseconds left before this lapses back to autonomous, or `null` when
    /// nothing is holding the run.
    pub expires_in_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct InterveneBody {
    pub message: String,
}

fn word(control: Control) -> &'static str {
    match control {
        Control::Autonomous => "autonomous",
        Control::Observed => "observed",
        Control::TakenOver => "taken_over",
    }
}

/// What a run is on the control axis, **after** letting a stale lease lapse.
///
/// Derived on read rather than swept by a timer, for the same reason `05 run
/// state model` derives liveness: a stored value and a crashed process disagree,
/// and the disagreement always favours the wrong answer - here, a worker frozen
/// for a person who closed their laptop.
pub(crate) fn control_of(state: &AppState, run_id: RunId, now: i64) -> Control {
    let mut held = state.attachments();
    let Some(attachment) = held.get(&run_id).copied() else {
        return Control::Autonomous;
    };
    if now - attachment.seen_ms <= LEASE_MS {
        return attachment.control;
    }
    held.remove(&run_id);
    drop(held);
    // The lapse is recorded, because `07` makes taking over an event carrying
    // who and when, and a release that only happened because nobody renewed is
    // still the moment the agent got the wheel back.
    record(
        state,
        run_id,
        serde_json::json!({
            "control": word(Control::Autonomous),
            "from": word(attachment.control),
            "reason": "the attachment lapsed without being released",
        }),
    );
    Control::Autonomous
}

/// Append to the run this is about, in the run's own cell.
///
/// Best-effort: a control move that could not be written is still a control
/// move, and refusing the release because the record is unavailable would hold
/// the worker for the wrong reason.
fn record(state: &AppState, run_id: RunId, payload: serde_json::Value) {
    let cell_id = state
        .store()
        .run(run_id)
        .ok()
        .flatten()
        .map(|row| row.cell_id);
    let Some(cell_id) = cell_id else { return };
    if let Err(error) = state.store().append(&NewEvent::new(
        cell_id,
        run_id,
        EventKind::new(EventKind::OPERATOR_INTERVENED),
        Actor::Operator,
        now_ms(),
        payload,
    )) {
        eprintln!("control change for run {run_id} was not recorded: {error}");
    }
}

pub(crate) async fn observe(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<Json<ControlView>> {
    hold(&state, &run_id, Control::Observed)
}

pub(crate) async fn take_over(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<Json<ControlView>> {
    hold(&state, &run_id, Control::TakenOver)
}

/// Renew without changing what is held.
///
/// Separate from `observe` so a client tailing output does not have to re-assert
/// a takeover it may not have, and so a heartbeat against a run nobody holds is
/// a `400` rather than a silent grab of the wheel.
pub(crate) async fn heartbeat(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<Json<ControlView>> {
    let run_id = parse(&run_id)?;
    let now = now_ms();
    let current = control_of(&state, run_id, now);
    if current == Control::Autonomous {
        return Err(ApiError::BadRequest(
            "nothing is attached to this run, so there is no lease to renew",
        ));
    }
    state.attachments().insert(
        run_id,
        Attachment {
            control: current,
            seen_ms: now,
        },
    );
    Ok(Json(view(run_id, current, now, now)))
}

/// Hand the run back to the agent. `07` section 7: the worker carries on.
pub(crate) async fn release(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<Json<ControlView>> {
    let run_id = parse(&run_id)?;
    let now = now_ms();
    let current = control_of(&state, run_id, now);
    if current == Control::Autonomous {
        return Err(ApiError::BadRequest("this run is already autonomous"));
    }
    state.attachments().remove(&run_id);
    record(
        &state,
        run_id,
        serde_json::json!({
            "control": word(Control::Autonomous),
            "from": word(current),
            "reason": "released by the operator",
        }),
    );
    Ok(Json(view(run_id, Control::Autonomous, now, now)))
}

/// Send the operator's words into a run they have taken over.
///
/// **Takeover is required**, per `07` section 3: silent injection makes the
/// manager's model of its own worker wrong with no marker anywhere, and the
/// explicit boundary is what the event hangs on.
///
/// `cancel` is deliberately not reachable from here. `07` section 7: taking over
/// never kills a worker, and killing one is not a form of intervention.
pub(crate) async fn intervene(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
    Json(body): Json<InterveneBody>,
) -> ApiResult<Json<ControlView>> {
    if body.message.trim().is_empty() {
        return Err(ApiError::BadRequest("message must not be empty"));
    }
    let run_id = parse(&run_id)?;
    let now = now_ms();
    if control_of(&state, run_id, now) != Control::TakenOver {
        return Err(ApiError::Policy(
            "take over this run before sending it anything - attach is read-only by default".into(),
        ));
    }
    let steer = state
        .runs()
        .get(&run_id)
        .ok_or(ApiError::NotFound("run"))?
        .steer
        .clone();
    let Some(steer) = steer else {
        return Err(ApiError::BadRequest(
            "this run's runner has no path for live input",
        ));
    };
    steer
        .steer(&body.message)
        .map_err(|e| ApiError::Steer(e.to_string()))?;

    // The two halves `07` section 6 requires: the manager is told, and the run
    // is marked for good. The flag is on the run row rather than derived from
    // the event, because `11 analytics questions` asks how often a human had to
    // step in and a query that has to scan the log to answer it is a query
    // nobody runs.
    crate::mark_touched(&state, run_id);
    record(
        &state,
        run_id,
        serde_json::json!({
            "control": word(Control::TakenOver),
            "message": body.message,
        }),
    );
    // The lease is renewed by using it: an operator typing is an operator
    // present, and asking them to also heartbeat would be a lease that lapses
    // mid-conversation.
    state.attachments().insert(
        run_id,
        Attachment {
            control: Control::TakenOver,
            seen_ms: now,
        },
    );
    Ok(Json(view(run_id, Control::TakenOver, now, now)))
}

/// What the run is on the control axis right now.
pub(crate) async fn read_control(
    State(state): State<Arc<AppState>>,
    UrlPath(run_id): UrlPath<String>,
) -> ApiResult<Json<ControlView>> {
    let run_id = parse(&run_id)?;
    let now = now_ms();
    let control = control_of(&state, run_id, now);
    let seen = state
        .attachments()
        .get(&run_id)
        .map(|attachment| attachment.seen_ms)
        .unwrap_or(now);
    Ok(Json(view(run_id, control, seen, now)))
}

fn hold(state: &Arc<AppState>, run_id: &str, to: Control) -> ApiResult<Json<ControlView>> {
    let run_id = parse(run_id)?;
    // A run farseer has never heard of is a `404`; a run that has finished is
    // not. `07` section 1 makes attach work on a run with no live process at
    // all - the subscription is replay - so refusing here would break the case
    // the ticket opened with.
    if state.store().run(run_id)?.is_none() {
        return Err(ApiError::NotFound("run"));
    }
    let now = now_ms();
    let from = control_of(state, run_id, now);
    state.attachments().insert(
        run_id,
        Attachment {
            control: to,
            seen_ms: now,
        },
    );
    if from != to {
        record(
            state,
            run_id,
            serde_json::json!({ "control": word(to), "from": word(from) }),
        );
    }
    Ok(Json(view(run_id, to, now, now)))
}

fn view(run_id: RunId, control: Control, seen: i64, now: i64) -> ControlView {
    ControlView {
        run_id: run_id.to_string(),
        control: word(control),
        expires_in_ms: (control != Control::Autonomous).then(|| (seen + LEASE_MS - now).max(0)),
    }
}

fn parse(run_id: &str) -> ApiResult<RunId> {
    run_id.parse().map_err(|_| ApiError::NotFound("run"))
}
