//! The record's unit: an event.
//!
//! `02 record scope`: `{ seq, event_id, ts, cell_id, run_id, kind, actor, payload }`.
//!
//! Events are written by the runtime, from things it **observed**. Agents may
//! not append them - `02 record scope` section 8 is explicit that an agent which can forge
//! events can rewrite its own history, at which point the record stops being
//! evidence and becomes a story the agent tells about itself.

use serde::{Deserialize, Serialize};

use crate::ids::{CellId, EventId, RunId, Seq};

/// Who caused this.
///
/// `02 record scope`: the field that is easy to omit and expensive to add later. Deriving
/// actor from `kind` works right up until one kind can come from two sources,
/// and at that moment every historical query becomes quietly wrong with no error
/// to notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Manager,
    Worker,
    Operator,
    System,
}

impl Actor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manager => "manager",
            Self::Worker => "worker",
            Self::Operator => "operator",
            Self::System => "system",
        }
    }
}

impl std::str::FromStr for Actor {
    type Err = UnknownActor;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "manager" => Ok(Self::Manager),
            "worker" => Ok(Self::Worker),
            "operator" => Ok(Self::Operator),
            "system" => Ok(Self::System),
            other => Err(UnknownActor(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown actor: {0}")]
pub struct UnknownActor(pub String);

/// What happened.
///
/// Open rather than closed, because runner adapters emit kinds farseer's own
/// code does not name. `02 record scope` versions each payload independently, so adding a
/// field to one kind never touches the others.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventKind(String);

impl EventKind {
    // Lifecycle, per `05 run state model`.
    pub const RUN_QUEUED: &'static str = "run_queued";
    pub const RUN_STARTED: &'static str = "run_started";
    pub const RUN_FINISHED: &'static str = "run_finished";

    // The three progress kinds. `05 run state model` made these a hard disqualifier for any
    // control channel that cannot emit them.
    pub const TOOL_CALL_STARTED: &'static str = "tool_call_started";
    pub const TOOL_RESULT: &'static str = "tool_result";
    pub const STATUS_CHANGED: &'static str = "status_changed";

    // Provenance, per `07 attach semantics` and `05 run state model`.
    pub const OPERATOR_INTERVENED: &'static str = "operator_intervened";
    pub const MANAGER_STEERED: &'static str = "manager_steered";

    /// A manager finished a turn and said something.
    ///
    /// Not the same as the run finishing. `10 runner inventory` observed that a
    /// Claude Code manager on live stdin emits its **own terminal result per
    /// turn** and stays alive for the next steer, so a run that answers three
    /// times has three of these and one `run_finished` - much later, when the
    /// operator closes it.
    ///
    /// Without this, `16 local api surface`'s promise that the answer arrives on
    /// the stream holds only for runs that exit, which a manager does not.
    pub const MANAGER_ANSWERED: &'static str = "manager_answered";

    /// Emitted when a worker reads memory through the MCP face. Carried into
    /// `02 record scope` from `11 analytics questions`, and it is what makes "which lessons actually reduced the
    /// failure rate" answerable at all.
    pub const MEMORY_CONSULTED: &'static str = "memory_consulted";

    /// `02 record scope`, amended 2026-08-23. Farseer can record **that** a compaction
    /// happened and **when**. It can never record what was dropped.
    pub const CONTEXT_COMPACTED: &'static str = "context_compacted";

    /// A nested run in the calling cell, per `06 cell transport` and `22 cell addressing`.
    pub const CELL_CALLED: &'static str = "cell_called";

    /// What the runner said about the session it opened: model, session id.
    ///
    /// Appended once, when the runner says it. Every field is optional because
    /// each is a claim a particular runner chose to make - Codex names a thread
    /// and no model, and `10 runner inventory`'s rule is that farseer reports
    /// what it observed rather than what was configured.
    pub const SESSION_STARTED: &'static str = "session_started";

    /// A context-window reading, per `29 harness protocol`.
    ///
    /// `used` out of `size`, plus a cumulative cost when the runner gives one.
    /// Only an ACP runner reports a **size** at all, which is the whole reason
    /// `29` admitted one: "how full is the context" was unanswerable while
    /// farseer only spoke dialects whose authors never sent the denominator.
    /// Cumulative, so the latest event is the answer and summing them is wrong.
    pub const USAGE_UPDATED: &'static str = "usage_updated";

    /// An ACP agent asked for permission farseer cannot give, per `29 harness protocol`.
    ///
    /// Recorded rather than dropped. Farseer runs unattended, so an unanswered
    /// `session/request_permission` is a live process producing nothing - the
    /// same hang `28 operator surface` paid for once when a granted tool was
    /// missing from `--allowedTools`. If this kind appears in a record, the
    /// session was opened in a mode that asks, and that is the bug.
    pub const PERMISSION_REQUESTED: &'static str = "permission_requested";

    /// An ACP session changed operating mode, per `29 harness protocol`.
    ///
    /// ACP's modes are `12 autonomy and deny list`'s ceiling one level down, and
    /// an agent may switch its own. A ceiling that moved without farseer asking
    /// belongs in the record.
    pub const MODE_CHANGED: &'static str = "mode_changed";

    /// A subscription window changed state, per `27 quota accounting`.
    ///
    /// Appended **on change only** and with `actor: system`: `10 runner inventory`
    /// measured this arriving on every successful run, so recording each one
    /// would bury the transitions. Current state derives from the latest.
    pub const RATE_LIMIT: &'static str = "rate_limit_event";

    /// A cell moved between active, paused, archived and deleted, per
    /// `17 cell lifecycle`.
    ///
    /// One kind rather than five, carrying `from` and `to`. The verbs differ in
    /// what they remove, not in what a reader of the record needs to ask, and a
    /// reader asking "when did this cell stop taking work" should not have to
    /// know four names to find out.
    pub const CELL_LIFECYCLE: &'static str = "cell_lifecycle";

    /// The tombstone purge leaves behind, per `17 cell lifecycle` section 5.
    ///
    /// Names the scope destroyed and when. `02 record scope` made the record
    /// **evidence**, and evidence with an unexplained hole in it is worth less
    /// than evidence that says where the hole came from - so a deliberate
    /// deletion and a lost write must not look identical.
    ///
    /// Appended after the delete, so its own `ts` is later than any range a
    /// purge could have just destroyed. A subsequent purge whose range reaches
    /// forward over this one does remove it, which is the honest behaviour: it
    /// is a record entry like any other, and purge is defined over the record.
    pub const CELL_PURGED: &'static str = "cell_purged";

    /// A permanent hole, per `17 cell lifecycle` section 5.
    ///
    /// The distinction `09 store decision` asked for. A `gap` is backpressure -
    /// the reader refetches the range and gets it. A `void` is a purge - the
    /// data is gone and refetching is a loop. A client that cannot tell them
    /// apart retries forever on the second, which is a bug that only appears
    /// after somebody purges.
    pub const VOID: &'static str = "void";

    pub fn new(kind: impl Into<String>) -> Self {
        Self(kind.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this kind belongs to the fleet view and the analytics queries.
    ///
    /// `05 run state model` split one concept into two: **activity** drives the liveness
    /// watchdog and is never recorded; **progress** drives the record. Token
    /// streams are activity, so they are absent from this list on purpose.
    pub fn is_progress(&self) -> bool {
        matches!(
            self.0.as_str(),
            Self::TOOL_CALL_STARTED
                | Self::TOOL_RESULT
                | Self::STATUS_CHANGED
                | Self::CONTEXT_COMPACTED
                | Self::USAGE_UPDATED
                | Self::PERMISSION_REQUESTED
                | Self::MODE_CHANGED
        )
    }
}

impl From<&str> for EventKind {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// An event on its way into the record. It has no `seq` yet, because `seq` is
/// assigned by the one owning writer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewEvent {
    pub event_id: EventId,
    /// Milliseconds since the Unix epoch. Supplied by the caller so this crate
    /// stays free of a clock.
    pub ts: i64,
    pub cell_id: CellId,
    pub run_id: RunId,
    pub kind: EventKind,
    pub actor: Actor,
    /// Kind-specific and independently versioned, per `02 record scope`.
    pub payload: serde_json::Value,
}

impl NewEvent {
    pub fn new(
        cell_id: CellId,
        run_id: RunId,
        kind: impl Into<EventKind>,
        actor: Actor,
        ts: i64,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_id: EventId::new(),
            ts,
            cell_id,
            run_id,
            kind: kind.into(),
            actor,
            payload,
        }
    }
}

/// An event as the record holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// The cursor. **Not contiguous after a purge**, per `09 store decision`, so nothing may
    /// infer how many events happened from a delta between two of these.
    pub seq: Seq,
    pub event_id: EventId,
    pub ts: i64,
    pub cell_id: CellId,
    pub run_id: RunId,
    pub kind: EventKind,
    pub actor: Actor,
    pub payload: serde_json::Value,
}

/// Which cells a reader may see memory from. `02 record scope` scopes memory by kind, not by
/// cell, and cross-cell reads beyond `global` are opt-in via the reader's own
/// definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTier {
    /// Operator preferences, tool gotchas, Windows workarounds. Readable by
    /// every cell, and `02 record scope` notes this is where nearly all the value sits.
    Global,
    /// Domain conventions, brand voice. The default write tier, per `25 memory lifecycle`.
    CellLocal,
    /// Scratch. Dies with the run.
    RunLocal,
}

impl MemoryTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::CellLocal => "cell_local",
            Self::RunLocal => "run_local",
        }
    }
}

