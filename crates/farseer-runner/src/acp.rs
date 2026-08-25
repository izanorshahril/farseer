//! The ACP runner: one adapter for every harness that speaks the Agent Client Protocol.
//!
//! `20 worker control channel` chose an ACP runner as the default path and it was never built, so
//! farseer grew four native adapters instead. `29 harness protocol` asked what
//! everyone else does and found the answer is this: no host of these binaries
//! parses per-harness dialects, because ACP is JSON-RPC 2.0 over stdio and one
//! parser admits Gemini CLI, opencode, Amp, Droid, Copilot, Qwen, pi, OpenClaw
//! and Aider at once.
//!
//! Captured from `goose acp` 1.47.0 on 2026-08-26, and every mapping below is
//! backed by a literal line from that transcript, per this crate's rule that no
//! progress mapping is guessed at past its verified shape.
//!
//! # What ACP gives farseer that no native adapter did
//!
//! **`size`.** The `usage_update` notification carries `used` *and* `size` -
//! the context window, which `28 operator surface` wanted and which neither
//! Claude Code nor Codex reports at all. `29` argued for it from the spec; the
//! capture shows `{"used":4560,"size":1050000}` arriving unprompted.
//!
//! # What ACP costs farseer, which is why this is a fifth runner and not a replacement
//!
//! There is no `rate_limit_event` and no compaction boundary in ACP, and those
//! are `27 quota accounting`'s foundation and a column `10 runner inventory`
//! scored. A harness driven over ACP is quota-blind. `29` decided the rule:
//! **use the richest face a harness offers**, which for Claude Code and Codex
//! is still their native one.
//!
//! # The permission hazard
//!
//! ACP expects the client to answer `session/request_permission`, and farseer
//! has no human at the prompt. An unanswered request is exactly the hang that
//! `28`'s missing `--allowedTools` entry produced - a live process, no output,
//! indistinguishable from thinking. This module therefore **surfaces the
//! request as a signal rather than swallowing it**, and a session must be put
//! in a mode that does not ask. `goose acp` opens in `auto`; that is luck, not
//! a guarantee, and the driver must set the mode explicitly.

use crate::claude_code::{FinishedSignal, ParseError, RunnerSignal};
use farseer_core::event::EventKind;
use farseer_core::run::Outcome;
use serde_json::{Value, json};

/// The protocol version farseer negotiates. Bumping it is a decision, not an
/// upgrade - a version farseer has not captured output from is a version whose
/// mappings are guesses.
pub const PROTOCOL_VERSION: i64 = 1;

/// `initialize`, declining the two capabilities farseer must not accept.
///
/// ACP lets the **client** serve `fs/read_text_file`, `fs/write_text_file` and
/// `terminal/*` to the agent. Accepting them would move file and shell access
/// out of the runner's git worktree and into farseer's own process, which is
/// `19 rust toolchain`'s workspace isolation dissolved. Capabilities are
/// negotiated here precisely so a client can refuse, so farseer refuses and the
/// agent falls back to its own filesystem access inside the workspace it was
/// given. Verified: `goose acp` accepted the refusal and ran the turn.
pub fn initialize_frame(id: i64) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": {
                "fs": { "readTextFile": false, "writeTextFile": false },
                "terminal": false
            }
        }
    })
    .to_string()
}

/// `session/new` in a workspace. `mcpServers` is empty because farseer's own
/// MCP face is reached by the manager's runner-native configuration rather than
/// forwarded per session - `16 local api surface` owns that address.
pub fn session_new_frame(id: i64, cwd: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/new",
        "params": { "cwd": cwd, "mcpServers": [] }
    })
    .to_string()
}

/// `session/set_mode`, which is how farseer refuses to be asked.
///
/// `12 autonomy and deny list` decides a cell's ceiling before the run starts,
/// and ACP's modes are the same idea one level down: goose exposes `auto`,
/// `approve`, `smart_approve` and `chat`. A farseer run must not open in a mode
/// that prompts, because nobody is watching the prompt.
pub fn set_mode_frame(id: i64, session_id: &str, mode_id: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/set_mode",
        "params": { "sessionId": session_id, "modeId": mode_id }
    })
    .to_string()
}

