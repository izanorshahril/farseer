//! Policy: what a worker or a callee is allowed to do.
//!
//! `12 autonomy and deny list` settled that policy is four things, and that only the first is a real
//! boundary: tool grants, irreversibility level, autonomy ceiling, deny list.
//! Everything here only ever **narrows**.
//!
//! The deny list is not a security boundary. `12 autonomy and deny list` is explicit: `deny read .env`
//! is defeated by `cat .env`, so it stops a worker that did not intend harm and
//! nothing else. If shell is granted, everything is granted.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// How hard a tool call is to take back. `12 autonomy and deny list` chose three levels: two would
/// collapse "embarrassing but fixable" with "the money is gone", and a spectrum
/// would be unfalsifiable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Irreversibility {
    /// File writes inside a workspace. `04 spike workspace teardown` measured teardown, so these are
    /// fully reversible. Ungated.
    Reversible,
    /// Post, pull request, comment. Gated, and a cell may lower the gate.
    Undoable,
    /// Payment, email send, package publish, force push, purge. Gated, and the
    /// gate is **never** lowerable.
    Irreversible,
}

impl Irreversibility {
    /// The wire name, matching the `snake_case` serde rename so a definition
    /// file, a JSON payload and an MCP argument all spell a level the same way.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reversible => "reversible",
            Self::Undoable => "undoable",
            Self::Irreversible => "irreversible",
        }
    }

    /// The inverse of [`Irreversibility::as_str`], for arguments that arrive as text.
    ///
    /// Not `FromStr`: an unknown level is not an error worth a type, and every
    /// caller here wants `Option` at the point of the match.
    pub fn parse(level: &str) -> Option<Self> {
        match level {
            "reversible" => Some(Self::Reversible),
            "undoable" => Some(Self::Undoable),
            "irreversible" => Some(Self::Irreversible),
            _ => None,
        }
    }

    /// Whether an action at this level needs operator approval before it runs.
    pub fn gated_by_default(self) -> bool {
        self > Self::Reversible
    }

    /// Whether a cell definition may switch the gate off.
    ///
    /// `12 autonomy and deny list`: otherwise unattended payments are one edit to a definition file
    /// away, and `01 cell primitive` made definitions plain files in git precisely so they
    /// would be easy to edit.
    pub fn gate_is_lowerable(self) -> bool {
        self < Self::Irreversible
    }
}

/// The policy in force for a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// The most irreversible action permitted without a gate.
    pub autonomy_ceiling: Irreversibility,
    /// Advisory. Catches accidents cheaply; see the module note.
    #[serde(default)]
    pub deny: BTreeSet<String>,
    /// How many workers this cell may have in flight at once. `01 cell primitive` made a cell
    /// with zero workers legal, so zero is a coherent cap.
    #[serde(default = "default_worker_cap")]
    pub worker_cap: u32,
}

fn default_worker_cap() -> u32 {
    4
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            autonomy_ceiling: Irreversibility::Reversible,
            deny: BTreeSet::new(),
            worker_cap: default_worker_cap(),
        }
    }
}

impl Policy {
    /// Compose this policy with the one a call is entering.
    ///
    /// `12 autonomy and deny list`: **deny lists union, autonomy ceilings take the minimum, both only
    /// ever narrow.** The worker cap is local to a cell and does not compose.
    pub fn narrow(&self, callee: &Policy) -> Policy {
        Policy {
            autonomy_ceiling: self.autonomy_ceiling.min(callee.autonomy_ceiling),
            deny: self.deny.union(&callee.deny).cloned().collect(),
            worker_cap: callee.worker_cap,
        }
    }

    /// Whether an action at `level` may run without stopping for the operator.
    ///
    /// Two conditions, and both must hold: the level is within the ceiling, and
    /// the gate at that level is one a cell was allowed to lower. `Irreversible`
    /// fails the second even when a definition raises the ceiling to match it.
    pub fn permits_unattended(&self, level: Irreversibility) -> bool {
        level <= self.autonomy_ceiling && level.gate_is_lowerable()
    }
}

/// What a run actually consumed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spend {
    /// Millionths of a US dollar. Integer money, never a float.
    pub usd_micros: u64,
    pub tokens: u64,
    pub wall_secs: u64,
}

