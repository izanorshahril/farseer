//! The cell call: what a manager sends another cell.
//!
//! `14 vocabulary lock` retired one word that meant both this and a worker
//! contract, so the two nouns are separate here as well as in prose.
//!
//! `06 cell transport` section 4 fixed the difference, and it is the whole
//! reason a cell call is not a worker call: **a manager-to-worker contract
//! names the workspace, the runner and the tool grants, and a manager-to-cell
//! contract must not.** The callee owns those, and that ownership is what makes
//! it a cell rather than a worker.
//!
//! The caller states what it wants and what it will pay. The callee decides how.

use serde::{Deserialize, Serialize};

use crate::{Budget, CallId, CellId, Irreversibility};

/// One call from a cell to a cell.
///
/// Every field survives a JSON boundary cleanly even though the local path
/// never serializes it, because `06 cell transport` also ships an A2A endpoint that maps this
/// at the boundary and a Rust-only type here would not survive the mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellCall {
    pub call_id: CallId,
    pub from_cell: CellId,
    pub to_cell: CellId,
    pub goal: String,
    /// A **ceiling**, never a level.
    ///
    /// `06 cell transport`: a caller may cap a callee and may never raise it above the
    /// callee's own policy, because a ceiling composes safely under nesting and
    /// an absolute value does not.
    pub autonomy_ceiling: Irreversibility,
    pub budget: Budget,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub definition_of_done: String,
    /// Unix milliseconds, absent when the caller set none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> CellCall {
        CellCall {
            call_id: CallId::new(),
            from_cell: CellId::new("zero"),
            to_cell: CellId::new("social"),
            goal: "post the changelog".into(),
            autonomy_ceiling: Irreversibility::Reversible,
            budget: Budget::default(),
            definition_of_done: String::new(),
            deadline_ms: None,
        }
    }

    #[test]
    fn a_cell_call_names_no_workspace_no_runner_and_no_tool_grants() {
        // `06 cell transport`: the callee owns all three, and a call that named
        // them would have made the callee a worker.
        let wire = serde_json::to_string(&call()).unwrap();
        for owned_by_the_callee in ["workspace", "runner", "tool_grants"] {
            assert!(
                !wire.contains(owned_by_the_callee),
                "a cell call must not carry `{owned_by_the_callee}`"
            );
        }
    }

    #[test]
    fn a_cell_call_survives_a_json_round_trip_unchanged() {
        let call = call();
        let wire = serde_json::to_string(&call).unwrap();
        assert_eq!(serde_json::from_str::<CellCall>(&wire).unwrap(), call);
    }
}
