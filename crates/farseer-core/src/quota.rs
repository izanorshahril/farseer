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
//! **What is never here: a percentage farseer computed.** `27 quota accounting`
//! refused one because farseer's own consumption is a **lower bound** on window
//! usage - it would be most wrong exactly near exhaustion, which is when the
//! operator would trust it most. That reasoning is untouched.
//!
//! `30 codex app server` then found that `codex app-server` **pushes the
//! provider's own `usedPercent`**, headless, for two windows at once - which
//! `10 runner inventory` had measured as impossible against `codex exec`, the
//! other face of the same binary. A number the provider states is an
//! observation, and `10`'s observed-never-advertised rule admits it for exactly
//! the reason it admits `resetsAt`. So [`WindowObservation::used_percent`] is
//! **reported when a runner states it and absent otherwise**, and nothing here
//! ever calculates one.
//!
//! **A window is identified by its account *and* its limit.** One account can
//! have several running at once - Codex reports a five-hour and a weekly - and
//! keying only by account makes two windows look like one flapping between two
//! states.

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
    ///
    /// Also the **second half of a window's identity**: an account can be
    /// running more than one at a time, and Codex reports two.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rate_limit_type: String,
    #[serde(default)]
    pub is_using_overage: bool,
    /// How full the provider says this window is, when it says.
    ///
    /// **Never derived.** This module refuses to compute a percentage from
    /// farseer's own spend, and that refusal stands - see the module comment.
    /// This is the provider's own number, present only for a runner that states
    /// one, which today is `codex-app-server` and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<i64>,
    /// How long the provider says this window runs for, in minutes, when it
    /// says. Codex reports 300 and 10080; nothing else reports any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_duration_mins: Option<i64>,
    /// Which **provider** this window belongs to, in the provider's own id -
    /// `openai-codex`, `anthropic`, `cursor`, `xai-oauth`, `google-antigravity`.
    ///
    /// Separate from [`Self::account`], which is a login, because **one login
    /// spans several providers**: four of the five omp reports on this machine
    /// carry the same email, so grouping by account put a Cursor window, an xAI
    /// window and a Claude window under one heading and called it a
    /// subscription. The provider is what an operator is actually looking for.
    ///
    /// Absent for a window a run reported: `rate_limit_event` names a runner and
    /// a limit and never a provider, and `10 runner inventory`'s rule is that
    /// farseer transcribes what it was told rather than filling the gap in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// The provider's own name for this window - `5 hours`, `Claude 7 Day`,
    /// `Usage (Google)`. Transcribed, never composed: farseer used to build a
    /// name out of [`Self::window_duration_mins`] and got `1 day` for three
    /// different Antigravity windows that omp calls apart perfectly well.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
            // A window filling up is a transition worth recording even while the
            // status stays `allowed`: it is the only advance warning farseer has
            // ever been able to see, and `26 routing policy` was designed
            // believing no runner offered one.
            || self.used_percent != previous.used_percent
    }

    /// What identifies this window: the account it belongs to and the limit the
    /// provider named. Two of these can be live at once on one account.
    pub fn window_key(&self) -> (&str, &str) {
        (&self.account, &self.rate_limit_type)
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
            used_percent: None,
            window_duration_mins: None,
            provider: None,
            label: None,
        }
    }

    #[test]
    fn two_windows_on_one_account_are_two_windows() {
        // `30 codex app server`: Codex reports a five-hour and a weekly window
        // at once. Keyed by account alone they would look like one window
        // flapping between two states.
        let mut weekly = observation(Availability::Allowed {
            resets_at: Some(1788273509),
        });
        weekly.rate_limit_type = "weekly".into();
        let five_hour = observation(Availability::Allowed {
            resets_at: Some(1787710593),
        });
        assert_ne!(weekly.window_key(), five_hour.window_key());
        assert!(weekly.differs_from(&five_hour));
    }

    #[test]
    fn a_window_filling_up_is_a_transition_even_while_it_stays_allowed() {
        // The advance warning `26 routing policy` was designed believing no
        // runner offered.
        let mut before = observation(Availability::Allowed { resets_at: Some(1) });
        before.used_percent = Some(40);
        let mut after = before.clone();
        after.used_percent = Some(75);
        assert!(after.differs_from(&before));
    }

    #[test]
    fn a_percentage_is_only_ever_reported_never_computed() {
        // Nothing in this module takes spend and produces a percentage. The
        // field is `Option` because a runner that does not state one leaves it
        // absent, per `10 runner inventory`'s observed-never-advertised rule.
        let quiet = observation(Availability::Unknown);
        assert_eq!(quiet.used_percent, None);
        let json = serde_json::to_string(&quiet).unwrap();
        assert!(!json.contains("used_percent"), "{json}");
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
