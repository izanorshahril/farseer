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

use std::ops::Not;

use serde_json::{Value, json};

use farseer_core::{EventKind, Outcome, ToolLevel};

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

/// The runner's own tool names, at each level `36 tool grant enforcement` defines.
///
/// **Probed, not read.** 2026-08-28, by loading an extension that calls
/// `getAllTools` on a live session, because `10 runner inventory`'s rule holds here as
/// everywhere - a help page is an advertisement.
///
/// pi has eight: `bash`, `edit`, `find`, `grep`, `ls`, `powershell`, `read`,
/// `write`. omp has twenty-three, including `task` and `hub`, which is how it
/// spawns subagents - so `32 harness capability floor`'s open question about
/// whether an omp manager may run its own background jobs beside farseer's
/// workers is answered here, as a level, rather than as a special case.
///
/// [`ToolLevel::Shell`] returns `None` rather than a list of everything: it
/// means "pass no allowlist", so a runner that gains a tool in a later version
/// gains it here too. Enumerating it would freeze today's set and call it a
/// grant.
pub fn tool_allowlist(runner: &str, level: ToolLevel) -> Option<Vec<&'static str>> {
    if level == ToolLevel::Shell {
        return None;
    }
    let (read, edit): (&[&str], &[&str]) = match runner {
        "pi" => (&["read", "ls", "find", "grep"], &["edit", "write"]),
        "omp" => (
            &["read", "glob", "grep", "lsp", "todo"],
            &["edit", "write", "ast_edit"],
        ),
        _ => return Some(Vec::new()),
    };
    let mut names = read.to_vec();
    if level == ToolLevel::Edit {
        names.extend_from_slice(edit);
    }
    Some(names)
}

/// Whether farseer can hold this runner to a tool level at all.
///
/// Both take `--tools` as an allowlist, which is the shape `12 autonomy and deny list` asked
/// for: without a sandbox, grant lists beat deny lists. Stated as a list rather
/// than a negation so a new runner is silent until somebody probes it.
pub fn takes_tool_allowlist(runner: &str) -> bool {
    matches!(runner, "pi" | "omp")
}

/// Whether this runner can be handed a skill directory on the argv.
///
/// pi takes `--skill <path>`, repeatable. omp takes `--skills <globs>`, which
/// filters what it **discovered** - so with discovery denied there is nothing
/// left for the filter to match, and no argv at all that loads a named
/// directory. Probed 2026-08-28: `omp --skill` is `unknown flag`.
///
/// Exported because the answer has to be known **before** a run starts. A cell
/// declares its skills per `32 harness capability floor`, and a runner that
/// silently drops them gives back exactly the failure that ticket closed: a
/// worse answer, for a reason nobody can see.
pub fn loads_skills_by_path(runner: &str) -> bool {
    runner == "pi"
}

