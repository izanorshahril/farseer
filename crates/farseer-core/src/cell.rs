//! The cell definition: data in git, never a loadable plugin.
//!
//! `13 harness build kit` assembled the minimum field list from six closed tickets and found it
//! fits on one page, which is the falsification test `01 cell primitive` set and `08 generalization test` passed.
//! Adding a field here is not a small act - `08 generalization test` proved the coding cell and the
//! social cell differ only in roster, tools and policy values.
//! `23 prototype loose ends` later required the task-root budget policy value and a per-worker cap; its correction to `13 harness build kit` records why those additions preserve the test rather than silently reopening it.
//!
//! Explicitly **not** here, per `13 harness build kit` section 7: no review mode, no scheduling,
//! no credential store, no git flag, no delivery-gate field, no `cell_kind`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::ids::CellId;
use crate::policy::{Budget, Irreversibility, Policy};
use crate::run::WorkspaceStrategy;

/// The mandatory manager. `01 cell primitive`: every cell has one, and only a manager may
/// spawn a worker or call another cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manager {
    /// A name from the runner inventory. `10 runner inventory` made the inventory a menu an
    /// author picks from, not a survey.
    pub runner: String,
    #[serde(default)]
    pub prompt: String,
}

/// What a cell may use.
///
/// `22 cell addressing` widened this from "workers and tools" to "workers, tools and callable
/// cells". Not a new field, which is why `08 generalization test`'s test survived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RosterEntry {
    /// Supervised, has a run, is cancellable. `01 cell primitive` classifies by supervision,
    /// not by whether an LLM is involved.
    Worker {
        name: String,
        runner: String,
        /// `23 prototype loose ends` makes this the per-callee cap, narrowed again by the caller's remaining task budget.
        #[serde(default)]
        max_budget: Budget,
    },
    /// A call that returns or errors. Declares its own irreversibility level,
    /// which policy then gates on, per `12 autonomy and deny list`.
    Tool {
        name: String,
        irreversibility: Irreversibility,
        /// Whether this tool reaches a shell. `12 autonomy and deny list`: **if shell is granted,
        /// everything is granted**, and the deny list becomes advisory. Recorded
        /// so that is a stated choice rather than an assumed one.
        #[serde(default)]
        grants_shell: bool,
    },
    /// A nested run in this cell, per `06 cell transport` and `22 cell addressing`.
    Cell {
        name: String,
        cell_id: CellId,
        /// The most the caller may grant this callee. Composition still only
        /// narrows: effective is the minimum of this, whatever the caller
        /// passes, and the callee's own policy.
        max_autonomy_ceiling: Irreversibility,
        /// `23 prototype loose ends` makes this the per-callee budget cap,
        /// narrowed again by the caller's remaining task budget.
        #[serde(default)]
        max_budget: Budget,
        /// A foreign A2A orchestrator rather than a local cell. `21 a2a conformance` found such
        /// a callee **silently ignores four of eight cell-call fields**, so `12 autonomy and deny list`
        /// pins it at the top level and it cannot be lowered.
        #[serde(default)]
        peer: bool,
    },
}

impl RosterEntry {
    pub fn name(&self) -> &str {
        match self {
            Self::Worker { name, .. } | Self::Tool { name, .. } | Self::Cell { name, .. } => name,
        }
    }
}

/// Which other cells' `cell_local` memory this cell may read.
///
/// `02 record scope`: the `global` tier is readable by every cell and needs no declaration.
/// Cross-cell reads beyond it are opt-in via the **reader's** definition, never
/// blanket - a coding cell inheriting brand voice is noise.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordScope {
    #[serde(default)]
    pub also_read: BTreeSet<CellId>,
}

