//! The Codex app-server runner: the face of Codex farseer had not opened.
//!
//! `codex exec --json`, which [`crate::codex`] drives, is the **cut-down**
//! face. `30 codex app server` looked at the real one - 95 client methods and 75
//! notifications, exact rather than transcribed, because
//! `codex app-server generate-json-schema` makes Codex emit its own protocol -
//! and found three things arriving unprompted in a six-second headless turn that
//! correct four closed tickets.
//!
//! Every mapping below is backed by a literal line from the live probe of
//! `codex-cli 0.149.1` on 2026-08-26, per this crate's rule that no progress
//! mapping is guessed at past its verified shape.
//!
//! # What this face reports that `codex exec` does not
//!
//! - **A context window.** `thread/tokenUsage/updated` carries `last`, `total`
//!   **and** `modelContextWindow`. `28 operator surface` asked for context info
//!   and `29 harness protocol` argued about which token scope was the honest
//!   answer; Codex sends both scopes and the denominator, so the argument was
//!   two adapters each able to answer half.
//! - **A compaction boundary.** `thread/compacted`. `10 runner inventory` scored
//!   that column and found only Claude Code passed it; ACP still has none at all.
//! - **Quota, with a percentage.** `account/rateLimits/updated` - see
//!   [`RateLimits`], and see `27 quota accounting`'s correction for why a
//!   provider-reported percentage is admissible where a derived one is not.
//!
//! # Why the deltas are activity and the item is the answer
//!
//! `item/agentMessage/delta` streams fragments and `item/completed` carries the
//! **assembled** text. Farseer learned this once already on ACP, where no
//! assembled form exists and [`RunnerSignal::OutputChunk`] had to accumulate
//! them. Here the runner does the assembling, so the deltas are activity - which
//! is what `05 run state model` says a token stream is - and the completed item
//! is the answer.

use crate::claude_code::{FinishedSignal, ParseError, RunnerSignal};
use farseer_core::event::EventKind;
use farseer_core::run::Outcome;
use serde_json::{Value, json};

/// `initialize`. `clientInfo` is required and farseer names itself honestly:
/// the app-server records which client opened a thread, and a thread the
/// operator finds later should say what put it there.
pub fn initialize_frame(id: i64) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "farseer",
                "title": "farseer",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }
    })
    .to_string()
}

/// The notification the server waits for before it will do anything.
pub fn initialized_frame() -> String {
    json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }).to_string()
}

/// `thread/start` in a workspace.
///
/// `sandbox` is named explicitly rather than left to the operator's own
/// `config.toml`: `12 autonomy and deny list` decides reach before a run starts,
/// and `10 runner inventory` measured that Codex's own `--sandbox read-only`
/// did not prevent a write on this machine - so this is a request, not a
/// guarantee, and the guarantee remains the worktree.
pub fn thread_start_frame(id: i64, cwd: &str, sandbox: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "thread/start",
        "params": { "cwd": cwd, "sandbox": sandbox }
    })
    .to_string()
}

/// `turn/start` - the goal, with the two knobs `28 operator surface` reported as
/// unreportable and `29 harness protocol` correctly guessed were merely
/// unrequested.
///
/// `model` and `effort` are both optional and both absent by default, because
/// `10 runner inventory`'s rule cuts this way too: farseer asking for a model
/// nobody configured is farseer advertising rather than observing.
pub fn turn_start_frame(
    id: i64,
    thread_id: &str,
    text: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> String {
    let mut params = json!({
        "threadId": thread_id,
        "input": [{ "type": "text", "text": text }],
    });
    if let Some(model) = model {
        params["model"] = json!(model);
    }
    if let Some(effort) = effort {
        params["effort"] = json!(effort);
    }
    json!({ "jsonrpc": "2.0", "id": id, "method": "turn/start", "params": params }).to_string()
}

/// `turn/interrupt`: `05 run state model`'s `cancel`, acknowledged by the runner
/// rather than inferred from a killed process.
pub fn interrupt_frame(id: i64, thread_id: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "turn/interrupt",
        "params": { "threadId": thread_id }
    })
    .to_string()
}

