//! Farseer's domain model, as locked by the decision tickets under
//! `.scratch/farseer/issues/`.
//!
//! This crate is pure: no clock, no filesystem, no network. Anything needing
//! those takes them as arguments so the rules stay testable.

pub mod cell;
pub mod event;
pub mod ids;
pub mod policy;
pub mod run;
pub mod scrub;

pub use cell::{
    Advisory, CellDefinition, LoadError, Manager, RosterEntry, ValidationError, ValidationReport,
};
pub use event::{Actor, Event, EventKind, MemoryTier, NewEvent};
pub use ids::{CellId, EventId, MemoryId, RunId, Seq, TaskId};
pub use policy::{Budget, BudgetError, BudgetStack, Irreversibility, Policy, Spend};
pub use run::{
    ActivityClock, Control, Lifecycle, Liveness, LivenessThresholds, ManagerVerb, Outcome, Pause,
    WorkerContract, WorkerContractSpec, WorkspaceState, WorkspaceStrategy,
};
pub use scrub::scrub;