/// What a run is allowed to consume.
///
/// `23 prototype loose ends` makes `None` an unbounded dimension and permits partial serialized objects to omit such dimensions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Budget {
    pub usd_micros: Option<u64>,
    pub tokens: Option<u64>,
    pub wall_secs: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("budget exhausted: {dimension}")]
pub struct BudgetError {
    pub dimension: &'static str,
}

impl Budget {
    /// The largest budget a callee may be handed, given what this one has left.
    ///
    /// `23 prototype loose ends`: a ceiling is a level and is checked once; a budget is a quantity.
    pub fn cap_to(&self, requested: Budget) -> Budget {
        fn narrower(remaining: Option<u64>, asked: Option<u64>) -> Option<u64> {
            match (remaining, asked) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, b) => b,
            }
        }
        Budget {
            usd_micros: narrower(self.usd_micros, requested.usd_micros),
            tokens: narrower(self.tokens, requested.tokens),
            wall_secs: narrower(self.wall_secs, requested.wall_secs),
        }
    }

    /// Subtract a spend. Returns the exhausted dimension rather than saturating.
    ///
    /// Per `05 run state model`, exhaustion is a `failed` outcome, never `cancelled`: nobody
    /// chose it.
    pub fn draw(&mut self, spend: Spend) -> Result<(), BudgetError> {
        fn take(
            remaining: &mut Option<u64>,
            amount: u64,
            dimension: &'static str,
        ) -> Result<(), BudgetError> {
            let Some(left) = remaining else {
                return Ok(());
            };
            *left = left.checked_sub(amount).ok_or(BudgetError { dimension })?;
            Ok(())
        }
        take(&mut self.usd_micros, spend.usd_micros, "usd")?;
        take(&mut self.tokens, spend.tokens, "tokens")?;
        take(&mut self.wall_secs, spend.wall_secs, "wall_secs")
    }
}

/// The budgets on a call path, outermost first.
///
/// `23 prototype loose ends` made budgets **draw down rather than be compared**, which is what stops
/// three sequential $2 calls spending $6 under a $2 parent. Encoding the path as
/// a stack is what makes that structural rather than a rule someone remembers.
#[derive(Debug, Clone, Default)]
pub struct BudgetStack(Vec<Budget>);

impl BudgetStack {
    pub fn new(root: Budget) -> Self {
        Self(vec![root])
    }

    /// Enter a nested call, capped by whatever the caller has left.
    pub fn push(&mut self, requested: Budget) {
        let capped = match self.0.last() {
            Some(caller) => caller.cap_to(requested),
            None => requested,
        };
        self.0.push(capped);
    }

    /// Leave a nested call. The spend already drawn stays drawn on the caller.
    pub fn pop(&mut self) {
        if self.0.len() > 1 {
            self.0.pop();
        }
    }

    /// Draw a spend against every level on the path.
    ///
    /// Applied whole or not at all, so a partial draw never leaves the stack
    /// disagreeing with itself about what was spent.
    pub fn draw(&mut self, spend: Spend) -> Result<(), BudgetError> {
        let mut trial = self.0.clone();
        for budget in trial.iter_mut() {
            budget.draw(spend)?;
        }
        self.0 = trial;
        Ok(())
    }

    /// What the innermost call has left.
    pub fn remaining(&self) -> Budget {
        self.0.last().copied().unwrap_or_default()
    }