/// Walk the handshake on a process somebody else spawned, leaving a thread
/// ready for [`turn_start_frame`].
///
/// Three steps rather than ACP's two, and the middle one is a **notification**:
/// the server does nothing until `initialized` arrives, and a client that skips
/// it waits forever on a `thread/start` the server has not begun listening for.
/// That is the third time this crate has met the same shape - a live process
/// producing nothing because of a handshake detail - so it is spelled out here
/// rather than left to the reader of a trace.
#[cfg(windows)]
pub fn handshake<F>(
    process: &mut crate::spawn::SupervisedProcess,
    cwd: &std::path::Path,
    sandbox: &str,
    ids: &mut crate::jsonrpc::Ids,
    on_line: &mut F,
) -> Result<ThreadOpened, crate::jsonrpc::RpcError>
where
    F: FnMut(Result<Vec<RunnerSignal>, ParseError>),
{
    use crate::jsonrpc::{RpcError, request};

    let id = ids.next();
    request(
        process,
        &initialize_frame(id),
        id,
        "initialize",
        parse_line,
        on_line,
    )?;
    process.write_line(&initialized_frame())?;

    let id = ids.next();
    let answer = request(
        process,
        &thread_start_frame(id, &cwd.to_string_lossy(), sandbox),
        id,
        "thread/start",
        parse_line,
        on_line,
    )?;
    let thread_id = thread_started(&answer.to_string()).ok_or(RpcError::Missing {
        method: "thread/start",
        field: "threadId",
    })?;

    // Asked for, not assumed. A runner that will not answer this is a runner
    // with no hint to give rather than a failed handshake, so a refusal here is
    // swallowed - the thread is already open and the goal still has to reach it.
    let id = ids.next();
    let configured = request(
        process,
        &config_read_frame(id),
        id,
        "config/read",
        parse_line,
        on_line,
    )
    .ok()
    .map(|answer| configured(&answer))
    .filter(|hint| !hint.is_empty());

    Ok(ThreadOpened {
        thread_id,
        configured,
    })
}

/// A thread, and what the runner is configured to bring to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadOpened {
    pub thread_id: String,
    /// A **hint**: the runner's own defaults, which farseer reads and never
    /// sets. See [`configured`].
    pub configured: Option<crate::claude_code::Configured>,
}

/// What the app-server says about the account's windows.
///
/// **Two windows at once**, which nothing else farseer drives reports: a short
/// rolling one and a weekly one, each with its own reset and its own
/// `usedPercent`. `27 quota accounting`'s `WindowObservation` holds one, which
/// is why this is read into its own shape and not yet forced into that one -
/// `30 codex app server` records widening it as the next step rather than
/// something to guess at here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimits {
    pub primary: Option<RateLimitWindow>,
    pub secondary: Option<RateLimitWindow>,
    /// `plus`, `pro`, `enterprise` and a dozen more. No other runner says.
    pub plan_type: Option<String>,
    /// Present only once a limit has actually been hit, so its absence is the
    /// normal case rather than a missing reading.
    pub reached: Option<String>,
}

/// One rolling window.
///
/// `usedPercent` is **the provider's own number**, which matters: `27 quota
/// accounting` refused to report a percentage farseer *derived from its own
/// spend*, because that is a lower bound on a window other sessions drain and is
/// most wrong exactly near exhaustion. This one is not that number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitWindow {
    pub used_percent: i64,
    pub resets_at: Option<i64>,
    pub window_duration_mins: Option<i64>,
}

/// Read `account/rateLimits/updated`, or `None` for any other line.
pub fn rate_limits(line: &str) -> Option<RateLimits> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("method").and_then(Value::as_str)? != "account/rateLimits/updated" {
        return None;
    }
    let limits = v.get("params")?.get("rateLimits")?;
    Some(RateLimits {
        primary: limits.get("primary").and_then(window),
        secondary: limits.get("secondary").and_then(window),
        plan_type: text(limits, "planType"),
        reached: text(limits, "rateLimitReachedType"),
    })
}

fn window(v: &Value) -> Option<RateLimitWindow> {
    Some(RateLimitWindow {
        used_percent: v.get("usedPercent").and_then(Value::as_i64)?,
        resets_at: v.get("resetsAt").and_then(Value::as_i64),
        window_duration_mins: v.get("windowDurationMins").and_then(Value::as_i64),
    })
}

