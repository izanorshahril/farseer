//! Maps Claude Code's stream-json line stream onto the contract `05 run state model` wrote and `20 worker control channel`/`10 runner inventory` scored this runner against.
//! [`crate::invocation::build_args`] selects the production invocation for the process role.
//! A manager uses `--input-format stream-json` and receives its initial goal plus later steer messages through live stdin.
//! A worker omits `--input-format` and receives one positional goal so its invocation ends after one turn.
//! Neither production invocation uses `--include-partial-messages`.
//! A `content_block_start` or `stream_event` line is therefore seen only when another caller opts into partial messages.
//!
//! **Every successfully parsed line is activity**, full stop - that is what
//! solves `05 run state model`'s twenty-minute-reasoning problem, and it does not depend on
//! recognising the line's shape. [`RunnerSignal`] carries only what is
//! additionally **progress** (`05 run state model`'s three kinds) or terminal, because those
//! are the only things the record keeps. A line this module does not
//! recognise still counts as activity and yields no signal - the schema will
//! grow kinds this crate does not know yet, and an unmapped line is not a
//! hang.
//!
//! Field names below are `camelCase` where Claude Code's own JSON uses it
//! (`resetsAt`, `rateLimitType`) - transcribed from the payload `10 runner inventory` captured
//! on this machine, not renamed to Rust convention, so a `grep` on the wire
//! format finds this file.
//!
//! **Steer is no longer blocked on an unverified frame.** Verified
//! 2026-08-23 against the real, installed `claude` 2.1.233: piping
//! `{"type":"user","message":{"role":"user","content":[{"type":"text","text":"..."}]}}\n`
//! lines to a process started with `--input-format stream-json` is accepted
//! both as the initial message and as a genuine follow-up turn - a second
//! message sent before closing stdin correctly recalled a fact stated in the
//! first, under the same `session_id`, and each turn produced its **own**
//! terminal `result` event rather than one at the very end. `invocation.rs`'s
//! doc comment previously called this frame unobserved; it is now
//! observed and cited here. Production wires the live process's stdin to later
//! HTTP steer requests through the manager's `SteerHandle`; this probe covered
//! Claude Code's protocol, not the API path around it.

use farseer_core::event::EventKind;
use farseer_core::run::Outcome;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum RunnerSignal {
    /// One of `EventKind::is_progress`'s kinds, ready to become a `NewEvent`
    /// payload. The caller supplies `event_id`/`ts`/`cell_id`/`run_id`/`actor`.
    Progress {
        kind: EventKind,
        payload: Value,
    },
    RateLimit(RateLimitInfo),
    /// User-visible terminal text which a supervising manager can relay.
    Output(String),
    Finished(FinishedSignal),
}

/// `10 runner inventory`: "farseer never has to hit a limit to know where it stands" - this is
/// that event, transcribed field for field from the captured payload.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitInfo {
    pub status: String,
    pub resets_at: i64,
    pub rate_limit_type: String,
    pub is_using_overage: bool,
}

/// `11 analytics questions`'s cost metric, native to this runner per `10 runner inventory`: `total_cost_usd` in the
/// terminal `result` event, converted to the store's integer micros so no
/// float ever reaches the record.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedSignal {
    pub outcome: Outcome,
    pub cost_usd_micros: Option<i64>,
    pub tokens: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid stream-json line: {0}")]
pub struct ParseError(pub String);

/// The frame this doc comment's 2026-08-23 probe verified: one
/// stream-json line a process started with `--input-format stream-json`
/// accepts as a user turn, whether it is the first message or a later steer.
/// `serde_json::json!` rather than hand-built string interpolation so `text`
/// containing a quote or newline serializes correctly instead of breaking
/// the frame.
pub fn steer_frame(text: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{ "type": "text", "text": text }]
        }
    })
    .to_string()
}