/// A cell, as the operator wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellDefinition {
    /// **Stable across reload, never derived from content**, per `17 cell lifecycle`. Content
    /// changes on every edit, and `06 cell transport` needs this to survive a reload or the
    /// record loses its join key and history detaches from the cell.
    pub cell_id: CellId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Plain git handles versioning and rollback, per `13 harness build kit` section 4. This is
    /// the label `17 cell lifecycle` pins per run so a rollback never reaches into work already
    /// executing.
    #[serde(default)]
    pub version: String,
    pub manager: Manager,
    /// `23 prototype loose ends` makes this the task-root pool that every delegated call draws down.
    #[serde(default)]
    pub budget: Budget,
    /// Zero entries is legal, per `01 cell primitive`. A worker may never spawn.
    #[serde(default)]
    pub roster: Vec<RosterEntry>,
    pub workspace_strategy: WorkspaceStrategy,
    #[serde(default)]
    pub policy: Policy,
    #[serde(default)]
    pub record_scope: RecordScope,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("cell_id is empty; `17 cell lifecycle` requires a stable id that survives a reload")]
    EmptyCellId,
    #[error("name is empty; `21 a2a conformance` generates the A2A agent card from it")]
    EmptyName,
    #[error("manager.runner is empty; `01 cell primitive` makes the manager mandatory")]
    EmptyManagerRunner,
    #[error("roster entry `{0}` is declared more than once")]
    DuplicateRosterName(String),
    #[error(
        "roster cell `{0}` is a foreign peer but its ceiling is `{1:?}`; \
         `21 a2a conformance` pins a peer at `irreversible` and it cannot be lowered"
    )]
    PeerCeilingLowered(String, Irreversibility),
    #[error(
        "roster cell `{0}` calls this cell itself; `22 cell addressing` refuses a cycle on the call path"
    )]
    SelfCall(String),
}

/// Something true about the definition that the operator should have chosen
/// deliberately. Not an error: the definition still loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advisory {
    /// `12 autonomy and deny list`: `deny read .env` is defeated by `cat .env`. A cell whose roster
    /// includes a shell has accepted that its deny list is advisory.
    DenyListIsAdvisory { shell_tool: String },
    /// `13 harness build kit`: a definition with no workers is coherent, and the social cell being
    /// thinner than the coding cell is a fact about the domain, not a smell.
    NoWorkers,
}

impl std::fmt::Display for Advisory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DenyListIsAdvisory { shell_tool } => write!(
                f,
                "tool `{shell_tool}` reaches a shell, so the deny list is advisory for this cell"
            ),
            Self::NoWorkers => f.write_str("roster declares no workers; this cell delegates only"),
        }
    }
}

/// What `16 local api surface`'s `validate` returns: enough to tell a broken definition from a
/// working one without a restart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
    pub advisories: Vec<Advisory>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cell definition is not valid TOML: {0}")]
    Syntax(#[from] toml::de::Error),
    #[error("cell definition is invalid: {}", .0.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<ValidationError>),
}

impl CellDefinition {
    /// Parse and validate in one step. `16 local api surface` gives the API read, validate and
    /// reload, and no edit path: definitions are files, and the operator already
    /// has an editor and a diff tool.
    pub fn load(toml_text: &str) -> Result<(Self, Vec<Advisory>), LoadError> {
        let definition: Self = toml::from_str(toml_text)?;
        let report = definition.validate();
        if !report.is_valid() {
            return Err(LoadError::Invalid(report.errors));
        }
        Ok((definition, report.advisories))
    }

    pub fn validate(&self) -> ValidationReport {
        let mut report = ValidationReport::default();

        if self.cell_id.as_str().trim().is_empty() {
            report.errors.push(ValidationError::EmptyCellId);
        }
        if self.name.trim().is_empty() {
            report.errors.push(ValidationError::EmptyName);
        }
        if self.manager.runner.trim().is_empty() {
            report.errors.push(ValidationError::EmptyManagerRunner);
        }

        let mut seen = BTreeSet::new();
        for entry in &self.roster {
            if !seen.insert(entry.name()) {
                report.errors.push(ValidationError::DuplicateRosterName(
                    entry.name().to_string(),
                ));
            }
            match entry {
                RosterEntry::Cell {
                    name,
                    cell_id,
                    max_autonomy_ceiling,
                    peer,
                    ..
                } => {
                    if *peer && *max_autonomy_ceiling != Irreversibility::Irreversible {
                        report.errors.push(ValidationError::PeerCeilingLowered(
                            name.clone(),
                            *max_autonomy_ceiling,
                        ));
                    }
                    if *cell_id == self.cell_id {
                        report.errors.push(ValidationError::SelfCall(name.clone()));
                    }
                }
                RosterEntry::Tool {
                    name, grants_shell, ..
                } if *grants_shell => {
                    report.advisories.push(Advisory::DenyListIsAdvisory {
                        shell_tool: name.clone(),
                    });
                }
                _ => {}
            }
        }

        if !self
            .roster
            .iter()
            .any(|e| matches!(e, RosterEntry::Worker { .. }))
        {
            report.advisories.push(Advisory::NoWorkers);
        }

        report
    }

