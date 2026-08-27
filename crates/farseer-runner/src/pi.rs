//! The `pi` runner: pi's headless RPC mode, per `29 harness protocol`.
//!
//! Every mapping below is backed by a literal line from the live probe of
//! `pi 0.84.3` on 2026-08-27, per this crate's rule that no progress mapping is
//! guessed at past its verified shape.
//!
//! # A fifth protocol, and why it is not a fourth ACP
//!
//! `pi --mode rpc` speaks **line-delimited JSON commands**, not JSON-RPC: a
//! command is `{"type":"prompt","message":...}` with an optional `id`, and a
//! reply is `{"type":"response","command":"prompt","success":true}`. There is no
//! `method`, no `params`, and no handshake - the first line farseer writes is
//! the goal. So [`crate::jsonrpc`] does not apply here, and `08 generalization
//! test`'s extract-on-second-use rule says to leave it that way: this is the
//! first protocol of its shape, not the second.
//!
//! # What pi reports that nothing else does
//!
//! - **Money.** `10 runner inventory` found Claude Code the only runner
//!   reporting cost, and `30 codex app server` confirmed Codex reports tokens
//!   with no currency at all. pi prices every message itself, per-model, and
//!   carries `cost.total` in dollars on the assistant message. It is the second
//!   runner able to answer "what did that cost".
//! - **Native steering with a mode.** `{"type":"steer"}` is a first-class
//!   command beside `follow_up`, and pi distinguishes the two - a steer lands in
//!   the turn already running, a follow-up queues behind it. `20 worker control
//!   channel` had to infer that distinction from Claude Code's behaviour; pi
//!   states it.
//! - **A compaction boundary**, as `compaction_start`/`compaction_end`. The
//!   third runner able to say so, after Claude Code and the Codex app-server.
//!
//! # What it does not report
//!
//! No quota. pi talks to whichever provider the operator configured, and a
//! subscription window is the provider's fact rather than pi's - so
//! `27 quota accounting` gets nothing here, and `control_of` says so rather than
//! showing an empty column that looks like an absence of spending.
//!
//! The context **denominator** is knowable but not streamed: `get_state` returns
//! the model record with its `contextWindow`, while the event stream carries
//! only the numerator. Reporting `used` with no `size` would be the half-answer
//! `29 harness protocol` argued about, so this adapter reports neither and the
//! menu says why.
//!
//! # Why the deltas are activity and the message is the answer
//!
//! `message_update` streams `text_delta` fragments and `message_end` carries the
//! assembled text. The same shape ACP and the Codex app-server have, and the
//! same rule from `05 run state model`: **token streams are activity, not
//! progress**. See [`RunnerSignal::OutputChunk`].

use serde_json::{Value, json};

use farseer_core::{EventKind, Outcome};

use crate::claude_code::{Configured, FinishedSignal, ParseError, RunnerSignal, SessionInfo};

/// The command that starts a turn. Written straight after spawn - pi needs no
/// handshake, which is why this runner has no `handshake` free function beside
/// the ones [`crate::acp_drive`] and [`crate::codex_app_server`] needed.
pub fn prompt_frame(message: &str) -> String {
    json!({ "type": "prompt", "message": message }).to_string()
}

/// Put words into a turn that is already running.
///
/// `steer` rather than `follow_up` on purpose: `20 worker control channel` is
/// about reaching a run **in flight**, and pi queues a follow-up until the
/// current turn ends, which is a different verb wearing a similar name.
pub fn steer_frame(message: &str) -> String {
    json!({ "type": "steer", "message": message }).to_string()
}

/// Ask pi what it is configured as, before the goal goes in.
///
/// The answer names the model, the thinking level and pi's own session id, which
/// is `28 operator surface`'s "find this conversation in the runner's own
/// tooling" - and it is a **claim pi makes about itself**, so it satisfies
/// `10 runner inventory`'s observed-never-advertised rule where reading
/// farseer's own launch flags back would not.
pub fn get_state_frame() -> String {
    json!({ "type": "get_state" }).to_string()
}

/// Stop the turn. pi keeps the session, so this is `05 run state model`'s
/// cancel-the-work, not kill-the-process - the Job Object still owns the latter.
pub fn abort_frame() -> String {
    json!({ "type": "abort" }).to_string()
}