/// Parse one line. `Ok` always means "count this as activity"; the returned
/// `Vec` is what, if anything, additionally belongs in the record.
pub fn parse_line(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
    let v: Value = serde_json::from_str(line).map_err(|e| ParseError(e.to_string()))?;
    let Some(kind) = v.get("type").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };

    if kind == "result" {
        let mut signals = Vec::new();
        if let Some(result) = v.get("result").and_then(Value::as_str) {
            signals.push(RunnerSignal::Output(result.to_string()));
        }
        signals.push(RunnerSignal::Finished(finished(&v)));
        return Ok(signals);
    }

    let signal = match kind {
        "rate_limit_event" => v
            .get("rate_limit_info")
            .and_then(rate_limit)
            .map(RunnerSignal::RateLimit),
        "system" if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") => {
            Some(RunnerSignal::Progress {
                kind: EventKind::new(EventKind::CONTEXT_COMPACTED),
                payload: serde_json::json!({ "trigger": v.get("trigger") }),
            })
        }
        "stream_event" => v.get("event").and_then(stream_event_progress),
        "user" => v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
            .and_then(|blocks| {
                blocks
                    .iter()
                    .find(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            })
            .map(|block| RunnerSignal::Progress {
                kind: EventKind::new(EventKind::TOOL_RESULT),
                payload: block.clone(),
            }),
        _ => None,
    };

    Ok(signal.into_iter().collect())
}