impl std::str::FromStr for MemoryTier {
    type Err = UnknownTier;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "global" => Ok(Self::Global),
            "cell_local" => Ok(Self::CellLocal),
            "run_local" => Ok(Self::RunLocal),
            other => Err(UnknownTier(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown memory tier: {0}")]
pub struct UnknownTier(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_round_trips_through_its_wire_form() {
        for actor in [
            Actor::Manager,
            Actor::Worker,
            Actor::Operator,
            Actor::System,
        ] {
            assert_eq!(actor.as_str().parse::<Actor>().unwrap(), actor);
        }
    }

    #[test]
    fn tier_round_trips_through_its_wire_form() {
        for tier in [
            MemoryTier::Global,
            MemoryTier::CellLocal,
            MemoryTier::RunLocal,
        ] {
            assert_eq!(tier.as_str().parse::<MemoryTier>().unwrap(), tier);
        }
    }

    #[test]
    fn the_three_progress_kinds_are_progress_and_a_token_stream_is_not() {
        assert!(EventKind::new(EventKind::TOOL_CALL_STARTED).is_progress());
        assert!(EventKind::new(EventKind::TOOL_RESULT).is_progress());
        assert!(EventKind::new(EventKind::STATUS_CHANGED).is_progress());
        assert!(!EventKind::new("agent_message_chunk").is_progress());
    }

    #[test]
    fn an_adapter_may_emit_a_kind_farseer_does_not_name() {
        let kind = EventKind::new("acp.session_update");
        assert_eq!(kind.as_str(), "acp.session_update");
        assert!(!kind.is_progress());
    }
}