/// The launch argv, given what the operator pinned in `runners.toml`.
///
/// `--mode rpc` is the whole runner; everything after it is the operator's
/// declaration. Absent means absent: a model farseer does not pass is a model pi
/// chooses from its own config, which is the same deference `30 codex app
/// server` settled for effort.
pub fn build_args(model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let mut args = vec!["--mode".to_string(), "rpc".to_string()];
    if let Some(model) = model {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    if let Some(effort) = effort {
        args.push("--thinking".to_string());
        args.push(effort.to_string());
    }
    args
}

/// What pi said about itself, from a `get_state` reply.
fn session_from_state(data: &Value) -> SessionInfo {
    let model_id = data
        .get("model")
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    SessionInfo {
        // What it will use, which is also what it is configured to use - pi is
        // asked before the turn rather than reporting after it.
        model: model_id.clone(),
        session_id: data
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string),
        // pi names the upstream it will call, which is the field `28 operator
        // surface` wanted and only an ACP agent had answered until now.
        provider: data
            .get("model")
            .and_then(|m| m.get("provider"))
            .and_then(Value::as_str)
            .map(str::to_string),
        configured: Some(Configured {
            model: model_id,
            effort: data
                .get("thinkingLevel")
                .and_then(Value::as_str)
                .map(str::to_string),
            // pi reports the level, not which file set it. Absent rather than
            // filled with a guess, per `13 harness build kit`.
            from: None,
        }),
    }
}

pub fn parse_line(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
    let v: Value = serde_json::from_str(line).map_err(|e| ParseError(e.to_string()))?;

    let signal = match v.get("type").and_then(Value::as_str).unwrap_or_default() {
        // A reply to something farseer asked. Only `get_state` says anything
        // farseer records; the rest are acknowledgements.
        "response" => (v.get("command").and_then(Value::as_str) == Some("get_state"))
            .then(|| v.get("data"))
            .flatten()
            .map(|data| RunnerSignal::Session(session_from_state(data))),
        // A fragment of an answer still being written.
        "message_update" => v
            .get("assistantMessageEvent")
            .filter(|e| e.get("type").and_then(Value::as_str) == Some("text_delta"))
            .and_then(|e| e.get("delta"))
            .and_then(Value::as_str)
            .filter(|delta| !delta.is_empty())
            .map(|delta| RunnerSignal::OutputChunk(delta.to_string())),
        // `02 record scope`: farseer records **that** a compaction happened and
        // **when**, never what was dropped.
        "compaction_end" => Some(RunnerSignal::Progress {
            kind: EventKind::new(EventKind::CONTEXT_COMPACTED),
            payload: json!({}),
        }),
        // The agent loop settled, which is one or many turns later than the
        // first `turn_end`: a turn is one round-trip, and a tool call starts
        // another. Ending a worker at `turn_end` would cut the loop mid-tool.
        "agent_end" => Some(RunnerSignal::Finished(finished(&v))),
        _ => None,
    };
    Ok(signal.into_iter().collect())
}