    /// Whether the roster explicitly grants a shell-reaching tool.
    ///
    /// `12 autonomy and deny list` says a shell grant is equivalent to every tool grant, so native LLM runner reach must be stated rather than assumed.
    pub fn has_shell_grant(&self) -> bool {
        self.roster.iter().any(|entry| {
            matches!(
                entry,
                RosterEntry::Tool {
                    grants_shell: true,
                    ..
                }
            )
        })
    }

    /// The tool grants this definition confers, for a worker contract.
    pub fn tool_grants(&self) -> Vec<String> {
        self.roster
            .iter()
            .filter_map(|e| match e {
                RosterEntry::Tool { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// The effective ceiling for calling `name`, given what the caller offers.
    ///
    /// `22 cell addressing` section 3: an ungranted cell stays ungranted even if the operator
    /// names it. A mechanism that yields to conversational pressure is not a
    /// mechanism, and the fix - edit the definition and `reload` - takes about
    /// ten seconds and leaves a git commit.
    pub fn ceiling_for_cell_call(
        &self,
        name: &str,
        offered: Irreversibility,
    ) -> Option<Irreversibility> {
        self.roster.iter().find_map(|e| match e {
            RosterEntry::Cell {
                name: n,
                max_autonomy_ceiling,
                ..
            } if n == name => Some((*max_autonomy_ceiling).min(offered)),
            _ => None,
        })
    }

    /// The composed policy for a call into `callee`, per `12 autonomy and deny list`.
    pub fn policy_for_call(&self, callee: &CellDefinition) -> Policy {
        self.policy.narrow(&callee.policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODING: &str = r#"
cell_id = "zero"
name = "Cell Zero"
description = "The builder harness."
version = "1"
workspace_strategy = "worktree"
budget = { usd_micros = 5_000_000 }

[manager]
runner = "claude-code"
prompt = "You coordinate."

[[roster]]
kind = "worker"
name = "coder"
runner = "codex"
max_budget = { tokens = 100_000 }

[[roster]]
kind = "tool"
name = "shell"
irreversibility = "irreversible"
grants_shell = true

[[roster]]
kind = "cell"
name = "social"
cell_id = "social"
max_autonomy_ceiling = "undoable"
max_budget = { usd_micros = 2_000_000 }

[policy]
autonomy_ceiling = "reversible"
deny = ["git push"]
worker_cap = 4

[record_scope]
also_read = ["social"]
"#;

    #[test]
    fn a_hand_written_definition_loads_and_keeps_its_shape() {
        let (cell, _) = CellDefinition::load(CODING).unwrap();
        assert_eq!(cell.cell_id.as_str(), "zero");
        assert_eq!(cell.manager.runner, "claude-code");
        assert_eq!(cell.roster.len(), 3);
        assert_eq!(cell.budget.usd_micros, Some(5_000_000));
        assert!(matches!(
            cell.roster.first(),
            Some(RosterEntry::Worker { max_budget, .. }) if max_budget.tokens == Some(100_000)
        ));
        assert!(matches!(
            cell.roster.get(2),
            Some(RosterEntry::Cell { max_budget, .. })
                if max_budget.usd_micros == Some(2_000_000)
        ));
        assert_eq!(cell.tool_grants(), ["shell"]);
        assert_eq!(cell.policy.worker_cap, 4);
        assert!(cell.record_scope.also_read.contains(&CellId::new("social")));
    }

    #[test]
    fn a_cell_with_an_empty_roster_is_coherent() {
        // `08 generalization test`'s falsification test, expressed as a test.
        let toml_text = r#"
cell_id = "thin"
name = "Thin"
workspace_strategy = "plain_directory"

[manager]
runner = "claude-code"
"#;
        let (cell, advisories) = CellDefinition::load(toml_text).unwrap();
        assert!(cell.roster.is_empty());
        assert_eq!(advisories, [Advisory::NoWorkers]);
    }

    #[test]
    fn omitted_task_and_worker_budgets_are_unbounded() {
        let toml_text = r#"
cell_id = "defaults"
name = "Defaults"
workspace_strategy = "plain_directory"

[manager]
runner = "claude-code"

[[roster]]
kind = "worker"
name = "coder"
runner = "codex"
"#;
        let (cell, _) = CellDefinition::load(toml_text).unwrap();
        assert_eq!(cell.budget, Budget::default());
        assert!(matches!(
            cell.roster.first(),
            Some(RosterEntry::Worker { max_budget, .. }) if *max_budget == Budget::default()
        ));
    }

    #[test]
    fn an_omitted_callable_cell_budget_is_unbounded() {
        let toml_text = r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "plain_directory"

[manager]
runner = "claude-code"

[[roster]]
kind = "cell"
name = "social"
cell_id = "social"
max_autonomy_ceiling = "undoable"
"#;
        let (cell, _) = CellDefinition::load(toml_text).unwrap();
        assert!(matches!(
            cell.roster.first(),
            Some(RosterEntry::Cell { max_budget, .. }) if *max_budget == Budget::default()
        ));
    }

    #[test]
    fn granting_a_shell_states_that_the_deny_list_is_advisory() {
        let (cell, advisories) = CellDefinition::load(CODING).unwrap();
        assert!(cell.has_shell_grant());
        assert!(advisories.contains(&Advisory::DenyListIsAdvisory {
            shell_tool: "shell".to_string()
        }));
    }

    #[test]
    fn a_shell_reaching_runner_does_not_replace_an_explicit_shell_tool_grant() {
        let toml_text = r#"
cell_id = "implicit-shell"
name = "Implicit shell"
workspace_strategy = "plain_directory"

[manager]
runner = "claude-code"

[[roster]]
kind = "worker"
name = "coder"
runner = "codex"
"#;
        let (cell, _) = CellDefinition::load(toml_text).unwrap();
        assert!(!cell.has_shell_grant());
    }

    #[test]
    fn a_foreign_peer_pinned_below_the_top_level_is_rejected() {
        let toml_text = r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "worktree"

[manager]
runner = "claude-code"

[[roster]]
kind = "cell"
name = "someone-elses-orchestrator"
cell_id = "remote"
max_autonomy_ceiling = "undoable"
peer = true
"#;
        let err = CellDefinition::load(toml_text).unwrap_err();
        assert!(
            matches!(err, LoadError::Invalid(ref e) if matches!(e[0], ValidationError::PeerCeilingLowered(..)))
        );
    }

    #[test]
    fn a_cell_that_lists_itself_as_callable_is_rejected() {
        let toml_text = r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "worktree"

[manager]
runner = "claude-code"

[[roster]]
kind = "cell"
name = "me"
cell_id = "zero"
max_autonomy_ceiling = "reversible"
"#;
        let err = CellDefinition::load(toml_text).unwrap_err();
        assert!(
            matches!(err, LoadError::Invalid(ref e) if matches!(e[0], ValidationError::SelfCall(..)))
        );
    }

    #[test]
    fn a_duplicated_roster_name_is_rejected() {
        let toml_text = r#"
cell_id = "zero"
name = "Cell Zero"
workspace_strategy = "worktree"

[manager]
runner = "claude-code"

[[roster]]
kind = "tool"
name = "shell"
irreversibility = "irreversible"

[[roster]]
kind = "worker"
name = "shell"
runner = "codex"
"#;
        let err = CellDefinition::load(toml_text).unwrap_err();
        assert!(
            matches!(err, LoadError::Invalid(ref e) if matches!(e[0], ValidationError::DuplicateRosterName(..)))
        );
    }

    #[test]
    fn a_typo_in_a_field_name_is_an_error_rather_than_silence() {
        let toml_text = r#"
cell_id = "zero"
name = "Cell Zero"
workspace_stratergy = "worktree"

[manager]
runner = "claude-code"
"#;
        assert!(matches!(
            CellDefinition::load(toml_text),
            Err(LoadError::Syntax(_))
        ));
    }

    #[test]
    fn an_ungranted_cell_stays_ungranted_even_when_named() {
        let (cell, _) = CellDefinition::load(CODING).unwrap();
        assert_eq!(
            cell.ceiling_for_cell_call("social", Irreversibility::Irreversible),
            Some(Irreversibility::Undoable),
            "the roster entry caps what the caller may offer"
        );
        assert_eq!(
            cell.ceiling_for_cell_call("finance", Irreversibility::Reversible),
            None,
            "a cell absent from the roster is not callable"
        );
    }
}