    pub fn depth(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::Irreversibility::*;
    use super::*;

    fn usd(dollars: u64) -> Budget {
        Budget {
            usd_micros: Some(dollars * 1_000_000),
            ..Budget::default()
        }
    }

    fn spent(dollars: u64) -> Spend {
        Spend {
            usd_micros: dollars * 1_000_000,
            ..Spend::default()
        }
    }

    #[test]
    fn levels_order_by_how_hard_they_are_to_take_back() {
        assert!(Reversible < Undoable && Undoable < Irreversible);
    }

    #[test]
    fn only_reversible_runs_ungated() {
        assert!(!Reversible.gated_by_default());
        assert!(Undoable.gated_by_default());
        assert!(Irreversible.gated_by_default());
    }

    #[test]
    fn the_top_level_gate_is_never_lowerable() {
        assert!(Undoable.gate_is_lowerable());
        assert!(!Irreversible.gate_is_lowerable());
    }

    #[test]
    fn a_ceiling_raised_to_the_top_still_does_not_run_a_payment_unattended() {
        let permissive = Policy {
            autonomy_ceiling: Irreversible,
            ..Policy::default()
        };
        assert!(permissive.permits_unattended(Undoable));
        assert!(!permissive.permits_unattended(Irreversible));
    }

    #[test]
    fn a_ceiling_takes_the_minimum_and_a_caller_cannot_raise_it() {
        let caller = Policy {
            autonomy_ceiling: Reversible,
            ..Policy::default()
        };
        let permissive_callee = Policy {
            autonomy_ceiling: Irreversible,
            ..Policy::default()
        };
        assert_eq!(
            caller.narrow(&permissive_callee).autonomy_ceiling,
            Reversible
        );
        assert_eq!(
            permissive_callee.narrow(&caller).autonomy_ceiling,
            Reversible
        );
    }

    #[test]
    fn deny_lists_union_down_the_chain() {
        let a = Policy {
            deny: ["push".to_string()].into(),
            ..Policy::default()
        };
        let b = Policy {
            deny: ["publish".to_string()].into(),
            ..Policy::default()
        };
        let composed = a.narrow(&b);
        assert!(composed.deny.contains("push") && composed.deny.contains("publish"));
    }

    #[test]
    fn narrowing_is_transitive_across_three_cells() {
        let a = Policy {
            autonomy_ceiling: Undoable,
            deny: ["x".into()].into(),
            ..Policy::default()
        };
        let b = Policy {
            autonomy_ceiling: Irreversible,
            deny: ["y".into()].into(),
            ..Policy::default()
        };
        let c = Policy {
            autonomy_ceiling: Reversible,
            deny: ["z".into()].into(),
            ..Policy::default()
        };
        let effective = a.narrow(&b).narrow(&c);
        assert_eq!(effective.autonomy_ceiling, Reversible);
        assert_eq!(effective.deny.len(), 3);
    }

    #[test]
    fn partial_budget_objects_default_omitted_dimensions_in_toml_and_json() {
        let from_toml: Budget = toml::from_str("tokens = 42").unwrap();
        assert_eq!(
            from_toml,
            Budget {
                tokens: Some(42),
                ..Budget::default()
            }
        );

        let from_json: Budget = serde_json::from_str(r#"{"wall_secs": 30}"#).unwrap();
        assert_eq!(
            from_json,
            Budget {
                wall_secs: Some(30),
                ..Budget::default()
            }
        );
    }

    #[test]
    fn a_budget_draws_down_and_names_the_dimension_it_ran_out_of() {
        let mut b = usd(2);
        assert!(b.draw(spent(1)).is_ok());
        assert_eq!(b.usd_micros, Some(1_000_000));
        assert_eq!(b.draw(spent(2)).unwrap_err().dimension, "usd");
    }

    #[test]
    fn an_unbounded_dimension_never_exhausts() {
        let mut b = Budget::default();
        assert!(b.draw(spent(1_000_000)).is_ok());
    }

    #[test]
    fn three_sequential_two_dollar_calls_cannot_spend_six_under_a_two_dollar_parent() {
        let mut stack = BudgetStack::new(usd(2));
        let mut spent_total = 0;
        for _ in 0..3 {
            stack.push(usd(2));
            if stack.draw(spent(2)).is_ok() {
                spent_total += 2;
            }
            stack.pop();
        }
        assert_eq!(spent_total, 2, "the parent pool bounds the whole path");
        assert_eq!(stack.remaining().usd_micros, Some(0));
    }

    #[test]
    fn a_nested_call_is_capped_by_what_the_caller_has_left() {
        let mut stack = BudgetStack::new(usd(5));
        stack.draw(spent(4)).unwrap();
        stack.push(usd(10));
        assert_eq!(stack.remaining().usd_micros, Some(1_000_000));
    }

    #[test]
    fn a_draw_that_fails_on_the_caller_leaves_the_callee_untouched() {
        // The caller bounds tokens, the callee bounds dollars. A spend within
        // the callee's dollars but past the caller's tokens must roll back.
        let mut stack = BudgetStack::new(Budget {
            tokens: Some(100),
            ..Budget::default()
        });
        stack.push(usd(5));
        let overrun = Spend {
            usd_micros: 1_000_000,
            tokens: 500,
            ..Spend::default()
        };
        assert_eq!(stack.draw(overrun).unwrap_err().dimension, "tokens");
        assert_eq!(stack.remaining().usd_micros, Some(5_000_000));
        stack.pop();
        assert_eq!(stack.remaining().tokens, Some(100));
    }
}