/// The launch argv, given what the operator pinned in `runners.toml`.
///
/// `--mode rpc` is the whole runner; everything after it is the operator's
/// declaration. Absent means absent: a model farseer does not pass is a model pi
/// chooses from its own config, which is the same deference `30 codex app
/// server` settled for effort.
pub fn build_args(
    // Which of the two. They speak one protocol and not one command line -
    // see [`loads_skills_by_path`] and [`UNATTENDED_EXCLUDED_TOOLS`].
    runner: &str,
    tool_level: ToolLevel,
    model: Option<&str>,
    effort: Option<&str>,
    skills: &[std::path::PathBuf],
    extensions: &[std::path::PathBuf],
    append_system_prompt: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["--mode".to_string(), "rpc".to_string()];
    if runner == "pi" {
        // The tool that waits for a person. `12 autonomy and deny list` decides
        // autonomy **before** the run, and nobody is watching a question
        // farseer did not expect - so a run that asks one is a run that hangs,
        // silently and forever. Measured on 2026-08-27: a pi manager given a
        // goal open enough to be worth asking about sat at `session_started`
        // for minutes with a live process and no output, which is
        // indistinguishable from a hang because it **is** one.
        //
        // The same call `29 harness protocol` made for ACP's unattended mode
        // and `30 codex app server` made for the sandbox, and the third time
        // this family has bitten - `06 cell transport` hit it first, as a
        // Claude Code tool missing from `--allowedTools`.
        //
        // pi only: omp has no `ask_question` and no `--exclude-tools` to name
        // it with. Probed 2026-08-28, both facts - its twenty tools are
        // `read`, `bash`, `task`, `hub` and the rest, and none of them waits
        // for a person.
        args.push("--exclude-tools".to_string());
        args.push(UNATTENDED_EXCLUDED_TOOLS.to_string());
    }
    if let Some(model) = model {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    if let Some(effort) = effort {
        args.push("--thinking".to_string());
        args.push(effort.to_string());
    }
    // `--no-skills` first, then the cell's own by path. Without the denial pi
    // discovers whatever is installed under the operator's `~/.agents/skills`,
    // which `32 harness capability floor` measured at 28 commands on this
    // machine - instructions farseer never chose loading into a run
    // `12 autonomy and deny list` is bounding. A cell gets what it declared and
    // nothing else, and a cell that declared nothing runs bare.
    // `36 tool grant enforcement`: the contract said what this run may do, and
    // until now said it only to the record. An allowlist rather than a denylist,
    // because `12 autonomy and deny list` found a denylist is routed around by
    // the first shell command.
    //
    // Names are farseer's, not the operator's: a bogus name in `--tools` is
    // **accepted in silence** by pi and yields a run holding nothing, so the
    // list comes from [`tool_allowlist`] and never from a cell file.
    if let Some(names) = tool_allowlist(runner, tool_level)
        && !names.is_empty()
    {
        args.push("--tools".to_string());
        args.push(names.join(","));
    }
    args.push("--no-skills".to_string());
    // ...and only pi can then be handed one back by path. omp's `--skills`
    // is a **glob filter over what it discovered**, which is the opposite
    // operation, so there is no argv that gives omp a specific directory.
    // Callers ask [`loads_skills_by_path`] before promising a cell its skills.
    if loads_skills_by_path(runner) {
        for skill in skills {
            args.push("--skill".to_string());
            args.push(skill.display().to_string());
        }
    }
    // The same rule for extensions, and for the same reason: an extension is
    // arbitrary code registering arbitrary tools into a run farseer is
    // bounding. `--no-extensions` denies discovery while leaving explicit `-e`
    // paths loadable, so the only extension in a farseer run is one farseer
    // handed it - today, the delegation tools of `31 manager delegation reach`.
    args.push("--no-extensions".to_string());
    for extension in extensions {
        args.push("-e".to_string());
        args.push(extension.display().to_string());
    }
    // Who this manager is and what its roster contains, per `31 manager
    // delegation reach`. Appended rather than replacing: pi's own coding-agent
    // prompt is why the runner is worth driving, and `13 harness build kit`
    // says farseer configures a harness rather than reimplementing one.
    if let Some(prompt) = append_system_prompt.filter(|text| !text.trim().is_empty()) {
        args.push("--append-system-prompt".to_string());
        args.push(prompt.to_string());
    }
    args
}

/// Tools farseer refuses to let an unattended run reach.
///
/// One entry, and it is not a capability: `ask_question` blocks the turn on a
/// human who is not there. Everything pi can actually *do* stays available -
/// `12 autonomy and deny list` bounds reach with the worktree and the deny
/// list, not by taking tools away.
/// pi's tool that waits for a person, denied because nobody is watching.
///
/// **pi's, not omp's.** They share this adapter because they share a protocol
/// verbatim, and sharing it hid that they do not share a command line: omp has
/// neither this tool nor the `--exclude-tools` flag to name it with, and
/// rejected the launch outright. See [`build_args`].
pub const UNATTENDED_EXCLUDED_TOOLS: &str = "ask_question";

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
        // What the runner actually did, as opposed to what it said it did.
        //
        // `02 record scope` named these kinds and pi is the first runner here
        // whose stream fills them. The distinction matters more than it looks:
        // before this, a thread showed only the agent's own prose - "Ran: echo
        // hello" was the **agent's claim**, and `31 manager delegation reach`
        // is the record of what happens when a claim like that is believed.
        // A tool call farseer saw is a fact; a tool call the agent describes is
        // a sentence.
        "tool_execution_start" => Some(RunnerSignal::Progress {
            kind: EventKind::new(EventKind::TOOL_CALL_STARTED),
            payload: json!({
                "tool_name": v.get("toolName"),
                "tool_call_id": v.get("toolCallId"),
                "args": v.get("args"),
            }),
        }),
        // The partial results in between are `05 run state model`'s activity -
        // they keep the watchdog awake and belong nowhere in the record.
        "tool_execution_end" => Some(RunnerSignal::Progress {
            kind: EventKind::new(EventKind::TOOL_RESULT),
            payload: json!({
                "tool_name": v.get("toolName"),
                "tool_call_id": v.get("toolCallId"),
                "is_error": v.get("isError"),
                "text": tool_text(v.get("result")),
            }),
        }),
        // `02 record scope`: farseer records **that** a compaction happened and
        // **when**, never what was dropped.
        //
        // Only when it actually happened. `compaction_end` fires whether or not
        // the compaction succeeded - the 2026-08-27 probe got one carrying
        // `"errorMessage": "Nothing to compact (session too small)"` - and
        // recording that as a compaction would put a false fact in the record
        // about the one thing `02` cares most about, since a result produced
        // after a compaction is a result produced from a summary.
        "compaction_end" => v
            .get("aborted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            .not()
            .then(|| v.get("errorMessage").filter(|e| !e.is_null()).is_none())
            .filter(|succeeded| *succeeded)
            .map(|_| RunnerSignal::Progress {
                kind: EventKind::new(EventKind::CONTEXT_COMPACTED),
                // `manual` when farseer or the operator asked, `auto` when pi
                // decided the context was full. A different fact about the run
                // in each case, so the record keeps which.
                payload: json!({ "reason": v.get("reason") }),
            }),
        // The agent loop settled, which is one or many turns later than the
        // first `turn_end`: a turn is one round-trip, and a tool call starts
        // another. Ending a worker at `turn_end` would cut the loop mid-tool.
        //
        // `isTerminal` is the second half of that, and omp is what taught it.
        // omp runs a subagent as a **named background job**: the foreground
        // loop calls `task`, then `hub {op: "wait"}`, then ends with
        // `"isTerminal": false` while the subagent is still going. A second
        // loop starts when the job's result arrives as an `async-result`
        // message, and *that* one is terminal. Treating the first as the end
        // would have ended a worker before its own subagent answered - the
        // same family of bug as ending at `turn_end`, one level further out.
        //
        // Absent means terminal: pi sends no such field and has no background
        // jobs to be waiting on.
        "agent_end" => {
            let spent = finished(&v);
            return Ok(
                if v.get("isTerminal").and_then(Value::as_bool) == Some(false) {
                    // Not the end, but real spending. Twice over: once for the
                    // operator reading the record, and once for the report,
                    // because the terminal `agent_end` carries only its own
                    // leg's messages and this leg's tokens are not in it.
                    vec![
                        RunnerSignal::Progress {
                            kind: EventKind::new(EventKind::TOOL_RESULT),
                            payload: json!({
                                "tool_name": "background job",
                                "tokens": spent.tokens,
                                "cost_usd_micros": spent.cost_usd_micros,
                            }),
                        },
                        RunnerSignal::LegSpend {
                            cost_usd_micros: spent.cost_usd_micros,
                            tokens: spent.tokens,
                        },
                    ]
                } else {
                    vec![RunnerSignal::Finished(spent)]
                },
            );
        }
        _ => None,
    };
    Ok(signal.into_iter().collect())
}

