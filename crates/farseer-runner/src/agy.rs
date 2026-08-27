//! The `agy` runner: Antigravity's CLI, per `29 harness protocol`.
//!
//! Every mapping below is backed by a literal line from the live probe of
//! `agy 1.1.13` on 2026-08-27, per this crate's rule that no progress mapping is
//! guessed at past its verified shape.
//!
//! # A sixth event vocabulary, and the first Google one
//!
//! `agy -p --output-format stream-json` emits `{"event": ...}` objects with the
//! payload under a key named after the event - `{"event":"result","result":{..}}`
//! rather than a flat object or a JSON-RPC envelope. That is a third framing
//! convention in this crate, after Claude Code's flat `type` and the app-server's
//! `method`/`params`, and it is why the runner list is a **menu rather than a
//! survey**: nothing about having driven five runners predicted this one.
//!
//! # What it reports
//!
//! - **The model, on `init`**, beside the whole tool list and a conversation id
//!   the operator can find in agy's own storage. `10 runner inventory`'s
//!   observed-never-advertised rule is satisfied at the first line rather than
//!   the last.
//! - **Tokens, twice**: on the terminal `step_update` and again on `result`.
//!   Read once, from `result`, so one number does not arrive by two routes.
//!   `thinking_tokens` and `cache_read_tokens` are broken out, which no other
//!   runner does; `11 analytics questions` wants the total, so the total is
//!   what travels and the breakdown stays in the runner.
//!
//! # What it does not report
//!
//! No cost, no quota window, no context denominator, and no compaction
//! boundary. It is also **one-shot**: `-p` runs a prompt and exits, and there is
//! no stdin protocol to steer through - `--continue` starts a new process the
//! way Codex and cursor-agent do, which `10 runner inventory` already measured
//! as not-steering twice.
//!
//! Effort is not a flag farseer sends here even when the operator pins one. agy
//! bakes it into the **model id** - `gemini-3.7-flash-low` and
//! `gemini-3.7-flash-high` are two entries in `agy models` rather than one model
//! at two settings - so a separate `--effort` would be a second place to say the
//! same thing, and the two could disagree.

use serde_json::Value;

use farseer_core::Outcome;

use crate::claude_code::{FinishedSignal, ParseError, RunnerSignal, SessionInfo};

/// The launch argv for one goal.
///
/// `--output-format stream-json` is what makes this a runner rather than a
/// screen-scrape. The model is the operator's pin from `runners.toml`, carried
/// verbatim: agy's own `models` listing is the vocabulary, and farseer does not
/// translate it.
pub fn build_args(goal: &str, model: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        goal.to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
    ];
    if let Some(model) = model {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    args
}

pub fn parse_line(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
    let v: Value = serde_json::from_str(line).map_err(|e| ParseError(e.to_string()))?;
    let event = v.get("event").and_then(Value::as_str).unwrap_or_default();
    // The payload lives under a key named after the event, which is agy's own
    // framing rather than anything the other five runners do.
    let body = v.get(event).unwrap_or(&Value::Null);

    let signal = match event {
        "init" => Some(RunnerSignal::Session(SessionInfo {
            model: body
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            session_id: v
                .get("conversation_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            // agy names no provider and no configured effort. The effort is
            // inside the model id, and splitting it out here would be farseer
            // parsing a vendor's naming scheme into a claim the runner never
            // made - exactly what `10 runner inventory`'s rule forbids.
            provider: None,
            configured: None,
        })),
        // Answer text arrives a delta at a time on the step that is producing
        // it. `05 run state model`: **token streams are activity, not
        // progress**, so these accumulate into one answer rather than becoming
        // one event each.
        "step_update" => body
            .get("text_delta")
            .and_then(Value::as_str)
            .filter(|delta| !delta.is_empty())
            .filter(|_| body.get("step_type").and_then(Value::as_str) == Some("agent_response"))
            .map(|delta| RunnerSignal::OutputChunk(delta.to_string())),
        "result" => Some(RunnerSignal::Finished(FinishedSignal {
            outcome: match body.get("status").and_then(Value::as_str) {
                Some("SUCCESS") => Outcome::Ok,
                // agy has no cancel farseer can ask for - the Job Object kill is
                // the whole story - so nothing here maps to `Cancelled`, and
                // `05 run state model` reserves it for a human choosing to stop.
                _ => Outcome::Failed,
            },
            // Reported in tokens only. `10 runner inventory` measured most
            // runners this way and this one does not change it.
            cost_usd_micros: None,
            tokens: body
                .get("usage")
                .and_then(|u| u.get("total_tokens"))
                .and_then(Value::as_i64),
        })),
        _ => None,
    };
    Ok(signal.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixture is a literal line from the 2026-08-27 probe of `agy` 1.1.13
    /// on `gemini-3.7-flash-low`, trimmed only of the tool list and timings.
    #[test]
    fn init_names_the_model_and_the_conversation_at_the_first_line() {
        let line = r#"{"event":"init","conversation_id":"8a55adea-9ef1","init":{"model":"gemini-3.7-flash-low","cwd":"D:\\Dev\\farseer","tools":["ask_question"]}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Session(SessionInfo {
                model: Some("gemini-3.7-flash-low".to_string()),
                session_id: Some("8a55adea-9ef1".to_string()),
                provider: None,
                // The effort is inside the model id. Splitting `-low` off it
                // would be farseer reading a vendor's naming scheme as a claim.
                configured: None,
            })]
        );
    }

    #[test]
    fn an_answer_delta_is_activity_rather_than_an_answer() {
        let line = r#"{"event":"step_update","step_update":{"step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"agy online."}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::OutputChunk("agy online.".to_string())]
        );
    }

    /// The operator's own prompt comes back as a step too. Recording it as
    /// answer text would put farseer's input into the record as agy's output.
    #[test]
    fn a_step_that_is_not_the_agent_speaking_is_not_answer_text() {
        let line = r#"{"event":"step_update","step_update":{"step_index":0,"state":"DONE","step_type":"user_input","text_delta":"Reply with exactly: agy online."}}"#;
        assert_eq!(parse_line(line).unwrap(), vec![]);
    }

    #[test]
    fn the_result_carries_the_outcome_and_the_tokens_but_no_money() {
        let line = r#"{"event":"result","result":{"conversation_id":"8a55adea","status":"SUCCESS","response":"agy online.\n","duration_seconds":2.43,"num_turns":1,"usage":{"input_tokens":13804,"output_tokens":25,"thinking_tokens":22,"cache_read_tokens":0,"total_tokens":13829}}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Ok,
                cost_usd_micros: None,
                tokens: Some(13829),
            })]
        );
    }

    /// `05 run state model` reserves `Cancelled` for a human choosing to stop,
    /// and agy offers farseer no way to ask - so anything that is not success is
    /// a failure rather than a guess at why.
    #[test]
    fn anything_that_is_not_success_is_failed_rather_than_cancelled() {
        let line = r#"{"event":"result","result":{"status":"ERROR","usage":{"total_tokens":40}}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Failed,
                cost_usd_micros: None,
                tokens: Some(40),
            })]
        );
    }

    #[test]
    fn the_operator_pins_the_model_and_an_unpinned_one_stays_agys_own() {
        assert_eq!(
            build_args("say hi", None),
            ["-p", "say hi", "--output-format", "stream-json"]
        );
        assert_eq!(
            build_args("say hi", Some("gemini-3.7-flash-low")),
            [
                "-p",
                "say hi",
                "--output-format",
                "stream-json",
                "--model",
                "gemini-3.7-flash-low"
            ]
        );
    }
}
