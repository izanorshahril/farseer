//! Subscription windows read from omp, for every account it is logged into.
//!
//! `33 google quota` closed on 2026-08-27 with a **negative answer**: the
//! Antigravity quota exists, is metered hard, and "is simply not exposed
//! anywhere a runner adapter can read". Four routes were checked and all four
//! were closed.
//!
//! That answer was correct about `agy` and wrong about the world.
//! `omp usage --json` reports it - along with Codex's two windows - because omp
//! holds the same credential. **The re-check condition named the wrong binary:**
//! it waited for the harness that owns the account to grow a usage command, and
//! the answer came from a harness that merely shares the login.
//!
//! This is a **poll, not a run**. Every other window farseer records arrives on
//! a run's own stream, so an idle farseer learned nothing; this reports while
//! nothing is running, which is exactly when an operator asks.
//!
//! `27 quota accounting`'s rules carry over unchanged:
//!
//! - **Never a percentage farseer computed.** `used` here is the provider's own
//!   number, passed through, present only where the unit says `percent`.
//! - **A window is identified by its account *and* its limit.** omp gives three
//!   Antigravity windows that all report `window.id: "daily"`, so the limit's
//!   own `id` is the discriminator - keying on the window id would make three
//!   windows look like one flapping between three states.

use farseer_core::{Availability, WindowObservation};
use serde_json::Value;

/// The argv. `--json` because the default output is a bar chart for a human.
pub fn build_args() -> Vec<String> {
    vec!["usage".to_string(), "--json".to_string()]
}

/// Every window omp reports, in farseer's own shape.
///
/// A malformed or unexpected document yields an empty list rather than an
/// error: this is a side-channel that improves a view, and `10 runner
/// inventory`'s rule is that farseer reports what it observed - not that it
/// fails when a tool it does not own changes its mind.
pub fn parse(document: &str) -> Vec<WindowObservation> {
    let Ok(root) = serde_json::from_str::<Value>(document) else {
        return Vec::new();
    };
    root.get("reports")
        .and_then(Value::as_array)
        .map(|reports| reports.iter().flat_map(report).collect())
        .unwrap_or_default()
}