/// A tool result's text, flattened out of pi's content-block array.
///
/// Clipped, because a tool result is unbounded - a `read` of a large file lands
/// here whole - and `02 record scope` wants the record to say what happened
/// rather than to become a second copy of the workspace. The agent still sees
/// all of it; only the record is clipped, and it says so by ending in an
/// ellipsis rather than by trimming silently.
fn tool_text(result: Option<&Value>) -> Option<String> {
    const LIMIT: usize = 2_000;
    let joined: String = result?
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    if joined.is_empty() {
        return None;
    }
    if joined.chars().count() <= LIMIT {
        return Some(joined);
    }
    Some(format!(
        "{}...",
        joined.chars().take(LIMIT).collect::<String>()
    ))
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

    /// The record gets what the runner **did**, separately from what it said it
    /// did. `31 manager delegation reach` exists because those two came apart.
    #[test]
    fn a_tool_call_reaches_the_record_as_a_fact_rather_than_as_a_claim() {
        let start = r#"{"type":"tool_execution_start","toolCallId":"call_pfF","toolName":"bash","args":{"command":"echo hello"}}"#;
        let signals = parse_line(start).unwrap();
        let [RunnerSignal::Progress { kind, payload }] = &signals[..] else {
            panic!("one progress signal, got {signals:?}")
        };
        assert_eq!(kind.as_str(), EventKind::TOOL_CALL_STARTED);
        assert_eq!(payload["tool_name"], "bash");
        assert_eq!(payload["args"]["command"], "echo hello");

        let end = r#"{"type":"tool_execution_end","toolCallId":"call_pfF","toolName":"bash","result":{"content":[{"type":"text","text":"hello"}]},"isError":false}"#;
        let signals = parse_line(end).unwrap();
        let [RunnerSignal::Progress { kind, payload }] = &signals[..] else {
            panic!("one progress signal, got {signals:?}")
        };
        assert_eq!(kind.as_str(), EventKind::TOOL_RESULT);
        assert_eq!(payload["text"], "hello");
        assert_eq!(payload["is_error"], false);
    }

    /// A `read` of a large file is a tool result too. The record says what
    /// happened; it does not become a second copy of the workspace.
    #[test]
    fn a_huge_tool_result_is_clipped_and_says_so() {
        let big = "x".repeat(5_000);
        let line = format!(
            r#"{{"type":"tool_execution_end","toolName":"read","result":{{"content":[{{"type":"text","text":"{big}"}}]}}}}"#
        );
        let signals = parse_line(&line).unwrap();
        let [RunnerSignal::Progress { payload, .. }] = &signals[..] else {
            panic!("one signal")
        };
        let text = payload["text"].as_str().unwrap();
        assert!(text.ends_with("..."), "clipped results say so");
        assert!(text.chars().count() < 2_100, "{}", text.len());
    }

    /// `compaction_end` fires whether or not the compaction happened, and the
    /// one thing `02 record scope` most wants to be true is whether a result
    /// came out of a summary.
    #[test]
    fn a_compaction_that_did_not_happen_is_not_recorded_as_one() {
        let failed = r#"{"type":"compaction_end","reason":"manual","aborted":false,"willRetry":false,"errorMessage":"Compaction failed: Nothing to compact (session too small)"}"#;
        assert_eq!(parse_line(failed).unwrap(), vec![]);

        let aborted =
            r#"{"type":"compaction_end","reason":"auto","aborted":true,"willRetry":false}"#;
        assert_eq!(parse_line(aborted).unwrap(), vec![]);

        let real = r#"{"type":"compaction_end","reason":"auto","aborted":false,"willRetry":false}"#;
        let signals = parse_line(real).unwrap();
        let [RunnerSignal::Progress { kind, payload }] = &signals[..] else {
            panic!("one progress signal, got {signals:?}")
        };
        assert_eq!(kind.as_str(), EventKind::CONTEXT_COMPACTED);
        // Which kind of compaction it was: pi deciding the context was full is
        // a different fact about the run than farseer asking.
        assert_eq!(payload["reason"], "auto");
    }

    /// `36 tool grant enforcement`. The contract said what a run may do and said
    /// it only to the record; this is the sentence that reaches the process.
    #[test]
    fn a_tool_level_reaches_the_argv_and_shell_asks_for_nothing() {
        let read = build_args("pi", ToolLevel::Read, None, None, &[], &[], None);
        let i = read
            .iter()
            .position(|a| a == "--tools")
            .expect("an allowlist");
        assert_eq!(read[i + 1], "read,ls,find,grep");
        assert!(!read[i + 1].contains("bash"), "read may not reach a shell");
        assert!(!read[i + 1].contains("write"), "read may not write");

        // `12 autonomy and deny list`'s boundary: writing inside a worktree is
        // fully reversible, and a shell is the first thing that is not.
        let edit = build_args("pi", ToolLevel::Edit, None, None, &[], &[], None);
        let i = edit
            .iter()
            .position(|a| a == "--tools")
            .expect("an allowlist");
        assert!(edit[i + 1].contains("write"), "{}", edit[i + 1]);
        assert!(!edit[i + 1].contains("bash"), "{}", edit[i + 1]);

        // Not an enumeration of everything: passing no flag is what keeps a
        // tool the runner adds in a later version from being silently denied.
        let shell = build_args("pi", ToolLevel::Shell, None, None, &[], &[], None);
        assert!(!shell.contains(&"--tools".to_string()), "{shell:?}");
    }

    /// omp's `task` and `hub` are how it spawns subagents, which `32 harness
    /// capability floor` left open as its own question. It is not one: they are
    /// tools, so the answer is a level somebody chose.
    #[test]
    fn omps_subagent_tools_are_absent_below_shell_and_present_at_it() {
        for level in [ToolLevel::Read, ToolLevel::Edit] {
            let names = tool_allowlist("omp", level).expect("a list below shell");
            assert!(!names.contains(&"task"), "{level:?}: {names:?}");
            assert!(!names.contains(&"hub"), "{level:?}: {names:?}");
        }
        assert_eq!(tool_allowlist("omp", ToolLevel::Shell), None);
    }

    /// A runner farseer has not probed gets an empty list rather than a guess,
    /// and the layer above refuses before it ever gets that far.
    #[test]
    fn an_unprobed_runner_is_not_given_a_made_up_tool_list() {
        assert_eq!(tool_allowlist("goose-acp", ToolLevel::Read), Some(vec![]));
        assert!(!takes_tool_allowlist("goose-acp"));
        assert!(takes_tool_allowlist("pi") && takes_tool_allowlist("omp"));
    }

    /// omp runs a subagent as a background job and ends the foreground loop
    /// while it is still running. Ending the worker there would cut the run off
    /// before its own subagent answered.
    #[test]
    fn a_loop_that_says_it_is_not_terminal_does_not_end_the_run() {
        let waiting = r#"{"type":"agent_end","isTerminal":false,"messages":[{"role":"assistant","usage":{"totalTokens":900,"cost":{"total":0.004}},"stopReason":"toolUse"}]}"#;
        let signals = parse_line(waiting).unwrap();
        let [
            RunnerSignal::Progress { payload, .. },
            RunnerSignal::LegSpend {
                cost_usd_micros,
                tokens,
            },
        ] = &signals[..]
        else {
            panic!("spending, not an ending: {signals:?}")
        };
        // The leg still spent money, and the terminal `agent_end` will carry
        // only its own messages - so twice: once for the operator reading the
        // record, and once for the report, which used to lose it entirely.
        assert_eq!(payload["tokens"], 900);
        assert_eq!(payload["cost_usd_micros"], 4000);
        assert_eq!((*cost_usd_micros, *tokens), (Some(4000), Some(900)));

        let done = r#"{"type":"agent_end","isTerminal":true,"messages":[{"role":"assistant","usage":{"totalTokens":100,"cost":{"total":0.001}},"stopReason":"stop"}]}"#;
        assert!(matches!(
            parse_line(done).unwrap().as_slice(),
            [RunnerSignal::Finished(_)]
        ));
    }

    /// pi sends no `isTerminal` and has no background jobs to be waiting on, so
    /// absent must not be read as "not finished" - that would hang every pi run.
    #[test]
    fn a_runner_that_never_mentions_terminality_still_finishes() {
        let line = r#"{"type":"agent_end","messages":[{"role":"assistant","usage":{"totalTokens":10,"cost":{"total":0}},"stopReason":"stop"}]}"#;
        assert!(matches!(
            parse_line(line).unwrap().as_slice(),
            [RunnerSignal::Finished(_)]
        ));
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

    /// `31 manager delegation reach`: a manager that does not know its roster
    /// cannot say it has one, and a manager asked to delegate with no way to do
    /// so improvises. The prompt is how it finds out.
    #[test]
    fn a_manager_is_told_who_it_is_when_the_caller_says_so() {
        assert!(
            !build_args("pi", ToolLevel::Shell, None, None, &[], &[], None)
                .contains(&"--append-system-prompt".to_string())
        );

        let args = build_args(
            "pi",
            ToolLevel::Shell,
            None,
            None,
            &[],
            &[],
            Some("You are the manager for cell zero."),
        );
        let at = args
            .iter()
            .position(|a| a == "--append-system-prompt")
            .expect("passed");
        assert_eq!(args[at + 1], "You are the manager for cell zero.");

        // Whitespace is not an identity. An empty prompt sends no flag rather
        // than an empty one, which pi would take literally.
        assert!(
            !build_args("pi", ToolLevel::Shell, None, None, &[], &[], Some("  "))
                .contains(&"--append-system-prompt".to_string())
        );
    }

    /// A cell gets the skills it declared and nothing the machine happens to
    /// have installed - the denial is what makes the declaration mean anything.
    #[test]
    fn a_cell_gets_its_own_skills_and_never_the_machines() {
        let bare = build_args("pi", ToolLevel::Shell, None, None, &[], &[], None);
        assert!(bare.contains(&"--no-skills".to_string()));
        assert!(!bare.contains(&"--skill".to_string()));

        let declared = build_args(
            "pi",
            ToolLevel::Shell,
            None,
            None,
            &[std::path::PathBuf::from("/repo/skills/echo")],
            &[],
            None,
        );
        let flat = declared.join(" ");
        assert!(flat.contains("--no-skills"), "{flat}");
        assert!(flat.contains("--skill /repo/skills/echo"), "{flat}");
    }

    /// The tool that waits for a person is never on the argv of an unattended
    /// run, whatever else is. Asserted separately from the model pinning
    /// because it is not an option - it is the thing that stops a run hanging.
    #[test]
    fn an_unattended_run_can_never_reach_the_tool_that_waits_for_a_human() {
        for args in [
            build_args("pi", ToolLevel::Shell, None, None, &[], &[], None),
            build_args(
                "pi",
                ToolLevel::Shell,
                Some("m"),
                Some("low"),
                &[],
                &[],
                None,
            ),
        ] {
            let flat = args.join(" ");
            assert!(flat.contains("--exclude-tools ask_question"), "{flat}");
        }
    }

    #[test]
    fn the_operator_pins_the_model_and_an_unpinned_one_stays_pis_own() {
        assert_eq!(
            build_args("pi", ToolLevel::Shell, None, None, &[], &[], None),
            [
                "--mode",
                "rpc",
                "--exclude-tools",
                "ask_question",
                "--no-skills",
                "--no-extensions"
            ]
        );
        assert_eq!(
            build_args(
                "pi",
                ToolLevel::Shell,
                Some("gpt-5.6-luna"),
                Some("low"),
                &[],
                &[],
                None
            ),
            [
                "--mode",
                "rpc",
                "--exclude-tools",
                "ask_question",
                "--model",
                "gpt-5.6-luna",
                "--thinking",
                "low",
                "--no-skills",
                "--no-extensions"
            ]
        );
    }

    /// omp shares this adapter and does **not** share this command line.
    ///
    /// Found the hard way on 2026-08-28: omp had never actually launched
    /// through farseer, because `--exclude-tools` is pi's flag and omp exits
    /// with `unknown flag` before it reads anything else. The protocol matching
    /// verbatim is what made it look safe to share the launch too.
    #[test]
    fn omp_is_not_launched_with_pis_flags() {
        let omp = build_args("omp", ToolLevel::Shell, None, None, &[], &[], None).join(" ");
        assert!(!omp.contains("--exclude-tools"), "{omp}");
        assert!(omp.contains("--mode rpc"), "{omp}");
        assert!(omp.contains("--no-skills"), "{omp}");

        let pi = build_args("pi", ToolLevel::Shell, None, None, &[], &[], None).join(" ");
        assert!(pi.contains("--exclude-tools ask_question"), "{pi}");
    }

    /// A skill omp cannot be given must not be quietly put on its argv anyway.
    /// The refusal lives in the API, above this - see `runner_loads_skills` -
    /// and this pins the half that would otherwise fail silently.
    #[test]
    fn a_skill_path_is_only_ever_passed_to_the_runner_that_takes_one() {
        let skill = [std::path::PathBuf::from("/repo/skills/farseer-echo")];
        assert!(
            build_args("pi", ToolLevel::Shell, None, None, &skill, &[], None)
                .contains(&"--skill".to_string())
        );
        assert!(
            !build_args("omp", ToolLevel::Shell, None, None, &skill, &[], None)
                .contains(&"--skill".to_string())
        );
    }
}
