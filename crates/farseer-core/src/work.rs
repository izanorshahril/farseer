//! Durable operator conversations and tasks.
//!
//! `40 work model and session explorer` keeps work identity separate from a
//! harness-owned session and keeps every kanban view as a projection over tasks.

use serde::{Deserialize, Serialize};

use crate::{Actor, ConversationId, RunId, TaskId};

/// One durable operator-visible thread that groups tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub conversation_id: ConversationId,
    pub title: String,
    pub project_path: Option<String>,
    pub manager_runner: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub archived_ts: Option<i64>,
}

/// The operator's whole request, spanning any number of runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub task_id: TaskId,
    pub conversation_id: ConversationId,
    pub goal: String,
    pub title: String,
    pub project_path: Option<String>,
    pub state: TaskState,
    pub priority: i32,
    pub created_ts: i64,
    pub updated_ts: i64,
}
/// One validated task-state change with durable provenance.
///
/// `40 work model and session explorer` requires every transition to retain
/// both who caused it and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTransition {
    pub task_id: TaskId,
    pub from: TaskState,
    pub to: TaskState,
    pub actor: Actor,
    pub reason: String,
    pub ts: i64,
}

/// One provider-owned harness conversation observed on a run.
///
/// `40 work model and session explorer` permits several per run and preserves
/// the provider's identifier kind instead of flattening threads and sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSession {
    pub run_id: RunId,
    pub identifier_kind: String,
    pub identifier: String,
    pub log_pointer: Option<String>,
    pub observed_ts: i64,
}

/// How much transcript custody the operator authorized.
///
/// `40 work model and session explorer` keeps raw bytes outside the event log;
/// each later mode strictly adds capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptCustody {
    Reference,
    Copy,
    CopyPlusIndex,
}

impl TranscriptCustody {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Copy => "copy",
            Self::CopyPlusIndex => "copy-plus-index",
        }
    }
}

impl std::fmt::Display for TranscriptCustody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TranscriptCustody {
    type Err = UnknownTranscriptCustody;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "reference" => Ok(Self::Reference),
            "copy" => Ok(Self::Copy),
            "copy-plus-index" => Ok(Self::CopyPlusIndex),
            _ => Err(UnknownTranscriptCustody(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown transcript custody mode: {0}")]
pub struct UnknownTranscriptCustody(pub String);

/// Board state belongs to the task, never to one of its runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    #[default]
    Inbox,
    Planned,
    InProgress,
    Blocked,
    Review,
    Done,
    Cancelled,
}

impl TaskState {
    pub const ALL: [Self; 7] = [
        Self::Inbox,
        Self::Planned,
        Self::InProgress,
        Self::Blocked,
        Self::Review,
        Self::Done,
        Self::Cancelled,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Planned => "planned",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether `to` is a valid command from this state.
    ///
    /// `40 work model and session explorer` requires runtime validation, while
    /// `02 record scope` keeps terminal evidence immutable.
    pub const fn allows(self, to: Self) -> bool {
        self as u8 == to as u8
            || matches!(
                (self, to),
                (
                    Self::Inbox,
                    Self::Planned | Self::InProgress | Self::Cancelled
                ) | (
                    Self::Planned,
                    Self::InProgress | Self::Blocked | Self::Cancelled
                ) | (
                    Self::InProgress,
                    Self::Blocked | Self::Review | Self::Cancelled
                ) | (
                    Self::Blocked,
                    Self::Planned | Self::InProgress | Self::Cancelled
                ) | (
                    Self::Review,
                    Self::InProgress | Self::Done | Self::Cancelled
                )
            )
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TaskState {
    type Err = UnknownTaskState;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == value)
            .ok_or_else(|| UnknownTaskState(value.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown task state: {0}")]
pub struct UnknownTaskState(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_tasks_cannot_be_reopened_silently() {
        assert!(!TaskState::Done.allows(TaskState::InProgress));
        assert!(!TaskState::Cancelled.allows(TaskState::Planned));
        assert!(TaskState::Review.allows(TaskState::Done));
        assert!(!TaskState::Inbox.allows(TaskState::Done));
        assert!(TaskState::Blocked.allows(TaskState::InProgress));
    }

    #[test]
    fn task_states_round_trip_through_the_store_spelling() {
        for state in TaskState::ALL {
            assert_eq!(state.as_str().parse::<TaskState>().unwrap(), state);
        }
    }

    #[test]
    fn transcript_custody_round_trips() {
        for mode in [
            TranscriptCustody::Reference,
            TranscriptCustody::Copy,
            TranscriptCustody::CopyPlusIndex,
        ] {
            assert_eq!(mode.as_str().parse::<TranscriptCustody>().unwrap(), mode);
        }
    }
}