fn report(report: &Value) -> Vec<WindowObservation> {
    // Stated by the provider, not inferred by farseer - the same standing
    // `27 quota accounting` gives `used_percent`. Falling back to the provider
    // id keeps a login farseer cannot name from silently joining another one's
    // window.
    let account = report
        .pointer("/metadata/email")
        .and_then(Value::as_str)
        .or_else(|| report.get("provider").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();

    report
        .get("limits")
        .and_then(Value::as_array)
        .map(|limits| {
            limits
                .iter()
                .filter_map(|limit| observation(&account, limit))
                .collect()
        })
        .unwrap_or_default()
}

fn observation(account: &str, limit: &Value) -> Option<WindowObservation> {
    let id = limit.get("id").and_then(Value::as_str)?;
    // Milliseconds here; `10 runner inventory` transcribed Claude Code's
    // `resetsAt` as **seconds** and the record is in seconds, so this is the one
    // place a unit is converted rather than passed through.
    let resets_at = limit
        .pointer("/window/resetsAt")
        .and_then(Value::as_i64)
        .map(|ms| ms / 1_000);

    let ok = limit.get("status").and_then(Value::as_str) == Some("ok");
    let availability = match (ok, resets_at) {
        (true, resets_at) => Availability::Allowed { resets_at },
        // Anything that is not `ok` is treated as exhausted, which is the call
        // `27 quota accounting` already made for every other runner.
        (false, Some(resets_at)) => Availability::ExhaustedUntil { resets_at },
        (false, None) => Availability::Unknown,
    };

    Some(WindowObservation {
        account: account.to_string(),
        runner: "omp".to_string(),
        availability,
        rate_limit_type: id.to_string(),
        is_using_overage: false,
        used_percent: (limit.pointer("/amount/unit").and_then(Value::as_str) == Some("percent"))
            .then(|| limit.pointer("/amount/used").and_then(Value::as_i64))
            .flatten(),
        window_duration_mins: limit
            .pointer("/window/durationMs")
            .and_then(Value::as_i64)
            .map(|ms| ms / 60_000),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A literal trim of `omp usage --json` on this machine, 2026-08-28, kept
    /// whole enough to carry the two shapes that matter: Codex's two windows on
    /// one login, and Antigravity's three that share a window id.
    const FIXTURE: &str = r#"{
      "generatedAt": 1787917869625,
      "reports": [
        {"provider":"openai-codex","limits":[
          {"id":"openai-codex:primary","label":"5 hours",
           "window":{"id":"5h","durationMs":18000000,"resetsAt":1787933835000},
           "amount":{"used":12,"limit":100,"unit":"percent"},"status":"ok"},
          {"id":"openai-codex:secondary","label":"7 days",
           "window":{"id":"7d","durationMs":604800000,"resetsAt":1788456114000},
           "amount":{"used":0,"limit":100,"unit":"percent"},"status":"exhausted"}],
         "metadata":{"planType":"plus","email":"abah.intelek@gmail.com"}},
        {"provider":"google-antigravity","limits":[
          {"id":"google-antigravity:google:default:daily","label":"Usage (Google)",
           "window":{"id":"daily","durationMs":86400000,"resetsAt":1787935859000},
           "amount":{"used":0,"limit":100,"unit":"percent"},"status":"ok"},
          {"id":"google-antigravity:anthropic:default:daily","label":"Usage (Anthropic)",
           "window":{"id":"daily","durationMs":86400000,"resetsAt":1787935859000},
           "amount":{"used":0,"limit":100,"unit":"percent"},"status":"ok"}],
         "metadata":{"email":"izanorshahril.ibrahim@gmail.com"}}
      ]}"#;

    /// The answer `33 google quota` said did not exist.
    #[test]
    fn a_google_window_is_readable_after_all_and_keyed_so_three_stay_three() {
        let windows = parse(FIXTURE);
        let google: Vec<_> = windows
            .iter()
            .filter(|w| w.account == "izanorshahril.ibrahim@gmail.com")
            .collect();
        assert_eq!(google.len(), 2, "{windows:#?}");

        // Both report `window.id: "daily"`. Keying on that would collapse them
        // into one window flapping - `30 codex app server`'s exact finding.
        let keys: Vec<_> = google.iter().map(|w| w.window_key()).collect();
        assert_ne!(keys[0], keys[1], "{keys:?}");
    }

    /// Milliseconds in, seconds out, and the provider's own percentage
    /// untouched. Both are `10 runner inventory`'s rule in opposite directions.
    #[test]
    fn a_reset_is_converted_to_seconds_and_a_percentage_never_is() {
        let windows = parse(FIXTURE);
        let five_hour = windows
            .iter()
            .find(|w| w.rate_limit_type == "openai-codex:primary")
            .expect("codex reports two windows on one login");

        assert_eq!(
            five_hour.availability,
            Availability::Allowed {
                resets_at: Some(1787933835)
            }
        );
        assert_eq!(five_hour.used_percent, Some(12));
        assert_eq!(five_hour.window_duration_mins, Some(300));
        assert_eq!(five_hour.runner, "omp");
    }

    /// `27 quota accounting`'s existing call: anything that is not `ok` is
    /// exhausted, rather than a fourth state nobody has a use for.
    #[test]
    fn a_window_that_is_not_ok_is_exhausted_until_its_reset() {
        let seven_day = parse(FIXTURE)
            .into_iter()
            .find(|w| w.rate_limit_type == "openai-codex:secondary")
            .expect("the weekly window");
        assert_eq!(
            seven_day.availability,
            Availability::ExhaustedUntil {
                resets_at: 1788456114
            }
        );
    }

    /// A tool farseer does not own is free to change its mind. Improving a view
    /// is not worth failing a request over.
    #[test]
    fn a_document_farseer_does_not_recognise_yields_nothing_rather_than_an_error() {
        assert!(parse("not json").is_empty());
        assert!(parse(r#"{"reports":"surprise"}"#).is_empty());
        assert!(parse(r#"{"reports":[{"limits":[{"label":"no id"}]}]}"#).is_empty());
    }
}
