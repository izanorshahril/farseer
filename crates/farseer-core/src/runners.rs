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
    pub account: String,
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
            .map(|entry| entry.account.clone())
            .unwrap_or_else(|| runner.to_string())
    }

    /// Every runner that shares an account, for a display keyed by runner.
    ///
    /// `27 quota accounting` section 3: **the correct key for accounting and the
    /// natural key for display are different**, and that is fine as long as it
    /// is deliberate.
    pub fn runners_on(&self, account: &str) -> Vec<String> {
        let declared: Vec<String> = self
            .runners
            .iter()
            .filter(|(_, entry)| entry.account == account)
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
    }
}
