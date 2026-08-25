//! Subscription windows: observed, never allocated.
//!
//! `27 quota accounting` settled the distinction this module exists to keep:
//! **a budget is allocated, a window is observed.** A budget is a pool a caller
//! grants and a callee draws down, per `23 prototype loose ends`. A window has
//! no caller to grant it, is shared simultaneously by every cell on the account,
//! and is drained by sessions farseer never started.
//!
//! So this is not a third budget dimension. It is `26 routing policy`'s runner
//! availability signal, recorded over time.
//!
//! **What is never here: a percentage.** `10 runner inventory` proved
//! `used_percentage` reaches only a status line that does not fire headless, and
//! farseer's own consumption is a **lower bound** on window usage - it would be
//! most wrong exactly near exhaustion, which is when the operator would trust it
//! most.

use serde::{Deserialize, Serialize};

/// `26 routing policy`'s runner availability signal, and `27 quota accounting`'s
/// accounting primitive. One mechanism, as `26 routing policy` predicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Availability {
    /// Allowed, and usually with a reset already known: `10 runner inventory`
    /// measured `resetsAt` arriving on **every** report, not only on refusal.
    /// Dropping it here left the countdown blank in the normal case, which is
    /// the only case the operator ever sees on a good day.
    Allowed { resets_at: Option<i64> },
    /// Unix **seconds**, as `10 runner inventory` measured `resetsAt` on this
    /// machine. Transcribed rather than converted, so a `grep` on the wire
    /// format finds the same number.
    ExhaustedUntil { resets_at: i64 },
    /// The runner said nothing about a window, which is the honest answer for
    /// every runner except Claude Code today.
    Unknown,
}

impl Availability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allowed { .. } => "allowed",
            Self::ExhaustedUntil { .. } => "exhausted_until",
            Self::Unknown => "unknown",
        }
    }

    /// When this window resets, whichever state it is in.
    ///
    /// This doubles as the window's **identity**: two observations naming the
    /// same reset are the same window, so a new one is a transition even while
    /// the status stays `allowed`.
    pub fn resets_at(self) -> Option<i64> {
        match self {
            Self::Allowed { resets_at } => resets_at,
            Self::ExhaustedUntil { resets_at } => Some(resets_at),
            Self::Unknown => None,
        }
    }
}

/// One runner's report about the window behind it.
///
/// Keyed by **account**, because two runners on one login share one window and
/// `27 quota accounting` found a runner-keyed count misleads the moment the
/// operator adds the second one. The account is declared in runner config and
/// never inferred, per `12 autonomy and deny list`'s rule that farseer does not
/// deduce a fact about identity it cannot observe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowObservation {
    pub account: String,
    pub runner: String,
    #[serde(flatten)]
    pub availability: Availability,
    /// Which limit the provider named - `10 runner inventory` transcribed
    /// `rateLimitType` from the payload rather than renaming it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rate_limit_type: String,
    #[serde(default)]
    pub is_using_overage: bool,
}

impl WindowObservation {
    /// Whether this observation is a **transition** rather than a repeat.
    ///
    /// `27 quota accounting` section 4: `10 runner inventory` measured
    /// `rate_limit_event` arriving on every successful run, and every concurrent
    /// run on one account reports the same window identically. Appending each
    /// one would bury the transitions in noise; appending only the changes makes
    /// "how often did this account exhaust, and for how long" a scan of a
    /// handful of rows.
    ///
    /// The runner is deliberately not compared: two runners sharing an account
    /// share a window, so the second one reporting the same state is the same
    /// state, not a change.
    pub fn differs_from(&self, previous: &Self) -> bool {
        self.account != previous.account
            || self.availability != previous.availability
            || self.is_using_overage != previous.is_using_overage
            || self.rate_limit_type != previous.rate_limit_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(availability: Availability) -> WindowObservation {
        WindowObservation {
            account: "anthropic-max".into(),
            runner: "claude-code".into(),
            availability,
            rate_limit_type: "five_hour".into(),
            is_using_overage: false,
        }
    }

    #[test]
    fn the_same_window_seen_twice_is_not_a_transition() {
        let first = observation(Availability::Allowed {
            resets_at: Some(1_787_003_600),
        });
        let again = observation(Availability::Allowed {
            resets_at: Some(1_787_003_600),
        });
        assert!(!again.differs_from(&first));
    }

    #[test]
    fn a_second_runner_on_one_account_reporting_the_same_window_is_not_a_transition() {
        // `27 quota accounting` section 3: two runners on one login share one
        // window, so this is the same fact arriving twice.
        let first = observation(Availability::Allowed {
            resets_at: Some(1_787_003_600),
        });
        let mut second = observation(Availability::Allowed {
            resets_at: Some(1_787_003_600),
        });
        second.runner = "claude-acp".into();
        assert!(!second.differs_from(&first));
    }

    #[test]
    fn a_new_window_is_a_transition_even_while_the_status_stays_allowed() {
        // `10 runner inventory`: `resetsAt` arrives on every report, so a fresh
        // five-hour window is visible without waiting for an exhaustion.
        let first = observation(Availability::Allowed {
            resets_at: Some(1_787_003_600),
        });
        let next = observation(Availability::Allowed {
            resets_at: Some(1_787_021_600),
        });
        assert!(next.differs_from(&first));
    }

    #[test]
    fn a_status_flip_and_a_moved_reset_are_both_transitions() {
        let allowed = observation(Availability::Allowed {
            resets_at: Some(1_787_003_600),
        });
        let exhausted = observation(Availability::ExhaustedUntil {
            resets_at: 1_787_000_000,
        });
        assert!(exhausted.differs_from(&allowed));

        let later = observation(Availability::ExhaustedUntil {
            resets_at: 1_787_003_600,
        });
        assert!(
            later.differs_from(&exhausted),
            "a new window is a transition even while the status stays the same"
        );
    }

    #[test]
    fn availability_never_serializes_a_percentage() {
        // `27 quota accounting` section 2: farseer's own consumption is a lower
        // bound, so any percentage here would be wrong in a way the operator
        // could not detect.
        let wire = serde_json::to_string(&observation(Availability::Allowed {
            resets_at: Some(1_787_003_600),
        }))
        .unwrap();
        // `rate_limit_type` is the provider's own field name and legitimately
        // contains "limit"; what must never appear is a quantity of the window.
        for absent in ["percent", "used_", "remaining", "quota_left"] {
            assert!(!wire.contains(absent), "`{absent}` must not reach the wire");
        }
    }
}
