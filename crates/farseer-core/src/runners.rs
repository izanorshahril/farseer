//! Runner config: the machine-wide facts about a runner, which are not a cell's
//! business.
//!
//! `13 harness build kit` kept git paths out of `CellDefinition` for the same
//! reason this file exists: a cell definition describes work, and the account a
//! runner signs in with is a fact about the machine. `27 quota accounting`
//! section 3 put the account string here and was explicit that it is **declared
//! by the operator, never inferred** - nothing in `10 runner inventory` makes
//! account-sharing reliably detectable, and `12 autonomy and deny list` forbids
//! deducing an identity fact farseer cannot observe.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The whole file, as the operator wrote it. An absent file is an empty config
/// rather than an error: declaring accounts is how the operator *improves*
/// accounting, not a precondition for running anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerConfig {
    #[serde(default, flatten)]
    pub runners: BTreeMap<String, RunnerEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerEntry {
    /// Runners sharing this string share one subscription window.
    ///
    /// `26 routing policy`'s price table belongs beside this when `26` is built.
    /// It is absent rather than stubbed, per `13 harness build kit`'s rule about
    /// fields a kit may not emit.
    ///
    /// Optional so the operator can pin a [`Self::model`] without also having to
    /// invent an account name; absent falls back to
    /// [`RunnerConfig::account_for`]'s own-name rule.
    #[serde(default)]
    pub account: Option<String>,
    /// Which model farseer launches this runner with.
    ///
    /// Absent means farseer passes no model at all and the runner uses whatever
    /// its own config says - the deference `30 codex app server` settled, where
    /// sending a value farseer invented would silently override the operator.
    /// Present here **is** the operator saying so, in farseer's own file, which
    /// is why this is config rather than a constant.
    #[serde(default)]
    pub model: Option<String>,
    /// What a million tokens costs on this runner, in USD micros.
    ///
    /// `26 routing policy`'s price table, which that ticket placed here rather
    /// than in a cell definition: pricing is per runner and machine-wide, so
    /// putting it in the definition would duplicate it into every cell and
    /// reopen `08 generalization test` for no gain.
    ///
    /// **Absent means farseer prices nothing**, which is the honest default:
    /// `10 runner inventory` found only some runners report currency at all,
    /// and a made-up figure in the record is worse than a blank one because an
    /// operator would plan around it. A run whose cost is derived from this
    /// carries `cost_estimated` beside it, per `26` - a routing decision made
    /// on a farseer estimate must be distinguishable from one made on a
    /// reported figure, or a mispriced table becomes invisible.
    ///
    /// One blended rate rather than input and output separately, because that
    /// is the granularity farseer can actually observe: pi and omp report a
    /// total token count and no split.
    #[serde(default)]
    pub usd_micros_per_mtok: Option<i64>,
    /// Whether farseer may run this runner's own usage command on a timer to
    /// read subscription windows it could not otherwise see.
    ///
    /// **Off unless the operator says so.** `33 google quota` found `omp usage
    /// --json` reports every account it is logged into - including a Google
    /// quota nothing else on this machine exposes - which is worth having and is
    /// still farseer launching somebody else's binary on its own initiative.
    /// `13 harness build kit` made the inventory a menu an author picks from,
    /// and a poll nobody chose is the opposite of that.
    #[serde(default)]
    pub usage_poll: bool,
    /// How hard the runner should think: `low`, `medium`, `high`, `xhigh` on
    /// this machine, in the runner's own vocabulary rather than a farseer one.
    ///
    /// `30 codex app server` decided farseer **reads** the effort and never
    /// writes one, so that an operator who configured `xhigh` is not quietly
    /// downgraded. This does not reverse that: farseer still invents nothing.
    /// It sends a value only when the operator wrote it down here, and the
    /// hint-with-provenance on `session_started` still reports what the runner
    /// says it is actually configured as.
    #[serde(default)]
    pub effort: Option<String>,
}