/// The outcome and the totals, summed across the assistant messages of this
/// agent loop.
///
/// Summed rather than read off the last message because a tool-using loop
/// produces several, and `11 analytics questions` asks what the **run** cost.
fn finished(v: &Value) -> FinishedSignal {
    let empty = Vec::new();
    let messages = v
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let assistants = || {
        messages
            .iter()
            .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
    };
    let any = assistants().next().is_some();

    let summed = |path: &[&str]| -> f64 {
        assistants()
            .filter_map(|m| {
                path.iter()
                    .try_fold(m, |node, key| node.get(key))
                    .and_then(Value::as_f64)
            })
            .sum()
    };

    FinishedSignal {
        // `05 run state model`: only a human choosing not to proceed is
        // `Cancelled`, and pi's `aborted` is exactly farseer's own `abort`.
        outcome: match assistants()
            .next_back()
            .and_then(|m| m.get("stopReason"))
            .and_then(Value::as_str)
        {
            Some("stop" | "toolUse") => Outcome::Ok,
            Some("aborted") => Outcome::Cancelled,
            // No assistant message at all means the loop produced nothing, which
            // is a failure however quietly it ended.
            _ => Outcome::Failed,
        },
        // Dollars as the store's integer micros, so no float reaches the record.
        cost_usd_micros: any
            .then(|| (summed(&["usage", "cost", "total"]) * 1_000_000.0).round() as i64)
            .filter(|micros| *micros > 0),
        tokens: any.then(|| summed(&["usage", "totalTokens"]) as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixture is a literal line from the 2026-08-27 probe of `pi` 0.84.3
    /// against `openai-codex/gpt-5.6-luna`, trimmed only of ids and timestamps.
    #[test]
    fn a_text_delta_is_activity_rather_than_an_answer() {
        let line = r#"{"type":"message_update","usage":{"input":0,"output":0},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":" online"}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::OutputChunk(" online".to_string())]
        );
    }

    #[test]
    fn the_settled_loop_carries_the_outcome_the_tokens_and_the_money() {
        let line = r#"{"type":"agent_end","messages":[{"role":"user","content":[{"type":"text","text":"hi"}]},{"role":"assistant","content":[{"type":"text","text":"pi online."}],"model":"gpt-5.6-luna","usage":{"input":7833,"output":7,"cacheRead":0,"cacheWrite":0,"reasoning":0,"totalTokens":7840,"cost":{"input":0.0015666,"output":0.0000084,"cacheRead":0,"cacheWrite":0,"total":0.001575}},"stopReason":"stop"}],"willRetry":false}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Ok,
                // The second runner in the inventory able to answer this at all.
                cost_usd_micros: Some(1575),
                tokens: Some(7840),
            })]
        );
    }

    /// A tool-using loop settles once and bills several times, which is why
    /// `finished` sums rather than reading the last message.
    #[test]
    fn a_multi_turn_loop_bills_every_round_rather_than_only_the_last() {
        let line = r#"{"type":"agent_end","messages":[
            {"role":"assistant","usage":{"totalTokens":100,"cost":{"total":0.001}},"stopReason":"toolUse"},
            {"role":"user","content":[]},
            {"role":"assistant","usage":{"totalTokens":250,"cost":{"total":0.002}},"stopReason":"stop"}
        ],"willRetry":false}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Ok,
                cost_usd_micros: Some(3000),
                tokens: Some(350),
            })]
        );
    }

    #[test]
    fn an_abort_is_cancelled_because_a_person_chose_to_stop() {
        let line = r#"{"type":"agent_end","messages":[{"role":"assistant","usage":{"totalTokens":12,"cost":{"total":0}},"stopReason":"aborted"}],"willRetry":false}"#;
        let Ok(signals) = parse_line(line) else {
            panic!("parses")
        };
        assert_eq!(
            signals,
            vec![RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Cancelled,
                // Zero is not a cost pi reported, it is a cost pi computed to
                // nothing - `10 runner inventory`'s rule says report neither.
                cost_usd_micros: None,
                tokens: Some(12),
            })]
        );
    }

    #[test]
    fn get_state_is_what_pi_says_about_itself_rather_than_what_farseer_launched() {
        let line = r#"{"id":"a","type":"response","command":"get_state","success":true,"data":{"model":{"id":"gpt-5.6-luna","name":"GPT-5.6 Luna","provider":"openai-codex","contextWindow":272000},"thinkingLevel":"low","isStreaming":false,"sessionId":"01a0-4b","messageCount":0}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Session(SessionInfo {
                model: Some("gpt-5.6-luna".to_string()),
                session_id: Some("01a0-4b".to_string()),
                provider: Some("openai-codex".to_string()),
                configured: Some(Configured {
                    model: Some("gpt-5.6-luna".to_string()),
                    effort: Some("low".to_string()),
                    from: None,
                }),
            })]
        );
    }

    /// The acknowledgements pi sends for every command carry nothing the record
    /// wants, and inventing an event for them would put farseer's own writes
    /// into the record as if the runner had said something.
    #[test]
    fn an_acknowledgement_is_not_an_event() {
        let line = r#"{"id":"b","type":"response","command":"prompt","success":true}"#;
        assert_eq!(parse_line(line).unwrap(), vec![]);
    }

    #[test]
    fn the_operator_pins_the_model_and_an_unpinned_one_stays_pis_own() {
        assert_eq!(build_args(None, None), ["--mode", "rpc"]);
        assert_eq!(
            build_args(Some("gpt-5.6-luna"), Some("low")),
            ["--mode", "rpc", "--model", "gpt-5.6-luna", "--thinking", "low"]
        );
    }
}