/// `session/prompt` - the goal, and later any steer.
///
/// ACP has no mid-turn steer: `20 worker control channel` made steering
/// turn-boundary granular and ACP agrees by omission, so a steer is simply the
/// next `session/prompt` on the same session.
pub fn prompt_frame(id: i64, session_id: &str, text: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": text }]
        }
    })
    .to_string()
}

/// `session/cancel`, a notification: `05 run state model`'s `cancel` verb, and ACP expects no
/// reply to it.
pub fn cancel_frame(session_id: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": { "sessionId": session_id }
    })
    .to_string()
}

/// What a `session/new` response carried, for the driver that sent it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionOpened {
    pub session_id: String,
    /// The mode the agent chose for itself. Farseer does not trust it - see
    /// [`set_mode_frame`] - but recording it says whether the default was safe.
    pub current_mode: Option<String>,
    /// Modes the agent will accept, so a driver can pick one that does not ask.
    pub available_modes: Vec<String>,
    /// The agent's own settings, as `id -> currentValue`.
    ///
    /// `goose acp` returns a `configOptions` array on `session/new` naming its
    /// provider, its model where it has one, and whatever else it lets a client
    /// change. Kept whole rather than reduced to the one field farseer reads
    /// today, because which keys an agent offers is itself an observation - and
    /// `10 runner inventory`'s rule is that farseer reports what it saw.
    pub config: Vec<(String, String)>,
}

impl SessionOpened {
    /// The account behind this session, if the agent names one.
    ///
    /// `28 operator surface` wanted a provider on the conversation and could
    /// only get it from `runners.toml` - which is what the **operator declared**,
    /// not what the agent is using. This is the agent's own answer.
    ///
    /// `provider` is goose's key. ACP does not standardise `configOptions` ids,
    /// so an agent using another word reports nothing here rather than farseer
    /// guessing at a synonym.
    pub fn provider(&self) -> Option<&str> {
        self.setting("provider")
    }

    /// The model this session will actually use, if the agent names one.
    ///
    /// `opencode acp` answers this and names no provider; `goose acp` does the
    /// opposite. Neither is a gap in the other - they are different agents
    /// answering the questions they can, which is why both are read out of the
    /// same untouched `configOptions` rather than from a shape farseer imposed.
    pub fn model(&self) -> Option<&str> {
        self.setting("model")
    }

    /// Whether the agent will accept a mode change to `mode_id`.
    ///
    /// An agent that advertises **no modes at all** returns false for every id,
    /// which is the point: `opencode acp` has no `modes` in its `session/new`
    /// result, and asking it to set one is a JSON-RPC error that fails the run
    /// before the goal is ever sent.
    pub fn accepts_mode(&self, mode_id: &str) -> bool {
        self.available_modes.iter().any(|mode| mode == mode_id)
    }

    fn setting(&self, id: &str) -> Option<&str> {
        self.config
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, value)| value.as_str())
    }
}