fn rate_limit(v: &Value) -> Option<RateLimitInfo> {
    Some(RateLimitInfo {
        status: v.get("status")?.as_str()?.to_string(),
        resets_at: v.get("resetsAt")?.as_i64()?,
        rate_limit_type: v.get("rateLimitType")?.as_str()?.to_string(),
        is_using_overage: v
            .get("isUsingOverage")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// **`subtype` is not the success signal.** Verified 2026-08-23 against the
/// real, installed `claude` 2.1.233: a headless run refused for
/// `authentication_failed` still emitted `"subtype":"success"` on its
/// terminal `result` event, alongside a top-level `"is_error":true`. The
/// same probe's successful run had `is_error:false` with the identical
/// `subtype`. `is_error` is therefore the authoritative field; `subtype` is
/// kept as a fallback only for the case it is absent, which no captured
/// payload has shown but nothing rules out either.
fn finished(v: &Value) -> FinishedSignal {
    let outcome = match v.get("is_error").and_then(Value::as_bool) {
        Some(true) => Outcome::Failed,
        Some(false) => Outcome::Ok,
        None => match v.get("subtype").and_then(Value::as_str) {
            Some("success") => Outcome::Ok,
            _ => Outcome::Failed,
        },
    };
    let cost_usd_micros = v
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .map(|dollars| (dollars * 1_000_000.0).round() as i64);
    let tokens = v.get("usage").and_then(|u| {
        let input = u.get("input_tokens").and_then(Value::as_i64)?;
        let output = u.get("output_tokens").and_then(Value::as_i64)?;
        Some(input + output)
    });
    FinishedSignal {
        outcome,
        cost_usd_micros,
        tokens,
    }
}

/// The wrapped Anthropic Messages API stream event `--include-partial-messages`
/// forwards. Only the two shapes `05 run state model` needs as progress are mapped; a bare
/// `text_delta` is activity-only and returns `None` on purpose.
fn stream_event_progress(event: &Value) -> Option<RunnerSignal> {
    match event.get("type").and_then(Value::as_str) {
        Some("content_block_start")
            if event
                .get("content_block")
                .and_then(|b| b.get("type"))
                .and_then(Value::as_str)
                == Some("tool_use") =>
        {
            Some(RunnerSignal::Progress {
                kind: EventKind::new(EventKind::TOOL_CALL_STARTED),
                payload: event.get("content_block").cloned().unwrap_or(Value::Null),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steer_frame_matches_the_shape_the_2026_08_23_probe_verified() {
        let line = steer_frame("keep going");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"][0]["type"], "text");
        assert_eq!(v["message"]["content"][0]["text"], "keep going");
    }

    #[test]
    fn steer_frame_escapes_a_quote_in_the_message_rather_than_breaking_the_line() {
        let line = steer_frame(r#"say "hi" back"#);
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["message"]["content"][0]["text"], r#"say "hi" back"#);
    }

    #[test]
    fn a_rate_limit_event_carries_the_reset_epoch_and_the_overage_state() {
        // `10 runner inventory`'s captured payload, transcribed verbatim.
        let line = r#"{"type":"rate_limit_event",
 "rate_limit_info":{"status":"allowed",
                    "resetsAt":1787473800,
                    "rateLimitType":"five_hour",
                    "overageStatus":"rejected",
                    "overageDisabledReason":"org_level_disabled",
                    "isUsingOverage":false}}"#;

        let signals = parse_line(line).unwrap();
        assert_eq!(
            signals,
            [RunnerSignal::RateLimit(RateLimitInfo {
                status: "allowed".into(),
                resets_at: 1_787_473_800,
                rate_limit_type: "five_hour".into(),
                is_using_overage: false,
            })]
        );
    }

    #[test]
    fn a_successful_result_reports_ok_and_the_cost_in_micros() {
        let line = r#"{"type":"result","subtype":"success","result":"ok","total_cost_usd":0.32266,
                        "usage":{"input_tokens":100,"output_tokens":50}}"#;

        let signals = parse_line(line).unwrap();
        assert_eq!(
            signals,
            [
                RunnerSignal::Output("ok".into()),
                RunnerSignal::Finished(FinishedSignal {
                    outcome: Outcome::Ok,
                    cost_usd_micros: Some(322_660),
                    tokens: Some(150),
                })
            ]
        );
    }

    #[test]
    fn a_failed_result_is_failed_not_cancelled() {
        // `05 run state model`: only a human choosing not to proceed is `Cancelled`. A harness
        // reporting an error is `Failed`, which invites a retry.
        let line = r#"{"type":"result","subtype":"error_during_execution","total_cost_usd":0.01}"#;
        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Finished(f)] = signals.as_slice() else {
            panic!("expected one Finished signal, got {signals:?}");
        };
        assert_eq!(f.outcome, Outcome::Failed);
    }

    #[test]
    fn is_error_overrides_a_misleading_success_subtype() {
        // The exact shape a real `claude` 2.1.233 run emitted for
        // `authentication_failed`: `subtype` reads "success" - it names the
        // output format, not the outcome - while `is_error` correctly says
        // this failed. Trusting `subtype` alone would record this as `Ok`.
        let line = r#"{"type":"result","subtype":"success","is_error":true,"total_cost_usd":0}"#;
        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Finished(f)] = signals.as_slice() else {
            panic!("expected one Finished signal, got {signals:?}");
        };
        assert_eq!(f.outcome, Outcome::Failed);
    }

    #[test]
    fn is_error_false_with_a_success_subtype_is_ok() {
        let line =
            r#"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.01}"#;
        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Finished(f)] = signals.as_slice() else {
            panic!("expected one Finished signal, got {signals:?}");
        };
        assert_eq!(f.outcome, Outcome::Ok);
    }

    #[test]
    fn a_tool_use_content_block_start_is_a_tool_call_started_progress_event() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_start",
                        "index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"Read"}}}"#;

        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Progress { kind, .. }] = signals.as_slice() else {
            panic!("expected one Progress signal, got {signals:?}");
        };
        assert_eq!(kind.as_str(), EventKind::TOOL_CALL_STARTED);
    }

    #[test]
    fn a_text_delta_is_activity_only_and_yields_no_signal() {
        // The twenty-minute-reasoning case `05 run state model` built the activity/progress
        // split for: bytes arrive, nothing goes in the record.
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta",
                        "index":0,"delta":{"type":"text_delta","text":"thinking..."}}}"#;

        assert_eq!(parse_line(line).unwrap(), Vec::new());
    }

    #[test]
    fn a_tool_result_in_a_user_message_is_a_tool_result_progress_event() {
        let line = r#"{"type":"user","message":{"role":"user","content":[
                        {"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]}}"#;

        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Progress { kind, .. }] = signals.as_slice() else {
            panic!("expected one Progress signal, got {signals:?}");
        };
        assert_eq!(kind.as_str(), EventKind::TOOL_RESULT);
    }

    #[test]
    fn a_compact_boundary_system_event_is_a_context_compacted_progress_event() {
        let line = r#"{"type":"system","subtype":"compact_boundary","trigger":"auto"}"#;
        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Progress { kind, payload }] = signals.as_slice() else {
            panic!("expected one Progress signal, got {signals:?}");
        };
        assert_eq!(kind.as_str(), EventKind::CONTEXT_COMPACTED);
        assert_eq!(payload["trigger"], "auto");
    }

    #[test]
    fn an_unrecognised_type_is_still_activity_and_yields_no_signal() {
        // The schema will grow kinds this module does not know about yet.
        // That must count as the model doing something, never as a hang.
        let line = r#"{"type":"system","subtype":"init","session_id":"abc"}"#;
        assert_eq!(parse_line(line).unwrap(), Vec::new());
    }

    #[test]
    fn malformed_json_is_an_error_not_silence() {
        assert!(parse_line("not json").is_err());
    }
}