impl RunnerConfig {
    pub fn load(toml_text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_text)
    }

    /// Which account a runner's window belongs to.
    ///
    /// An undeclared runner is keyed by **its own name**, which is not an
    /// inference: it declines to merge two runners rather than guessing that
    /// they share a login. The failure mode is two windows shown where there is
    /// one, which the operator can see and fix with one line of config. The
    /// opposite guess would silently merge two accounts and misreport both.
    pub fn account_for(&self, runner: &str) -> String {
        self.runners
            .get(runner)
            .and_then(|entry| entry.account.clone())
            .unwrap_or_else(|| runner.to_string())
    }

    /// The model and effort the operator pinned for a runner, if any.
    ///
    /// Two `None`s rather than defaults: a runner farseer says nothing about is
    /// a runner using its own configuration, which is the only answer that
    /// cannot be wrong.
    pub fn launch_of(&self, runner: &str) -> (Option<&str>, Option<&str>) {
        let entry = self.runners.get(runner);
        (
            entry.and_then(|e| e.model.as_deref()),
            entry.and_then(|e| e.effort.as_deref()),
        )
    }

    /// Every runner that shares an account, for a display keyed by runner.
    ///
    /// `27 quota accounting` section 3: **the correct key for accounting and the
    /// natural key for display are different**, and that is fine as long as it
    /// is deliberate.
    /// What the operator says a million tokens costs on this runner.
    ///
    /// Absent for every runner until somebody writes one down - see
    /// [`RunnerEntry::usd_micros_per_mtok`].
    pub fn price_for(&self, runner: &str) -> Option<i64> {
        self.runners
            .get(runner)
            .and_then(|entry| entry.usd_micros_per_mtok)
    }

    /// Whether the operator asked farseer to poll this runner for usage.
    pub fn polls_usage(&self, runner: &str) -> bool {
        self.runners
            .get(runner)
            .is_some_and(|entry| entry.usage_poll)
    }

    pub fn runners_on(&self, account: &str) -> Vec<String> {
        let declared: Vec<String> = self
            .runners
            .iter()
            .filter(|(_, entry)| entry.account.as_deref() == Some(account))
            .map(|(runner, _)| runner.clone())
            .collect();
        if declared.is_empty() {
            vec![account.to_string()]
        } else {
            declared
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[claude-code]
account = "anthropic-max"

[claude-acp]
account = "anthropic-max"

[codex]
account = "openai-plus"
"#;

    /// Off unless the operator wrote it down. Starting farseer used to launch
    /// `omp` on its own initiative, which put a console window on the operator's
    /// screen and took the desktop shell down with it when they closed it.
    #[test]
    fn a_usage_poll_happens_only_where_an_operator_asked_for_one() {
        let config = RunnerConfig::load("[omp]
usage_poll = true

[pi]
account = \"x\"
")
            .expect("parses");
        assert!(config.polls_usage("omp"));
        assert!(!config.polls_usage("pi"), "declared, but not for this");
        assert!(!config.polls_usage("goose"), "undeclared runners poll nothing");
        assert!(
            !RunnerConfig::load("").expect("empty parses").polls_usage("omp"),
            "an absent config polls nothing at all"
        );
    }

    #[test]
    fn two_runners_declaring_one_account_share_one_window() {
        let config = RunnerConfig::load(CONFIG).unwrap();
        assert_eq!(config.account_for("claude-code"), "anthropic-max");
        assert_eq!(config.account_for("claude-acp"), "anthropic-max");
        assert_eq!(
            config.runners_on("anthropic-max"),
            ["claude-acp", "claude-code"]
        );
    }

    #[test]
    fn an_undeclared_runner_keys_by_its_own_name_rather_than_guessing_a_login() {
        let config = RunnerConfig::load(CONFIG).unwrap();
        assert_eq!(config.account_for("goose"), "goose");
        assert_eq!(config.runners_on("goose"), ["goose"]);
    }

    #[test]
    fn an_absent_config_is_empty_rather_than_an_error() {
        let config = RunnerConfig::default();
        assert_eq!(config.account_for("claude-code"), "claude-code");
        assert_eq!(config.launch_of("claude-code"), (None, None));
    }

    /// Pinning a model is not the same act as declaring an account, and an
    /// operator who wants one should not have to invent the other.
    #[test]
    fn a_runner_can_be_pinned_to_a_model_without_naming_an_account() {
        let config = RunnerConfig::load(
            r#"
[pi]
model = "openai-codex/gpt-5.6-luna"
effort = "low"
"#,
        )
        .unwrap();
        assert_eq!(
            config.launch_of("pi"),
            (Some("openai-codex/gpt-5.6-luna"), Some("low"))
        );
        assert_eq!(config.account_for("pi"), "pi");
    }

    /// Farseer invents nothing: a runner the operator said nothing about is
    /// launched with no model flag at all, per `30 codex app server`.
    #[test]
    fn an_unpinned_runner_keeps_its_own_configuration() {
        let config = RunnerConfig::load(CONFIG).unwrap();
        assert_eq!(config.launch_of("codex"), (None, None));
    }
}