/// Reads a `session/new` result. Returns `None` for any other message, so a
/// driver can feed it every line without first classifying them.
pub fn session_opened(line: &str) -> Option<SessionOpened> {
    let v: Value = serde_json::from_str(line).ok()?;
    let result = v.get("result")?;
    let session_id = result.get("sessionId").and_then(Value::as_str)?.to_string();
    let modes = result.get("modes");
    Some(SessionOpened {
        session_id,
        current_mode: modes
            .and_then(|m| m.get("currentModeId"))
            .and_then(Value::as_str)
            .map(str::to_string),
        available_modes: modes
            .and_then(|m| m.get("availableModes"))
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        config: result
            .get("configOptions")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| {
                        let id = option.get("id").and_then(Value::as_str)?;
                        let current = option.get("currentValue").and_then(Value::as_str)?;
                        Some((id.to_string(), current.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Parse one line of an ACP agent's stdout. `Ok` always means "count this as
/// activity", matching every other adapter in this crate.
///
/// The parse is deliberately **stateless**: a terminal result is recognised by
/// carrying a `stopReason` rather than by matching the id farseer sent, so the
/// same function fits [`crate::drive::drive`] without the driver having to
/// track outstanding requests. The cost is that a second concurrent prompt on
/// one connection would be indistinguishable, which farseer does not do -
/// `05 run state model` gives a run one process.
pub fn parse_line(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
    let v: Value = serde_json::from_str(line).map_err(|e| ParseError(e.to_string()))?;

    // A response to something farseer asked for. Only the terminal one matters
    // here; `session/new` is read by `session_opened` in the driver, which knows
    // which id it sent.
    if let Some(result) = v.get("result")
        && let Some(stop) = result.get("stopReason").and_then(Value::as_str)
    {
        return Ok(vec![RunnerSignal::Finished(FinishedSignal {
            outcome: outcome_for(stop),
            // ACP's terminal `usage` is per-turn tokens; cumulative cost arrives
            // on `usage_update` instead, so cost is left to that path rather
            // than reported twice from two different denominators.
            cost_usd_micros: None,
            tokens: result
                .get("usage")
                .and_then(|u| u.get("totalTokens"))
                .and_then(Value::as_i64),
        })]);
    }

    // An error response to a request farseer sent is terminal for the run: the
    // agent will not be answering it.
    if let Some(error) = v.get("error")
        && v.get("id").is_some()
    {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("agent returned an error");
        return Ok(vec![
            RunnerSignal::Output(message.to_string()),
            RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Failed,
                cost_usd_micros: None,
                tokens: None,
            }),
        ]);
    }

    let method = v.get("method").and_then(Value::as_str).unwrap_or_default();

    // A request *from* the agent that farseer must answer, and cannot. Surfaced
    // rather than dropped: an unanswered permission request is a live process
    // producing nothing, which is the hang `28 operator surface` already paid for once.
    if method == "session/request_permission" {
        return Ok(vec![RunnerSignal::Progress {
            kind: EventKind::new(EventKind::PERMISSION_REQUESTED),
            payload: v.get("params").cloned().unwrap_or(Value::Null),
        }]);
    }

    if method != "session/update" {
        return Ok(Vec::new());
    }
    let Some(update) = v.get("params").and_then(|p| p.get("update")) else {
        return Ok(Vec::new());
    };

    let signal = match update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        // Streamed a token at a time, which is what makes an ACP agent pass
        // `05 run state model`'s activity test where the same harness's one-object-per-run mode
        // fails it. `29 harness protocol` un-failed Gemini CLI on exactly this.
        "agent_message_chunk" => update
            .get("content")
            .filter(|content| content.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| RunnerSignal::OutputChunk(text.to_string())),
        // The field `28 operator surface` had no source for. `used` and `size` are the
        // context window, not the subscription window - ACP has no concept of
        // the latter, which is why this runner is quota-blind.
        "usage_update" => Some(RunnerSignal::Usage(UsageInfo {
            used: update.get("used").and_then(Value::as_i64),
            size: update.get("size").and_then(Value::as_i64),
            cost_usd_micros: update
                .get("cost")
                .and_then(|cost| cost.get("amount"))
                .and_then(Value::as_f64)
                .map(|amount| (amount * 1_000_000.0).round() as i64),
        })),
        "current_mode_update" => update
            .get("currentModeId")
            .and_then(Value::as_str)
            .map(|mode| RunnerSignal::Progress {
                kind: EventKind::new(EventKind::MODE_CHANGED),
                payload: json!({ "mode": mode }),
            }),
        _ => None,
    };
    Ok(signal.into_iter().collect())
}

/// ACP's context-window reading, and the cumulative cost beside it.
///
/// Both are **session-level and cumulative**, which is why they are one signal:
/// a caller that treats `used` as a per-turn delta will double-count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageInfo {
    /// Context tokens in use.
    pub used: Option<i64>,
    /// The window they are being used out of. The whole point.
    pub size: Option<i64>,
    /// Cumulative session cost, converted to the store's integer micros so no
    /// float reaches the record.
    pub cost_usd_micros: Option<i64>,
}

/// ACP's `stopReason` onto `05 run state model`'s outcomes.
///
/// `29 harness protocol` flagged that neither vocabulary covers the other:
/// `refusal` and `max_turn_requests` both land on `failed` here, which is
/// honest but lossy, and `26 routing policy` already suspects farseer needs a
/// fifth outcome. Deciding that is one ticket's job rather than this mapping's.
fn outcome_for(stop_reason: &str) -> Outcome {
    match stop_reason {
        "end_turn" => Outcome::Ok,
        "cancelled" => Outcome::Cancelled,
        _ => Outcome::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixture below is a literal line from the `goose acp` 1.47.0
    /// transcript captured on 2026-08-26, trimmed only of `_meta`.
    #[test]
    fn a_usage_update_carries_the_context_window_farseer_had_no_source_for() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"20260825_10","update":{"sessionUpdate":"usage_update","used":4560,"size":1050000,"cost":{"amount":0.000918,"currency":"USD"}}}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Usage(UsageInfo {
                used: Some(4560),
                size: Some(1_050_000),
                cost_usd_micros: Some(918),
            })]
        );
    }

    #[test]
    fn a_usage_update_before_the_first_turn_reports_a_window_and_no_cost() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"20260825_10","update":{"sessionUpdate":"usage_update","used":0,"size":1050000}}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Usage(UsageInfo {
                used: Some(0),
                size: Some(1_050_000),
                cost_usd_micros: None,
            })]
        );
    }

    /// The capture really did arrive as "Hello" then "!". A signal per fragment
    /// would be two answers in the record for one sentence.
    #[test]
    fn message_chunks_are_fragments_rather_than_answers() {
        let hello = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hello"},"messageId":"resp_1"}}}"#;
        let bang = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"!"},"messageId":"resp_1"}}}"#;
        assert_eq!(
            parse_line(hello).unwrap(),
            vec![RunnerSignal::OutputChunk("Hello".into())]
        );
        assert_eq!(
            parse_line(bang).unwrap(),
            vec![RunnerSignal::OutputChunk("!".into())]
        );
    }

    #[test]
    fn a_stop_reason_ends_the_turn_with_its_token_total() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"totalTokens":4560,"inputTokens":4554,"outputTokens":6}}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Ok,
                cost_usd_micros: None,
                tokens: Some(4560),
            })]
        );
    }

    #[test]
    fn cost_is_not_reported_twice_from_two_denominators() {
        // The terminal result's usage is per-turn; `usage_update`'s cost is
        // cumulative. Reading cost from both would add a session total to a
        // turn total.
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"totalTokens":10}}}"#;
        let RunnerSignal::Finished(finished) = &parse_line(line).unwrap()[0] else {
            panic!("expected a terminal signal");
        };
        assert_eq!(finished.cost_usd_micros, None);
    }

    #[test]
    fn every_stop_reason_lands_on_an_outcome_and_two_of_them_lose_information() {
        assert_eq!(outcome_for("end_turn"), Outcome::Ok);
        assert_eq!(outcome_for("cancelled"), Outcome::Cancelled);
        assert_eq!(outcome_for("max_tokens"), Outcome::Failed);
        // Both of these mean something `05` cannot express. Recorded as a test
        // so the day farseer grows a fifth outcome, this fails and says where.
        assert_eq!(outcome_for("refusal"), Outcome::Failed);
        assert_eq!(outcome_for("max_turn_requests"), Outcome::Failed);
    }

    #[test]
    fn a_permission_request_is_surfaced_rather_than_dropped() {
        let line = r#"{"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1"}}}"#;
        let signals = parse_line(line).unwrap();
        let RunnerSignal::Progress { kind, .. } = &signals[0] else {
            panic!("a permission request nobody answers is a hang, and must be visible");
        };
        assert_eq!(kind.as_str(), EventKind::PERMISSION_REQUESTED);
    }

    #[test]
    fn an_error_response_ends_the_run_and_quotes_the_agent() {
        let line = r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32603,"message":"no provider configured"}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![
                RunnerSignal::Output("no provider configured".into()),
                RunnerSignal::Finished(FinishedSignal {
                    outcome: Outcome::Failed,
                    cost_usd_micros: None,
                    tokens: None,
                }),
            ]
        );
    }

    #[test]
    fn session_new_names_the_session_and_the_modes_it_will_accept() {
        let line = r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"20260825_10","modes":{"currentModeId":"auto","availableModes":[{"id":"auto","name":"auto"},{"id":"approve","name":"approve"},{"id":"smart_approve","name":"smart_approve"},{"id":"chat","name":"chat"}]}}}"#;
        let opened = session_opened(line).unwrap();
        assert_eq!(opened.session_id, "20260825_10");
        assert_eq!(opened.current_mode.as_deref(), Some("auto"));
        assert_eq!(
            opened.available_modes,
            vec!["auto", "approve", "smart_approve", "chat"]
        );
        assert!(opened.accepts_mode("auto"));
        assert!(!opened.accepts_mode("yolo"));
    }

    /// Trimmed from the real `session/new` result, which carried a
    /// `configOptions` array of selects.
    #[test]
    fn session_new_names_the_provider_the_agent_is_actually_using() {
        let line = r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s","configOptions":[{"id":"provider","name":"Provider","type":"select","currentValue":"chatgpt_codex"},{"id":"mode","currentValue":"auto"}]}}"#;
        let opened = session_opened(line).unwrap();
        assert_eq!(opened.provider(), Some("chatgpt_codex"));
        // Kept whole: which keys an agent offers is an observation too.
        assert_eq!(opened.config.len(), 2);
    }

    /// Trimmed from the real `opencode acp` 1.18.22 `session/new` result on
    /// 2026-08-26, which named a model, offered **no modes at all**, and named
    /// no provider.
    #[test]
    fn a_second_agent_answers_the_other_half_of_the_same_question() {
        let line = r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"ses_fc4f","configOptions":[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"opencode/big-pickle"}]}}"#;
        let opened = session_opened(line).unwrap();
        assert_eq!(opened.model(), Some("opencode/big-pickle"));
        assert_eq!(opened.provider(), None);
        // And the thing that would have failed every run: no modes advertised.
        assert!(opened.available_modes.is_empty());
        assert!(!opened.accepts_mode("auto"));
    }

    #[test]
    fn an_agent_that_names_no_provider_reports_none_rather_than_a_guess() {
        let line = r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s","configOptions":[{"id":"llm","currentValue":"something"}]}}"#;
        assert_eq!(session_opened(line).unwrap().provider(), None);
    }

    #[test]
    fn session_opened_ignores_anything_that_is_not_a_session() {
        let notification = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"usage_update","used":1,"size":2}}}"#;
        assert!(session_opened(notification).is_none());
        assert!(session_opened("not json").is_none());
    }

    #[test]
    fn initialize_declines_the_capabilities_that_would_dissolve_the_workspace() {
        let frame: Value = serde_json::from_str(&initialize_frame(1)).unwrap();
        let caps = &frame["params"]["clientCapabilities"];
        assert_eq!(caps["fs"]["readTextFile"], json!(false));
        assert_eq!(caps["fs"]["writeTextFile"], json!(false));
        assert_eq!(caps["terminal"], json!(false));
    }

    #[test]
    fn a_prompt_frame_survives_text_that_would_break_hand_built_json() {
        let frame: Value =
            serde_json::from_str(&prompt_frame(3, "s", "a \"quoted\" line\nand another")).unwrap();
        assert_eq!(
            frame["params"]["prompt"][0]["text"],
            json!("a \"quoted\" line\nand another")
        );
    }

    #[test]
    fn cancel_is_a_notification_and_carries_no_id() {
        let frame: Value = serde_json::from_str(&cancel_frame("s")).unwrap();
        assert_eq!(frame["method"], json!("session/cancel"));
        assert!(
            frame.get("id").is_none(),
            "ACP expects no reply to a cancel"
        );
    }

    #[test]
    fn unrecognised_updates_are_activity_and_nothing_more() {
        // Real lines from the capture that farseer has no use for yet. They must
        // parse to nothing rather than to an error, because `drive` treats a
        // parse failure as a different kind of evidence.
        for line in [
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"compact"}]}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"session_info_update","title":"Greeting request"}}}"#,
        ] {
            assert_eq!(parse_line(line).unwrap(), Vec::new());
        }
    }
}