fn text(v: &Value, field: &str) -> Option<String> {
    v.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl RateLimits {
    /// Both windows, in `27 quota accounting`'s own shape.
    ///
    /// `account` and `runner` are left empty: the adapter does not know which
    /// login it is on, and `27` is explicit that the account is **declared by
    /// the operator, never inferred**. The layer that reads runner config fills
    /// them in.
    ///
    /// The provider's own `usedPercent` travels as-is. `27` refused a percentage
    /// farseer would *derive from its own spend*; this is not that number, and
    /// nothing here computes anything.
    pub fn observations(&self) -> Vec<farseer_core::WindowObservation> {
        [("primary", self.primary), ("secondary", self.secondary)]
            .into_iter()
            .filter_map(|(name, window)| {
                let window = window?;
                Some(farseer_core::WindowObservation {
                    account: String::new(),
                    runner: String::new(),
                    // A percentage is not a status. Codex says how full a window
                    // is and separately whether a limit has been **reached**, so
                    // exhaustion is read from the latter and never inferred from
                    // the former - 100% and refused are different claims.
                    availability: match (&self.reached, window.resets_at) {
                        (Some(_), Some(resets_at)) => {
                            farseer_core::Availability::ExhaustedUntil { resets_at }
                        }
                        (_, resets_at) => farseer_core::Availability::Allowed { resets_at },
                    },
                    // The provider's own word for which window this is.
                    rate_limit_type: name.to_string(),
                    // Codex reports credits and a spend control, neither of which
                    // is Claude Code's overage. Absent rather than mapped onto a
                    // word that means something else.
                    is_using_overage: false,
                    used_percent: Some(window.used_percent),
                    window_duration_mins: window.window_duration_mins,
                })
            })
            .collect()
    }
}

/// Read `config/read`'s answer into the hint farseer surfaces.
///
/// **Read, never written.** `28 operator surface` asked for a thinking level and
/// `29 harness protocol` found it settable per turn; `30 codex app server`
/// decided farseer does not set it, and this is that decision in code. On this
/// machine `~/.codex/config.toml` says `xhigh`, and a farseer that sent its own
/// value would silently downgrade every run the operator configured up.
///
/// The origin comes back too, which turns "why was that run slow" into a file
/// path rather than a guess.
pub fn configured(answer: &Value) -> crate::claude_code::Configured {
    let config = answer
        .get("result")
        .and_then(|result| result.get("config"))
        .unwrap_or(&Value::Null);
    crate::claude_code::Configured {
        model: text(config, "model"),
        effort: text(config, "model_reasoning_effort"),
        from: answer
            .get("result")
            .and_then(|result| result.get("origins"))
            .and_then(|origins| origins.get("model_reasoning_effort"))
            .and_then(|origin| origin.get("name"))
            .and_then(|name| name.get("file"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// `config/read`, which takes no parameters.
pub fn config_read_frame(id: i64) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": "config/read", "params": {} }).to_string()
}

/// The thread id out of a `thread/start` response, for the driver that sent it.
pub fn thread_started(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    let result = v.get("result")?;
    result
        .get("threadId")
        .or_else(|| result.get("thread").and_then(|t| t.get("id")))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Parse one line. `Ok` always means "count this as activity", as everywhere
/// else in this crate.
pub fn parse_line(line: &str) -> Result<Vec<RunnerSignal>, ParseError> {
    let v: Value = serde_json::from_str(line).map_err(|e| ParseError(e.to_string()))?;
    let method = v.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = v.get("params").unwrap_or(&Value::Null);

    let signal = match method {
        // The assembled answer, rather than the fragments that preceded it.
        "item/completed" => params
            .get("item")
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("agentMessage"))
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| RunnerSignal::Output(text.to_string())),
        // The denominator, and the totals either side of it. `total` rather than
        // `last`: the context window is filled by the whole thread, and a turn's
        // own tokens are a different question that `11 analytics questions` owns.
        "thread/tokenUsage/updated" => params.get("tokenUsage").map(|usage| {
            RunnerSignal::Usage(crate::acp::UsageInfo {
                used: usage
                    .get("total")
                    .and_then(|total| total.get("totalTokens"))
                    .and_then(Value::as_i64),
                size: usage.get("modelContextWindow").and_then(Value::as_i64),
                // Codex reports tokens and never currency, which `10 runner
                // inventory` measured and this face does not change.
                cost_usd_micros: None,
            })
        }),
        // `02 record scope`: farseer can record **that** a compaction happened
        // and **when**, never what was dropped. The second runner able to say so.
        "thread/compacted" => Some(RunnerSignal::Progress {
            kind: EventKind::new(EventKind::CONTEXT_COMPACTED),
            payload: json!({ "turn_id": params.get("turnId") }),
        }),
        // The line `10 runner inventory` recorded as impossible for this
        // runner, because it measured the other face of the same binary.
        "account/rateLimits/updated" => rate_limits(line)
            .map(|limits| limits.observations())
            .filter(|windows| !windows.is_empty())
            .map(RunnerSignal::Windows),
        "turn/completed" => {
            let turn = params.get("turn").unwrap_or(&Value::Null);
            let outcome = match turn.get("status").and_then(Value::as_str) {
                Some("completed") => Outcome::Ok,
                // `05 run state model`: only a human choosing not to proceed is
                // `Cancelled`, and an interrupt is exactly that - farseer's own
                // `turn/interrupt`, which no watchdog sends on its own.
                Some("interrupted") => Outcome::Cancelled,
                _ => Outcome::Failed,
            };
            Some(RunnerSignal::Finished(FinishedSignal {
                outcome,
                cost_usd_micros: None,
                // Reported on its own notification, so reading it here as well
                // would be one number arriving twice by two routes.
                tokens: None,
            }))
        }
        _ => None,
    };
    Ok(signal.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixture is a literal line from the 2026-08-26 probe of
    /// `codex app-server` 0.149.1, trimmed only of ids and timestamps.
    #[test]
    fn token_usage_carries_the_denominator_and_the_session_total() {
        let line = r#"{"method":"thread/tokenUsage/updated","params":{"threadId":"t","turnId":"u","tokenUsage":{"total":{"totalTokens":22287,"inputTokens":22281,"cachedInputTokens":0,"cacheWriteInputTokens":0,"outputTokens":6,"reasoningOutputTokens":0},"last":{"totalTokens":22287},"modelContextWindow":258400}}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Usage(crate::acp::UsageInfo {
                used: Some(22287),
                size: Some(258_400),
                cost_usd_micros: None,
            })]
        );
    }

    #[test]
    fn the_assembled_message_is_the_answer_and_the_deltas_are_activity() {
        let delta = r#"{"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"u","itemId":"msg_1","delta":"Hello"}}"#;
        let completed = r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"msg_1","text":"Hello!","phase":"final_answer"},"threadId":"t","turnId":"u"}}"#;
        assert_eq!(parse_line(delta).unwrap(), Vec::new());
        assert_eq!(
            parse_line(completed).unwrap(),
            vec![RunnerSignal::Output("Hello!".into())]
        );
    }

    #[test]
    fn the_operators_own_message_is_not_the_agents_answer() {
        // `item/completed` fires for the user's turn too, and recording it would
        // put the goal in the record as though the manager had said it.
        let line = r#"{"method":"item/completed","params":{"item":{"type":"userMessage","id":"u1","content":[{"type":"text","text":"Say hello in one short sentence."}]},"threadId":"t","turnId":"u"}}"#;
        assert_eq!(parse_line(line).unwrap(), Vec::new());
    }

    #[test]
    fn a_completed_turn_is_ok_and_does_not_report_tokens_twice() {
        let line = r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","items":[],"itemsView":"summary","status":"completed","error":null,"durationMs":6058}}}"#;
        assert_eq!(
            parse_line(line).unwrap(),
            vec![RunnerSignal::Finished(FinishedSignal {
                outcome: Outcome::Ok,
                cost_usd_micros: None,
                tokens: None,
            })]
        );
    }

    #[test]
    fn every_turn_status_lands_on_an_outcome_and_only_an_interrupt_is_cancelled() {
        let status = |s: &str| {
            let line =
                format!(r#"{{"method":"turn/completed","params":{{"turn":{{"status":"{s}"}}}}}}"#);
            match &parse_line(&line).unwrap()[0] {
                RunnerSignal::Finished(f) => f.outcome,
                other => panic!("expected a terminal signal, got {other:?}"),
            }
        };
        assert_eq!(status("completed"), Outcome::Ok);
        assert_eq!(status("failed"), Outcome::Failed);
        // `05 run state model`: only a human choosing not to proceed.
        assert_eq!(status("interrupted"), Outcome::Cancelled);
    }

    #[test]
    fn a_compaction_boundary_is_recorded_without_claiming_to_know_what_was_dropped() {
        let line = r#"{"method":"thread/compacted","params":{"threadId":"t","turnId":"u"}}"#;
        let signals = parse_line(line).unwrap();
        let RunnerSignal::Progress { kind, payload } = &signals[0] else {
            panic!("expected a progress signal, got {signals:?}");
        };
        assert_eq!(kind.as_str(), EventKind::CONTEXT_COMPACTED);
        assert_eq!(payload.get("turn_id").unwrap(), &json!("u"));
    }

    /// The line `10 runner inventory` recorded as impossible for this runner.
    #[test]
    fn the_account_windows_arrive_unprompted_with_the_providers_own_percentage() {
        let line = r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":1787710593},"secondary":{"usedPercent":0,"windowDurationMins":10080,"resetsAt":1788273509},"credits":{"hasCredits":false,"unlimited":false,"balance":"0"},"individualLimit":null,"spendControlReached":null,"planType":"plus","rateLimitReachedType":null}}}"#;
        let limits = rate_limits(line).unwrap();
        assert_eq!(
            limits.primary,
            Some(RateLimitWindow {
                used_percent: 0,
                resets_at: Some(1787710593),
                window_duration_mins: Some(300),
            })
        );
        // Two windows, which nothing else farseer drives reports.
        assert_eq!(limits.secondary.unwrap().window_duration_mins, Some(10080));
        assert_eq!(limits.plan_type.as_deref(), Some("plus"));
        // Absent is the normal case: a limit that has not been hit says nothing.
        assert_eq!(limits.reached, None);
    }

    #[test]
    fn both_windows_reach_the_record_in_the_shape_it_already_had() {
        let line = r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":12,"windowDurationMins":300,"resetsAt":1787710593},"secondary":{"usedPercent":3,"windowDurationMins":10080,"resetsAt":1788273509},"planType":"plus"}}}"#;
        let signals = parse_line(line).unwrap();
        let [RunnerSignal::Windows(windows)] = signals.as_slice() else {
            panic!("expected one windows signal, got {signals:?}");
        };
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].rate_limit_type, "primary");
        assert_eq!(windows[0].used_percent, Some(12));
        assert_eq!(windows[1].window_duration_mins, Some(10080));
        // Left for the layer that reads runner config: `27 quota accounting`
        // declares the account, never infers it.
        assert!(windows.iter().all(|w| w.account.is_empty()));
    }

    #[test]
    fn a_full_window_is_not_an_exhausted_one_until_the_provider_says_so() {
        // `usedPercent` is how full, `rateLimitReachedType` is whether refused.
        // Reading exhaustion off the percentage would be farseer deciding a
        // limit had been hit before the provider did.
        let full = r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":100,"resetsAt":9}}}}"#;
        let signals = parse_line(full).unwrap();
        let [RunnerSignal::Windows(windows)] = signals.as_slice() else {
            panic!("expected a windows signal");
        };
        assert_eq!(
            windows[0].availability,
            farseer_core::Availability::Allowed { resets_at: Some(9) }
        );

        let reached = r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":100,"resetsAt":9},"rateLimitReachedType":"rate_limit_reached"}}}"#;
        let signals = parse_line(reached).unwrap();
        let [RunnerSignal::Windows(windows)] = signals.as_slice() else {
            panic!("expected a windows signal");
        };
        assert_eq!(
            windows[0].availability,
            farseer_core::Availability::ExhaustedUntil { resets_at: 9 }
        );
    }

    /// Trimmed from the real `config/read` answer on 2026-08-26, which was
    /// 25kB and named the file behind every setting.
    #[test]
    fn the_configured_default_is_read_with_the_file_that_set_it() {
        let answer: Value = serde_json::from_str(
            r#"{"id":10,"result":{
                 "config":{"model":"gpt-5.6-sol","model_reasoning_effort":"xhigh"},
                 "origins":{"model_reasoning_effort":{"name":{"type":"user","file":"C:\\Users\\me\\.codex\\config.toml"}}}}}"#,
        )
        .unwrap();
        let hint = configured(&answer);
        assert_eq!(hint.effort.as_deref(), Some("xhigh"));
        assert_eq!(hint.model.as_deref(), Some("gpt-5.6-sol"));
        // "why was that run slow" becomes a path rather than a guess.
        assert!(hint.from.unwrap().ends_with("config.toml"));
    }

    #[test]
    fn a_runner_that_configures_nothing_offers_no_hint() {
        let answer: Value = serde_json::from_str(r#"{"id":10,"result":{"config":{}}}"#).unwrap();
        assert!(configured(&answer).is_empty());
    }

    #[test]
    fn farseer_reads_the_effort_and_never_sends_one() {
        // The operator's machine says `xhigh`. A turn that carried farseer's own
        // value would silently downgrade every run they configured up, so the
        // production frame carries none - `30 codex app server`'s decision, and
        // this is the test that keeps it true.
        let frame: Value =
            serde_json::from_str(&turn_start_frame(3, "t", "go", None, None)).unwrap();
        assert!(frame["params"].get("effort").is_none());
        assert!(frame["params"].get("model").is_none());
    }

    #[test]
    fn the_thread_id_is_read_out_of_whichever_shape_the_response_uses() {
        let flat = r#"{"id":2,"result":{"threadId":"01a03b5e"}}"#;
        let nested = r#"{"id":2,"result":{"thread":{"id":"01a03b5e"}}}"#;
        assert_eq!(thread_started(flat).as_deref(), Some("01a03b5e"));
        assert_eq!(thread_started(nested).as_deref(), Some("01a03b5e"));
        assert!(thread_started(r#"{"method":"turn/started"}"#).is_none());
    }

    #[test]
    fn a_turn_asks_for_nothing_it_was_not_told_to_ask_for() {
        let bare: Value =
            serde_json::from_str(&turn_start_frame(3, "t", "go", None, None)).unwrap();
        assert!(bare["params"].get("model").is_none());
        assert!(bare["params"].get("effort").is_none());

        // And carries both when it was. `28 operator surface` called the
        // thinking level unreportable; it is settable, per turn.
        let asked: Value = serde_json::from_str(&turn_start_frame(
            3,
            "t",
            "go",
            Some("gpt-5.6"),
            Some("low"),
        ))
        .unwrap();
        assert_eq!(asked["params"]["effort"], json!("low"));
        assert_eq!(asked["params"]["model"], json!("gpt-5.6"));
    }

    #[test]
    fn a_turn_survives_text_that_would_break_hand_built_json() {
        let frame: Value = serde_json::from_str(&turn_start_frame(
            3,
            "t",
            "a \"quoted\" line\nand more",
            None,
            None,
        ))
        .unwrap();
        assert_eq!(
            frame["params"]["input"][0]["text"],
            json!("a \"quoted\" line\nand more")
        );
    }

    #[test]
    fn the_noise_of_a_real_thread_is_activity_and_nothing_more() {
        // Real lines from the probe. The hooks and MCP servers are the
        // operator's own, loaded before farseer said anything - `30 codex app
        // server` records that as something `12 autonomy and deny list` should
        // have an opinion about, not something this parser should invent one for.
        for line in [
            r#"{"method":"hook/completed","params":{"threadId":"t","turnId":"u","run":{"status":"failed"}}}"#,
            r#"{"method":"mcpServer/startupStatus/updated","params":{"threadId":"t","name":"node_repl","status":"ready"}}"#,
            r#"{"method":"thread/status/changed","params":{"threadId":"t","status":{"type":"idle"}}}"#,
            r#"{"method":"item/reasoning/textDelta","params":{"delta":"thinking"}}"#,
        ] {
            assert_eq!(parse_line(line).unwrap(), Vec::new());
        }
    }

    #[test]
    fn malformed_json_is_an_error_not_silence() {
        assert!(parse_line("not json").is_err());
    }
}
