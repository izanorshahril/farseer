//! Farseer as an ACP **agent**, so an editor can drive it.
//!
//! `16 local api surface` section 1 settled the ordering: the substrate is the
//! bespoke HTTP plus SSE API, and an ACP server adapter sits **on top of it as a
//! first-class feature**. Roughly a fifth of farseer's surface maps to an ACP
//! verb, so ACP is a face rather than a transport - this file is that face, and
//! it reaches the runtime through `/v1` like any other client rather than
//! reaching into it.
//!
//! **Farseer says what it is.** `06 cell transport` section 3 allowed this
//! inbound path and refused to let it flatten silently: a caller that believes
//! it is driving a single agent when it is driving a fleet has quietly wrong
//! timeout, cancellation and progress assumptions. So the `initialize` answer
//! declares an orchestrator, names the cell a session will address, and lets the
//! caller decide whether to proceed.
//!
//! **A session is a cell.** ACP models one conversation; farseer's unit of
//! conversation is a cell's manager, which is exactly `15 manager
//! conversation`'s shape. `session/prompt` therefore instructs a cell and
//! reports that run's progress, and a prompt that the manager answers by
//! delegating shows up as tool calls - which is what it is.
//!
//! **No `session/load`.** ACP's replay is a session the agent reconstructs;
//! farseer's is a cursor over the record, and the two are not the same promise.
//! Rather than reconstruct a conversation from events and call it the session
//! the editor left, this declines the capability, which is what capability
//! negotiation is for.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde_json::{Value, json};

/// The version the client adapter negotiates, and the only one this face
/// answers. `29 harness protocol`: a version farseer has not captured output
/// from is a version whose mappings are guesses.
const PROTOCOL_VERSION: i64 = 1;

/// Which cell a session talks to when the caller names none.
/// `01 cell primitive` made it the default address the operator talks to.
const DEFAULT_CELL: &str = "zero";

/// How often the run's events are read while a prompt is in flight.
///
/// A poll rather than the SSE stream farseer also serves: the stream is the
/// better shape and this loop needs one thing the stream does not give it - the
/// answer to "has this run finished" *and* a cursor, in the same read, with no
/// second connection. Local, and 200ms is well under the latency of anything an
/// agent does.
const POLL_MS: u64 = 200;

pub struct Runtime {
    pub base: String,
    pub token: String,
}

impl Runtime {
    /// Where the CLI already looks, per `16 local api surface`.
    pub fn attach() -> Result<Self> {
        let path = farseer_api::runtime_file_path();
        let text = std::fs::read_to_string(&path).with_context(|| {
            format!(
                "no runtime at {} - start one with `farseer serve`",
                path.display()
            )
        })?;
        let runtime: Value = serde_json::from_str(&text).context("reading the runtime file")?;
        let port = runtime["port"]
            .as_u64()
            .context("runtime file has no port")?;
        let token = runtime["token"]
            .as_str()
            .context("runtime file has no token")?
            .to_string();
        Ok(Self {
            base: format!("http://127.0.0.1:{port}"),
            token,
        })
    }
}

/// One editor's connection, over stdio.
///
/// Line-delimited JSON-RPC, which is what every ACP agent farseer drives as a
/// client speaks, so the two directions share a wire format even though they
/// share no code.
///
/// **A prompt runs as its own task and the read loop keeps reading.** The first
/// shape of this awaited the turn inline, which reads fine and is wrong for the
/// one message that matters most: `session/cancel` arrives *during* a turn, and
/// a loop blocked on that turn would only see the cancellation after the thing
/// it cancels had finished. That is the hang this repository keeps cataloguing,
/// rebuilt in the one place farseer promised not to.
pub async fn serve_stdio(runtime: Runtime) -> Result<()> {
    let client = reqwest::Client::new();
    let runtime = Arc::new(runtime);
    // session id -> the cell it addresses.
    let sessions: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    // session id -> the run currently in flight for it, if any.
    let active: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

    // stdin is blocking, so it gets its own thread and hands lines over. On the
    // async side nothing blocks, which is what lets a cancel overtake a turn.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });

    while let Some(line) = rx.recv().await {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(frame) = serde_json::from_str::<Value>(&line) else {
            // A frame farseer cannot parse is answered rather than dropped: an
            // editor waiting on a reply that never comes is the hang this whole
            // repository is about.
            emit(&reply_error(
                Value::Null,
                -32700,
                "that was not JSON farseer could read",
            ));
            continue;
        };
        let id = frame.get("id").cloned().unwrap_or(Value::Null);
        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = frame.get("params").cloned().unwrap_or(Value::Null);

        match method.as_str() {
            "initialize" => emit(&reply(id, initialize_result())),
            "session/new" => {
                let session_id = format!("farseer-{}", uuid_ish());
                let cell = params
                    .pointer("/_meta/farseer/cell")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_CELL)
                    .to_string();
                sessions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(session_id.clone(), cell.clone());
                emit(&reply(
                    id,
                    json!({
                        "sessionId": session_id,
                        // Said again per session, because an editor that
                        // skipped the initialize detail still needs to know
                        // which cell its words reach.
                        "_meta": { "farseer": { "cell": cell } },
                    }),
                ));
            }
            "session/prompt" => {
                let session = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let cell = sessions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&session)
                    .cloned();
                let Some(cell) = cell else {
                    emit(&reply_error(id, -32602, "no such session"));
                    continue;
                };
                let text = prompt_text(&params);
                if text.trim().is_empty() {
                    emit(&reply_error(id, -32602, "the prompt has no text to act on"));
                    continue;
                }
                let (client, runtime, active) =
                    (client.clone(), Arc::clone(&runtime), Arc::clone(&active));
                tokio::spawn(async move {
                    let answer = run_prompt(&client, &runtime, &cell, &session, &text, &active)
                        .await
                        .map(|stop| reply(id.clone(), json!({ "stopReason": stop })))
                        .unwrap_or_else(|error| reply_error(id, -32000, &error.to_string()));
                    active
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&session);
                    emit(&answer);
                });
            }
            "session/cancel" => {
                // A notification: no id, no reply. `07 attach semantics` keeps
                // cancel a separate verb from intervention, and this is the
                // editor's version of the same one - it ends the run, and the
                // job object ends the tree under it.
                let session = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let run_id = active
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(session)
                    .cloned();
                if let Some(run_id) = run_id {
                    let (client, runtime) = (client.clone(), Arc::clone(&runtime));
                    tokio::spawn(async move {
                        let _ = post(
                            &client,
                            &runtime,
                            &format!("/v1/runs/{run_id}/cancel"),
                            json!({}),
                        )
                        .await;
                    });
                }
            }
            "" => emit(&reply_error(id, -32600, "no method")),
            other => emit(&reply_error(
                id,
                -32601,
                &format!("farseer's ACP face has no `{other}`"),
            )),
        }
    }
    Ok(())
}

/// What farseer declares at capability negotiation.
///
/// The `_meta.farseer` block is `06 cell transport` section 3 made concrete: an
/// orchestrator, not a single agent, and the caller decides whether to proceed.
/// Standard ACP has no field for "I am a fleet", so it goes where the protocol
/// puts things it does not model - and it is a declaration rather than a hint,
/// because allowing the path silently flattened is worse than refusing it.
pub fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "agentCapabilities": {
            // Declined, and both for the same reason as the client side's
            // refusals: farseer's answer is a run in a workspace, not a
            // conversation it can rebuild.
            "loadSession": false,
            "promptCapabilities": {
                "image": false,
                "audio": false,
                "embeddedContext": false,
            },
        },
        "authMethods": [],
        "_meta": {
            "farseer": {
                "kind": "orchestrator",
                "notice": "This is an orchestrator, not a single agent. A prompt starts a run in a cell whose manager may delegate to workers and to other cells, so a turn can outlive a usual agent turn and cancellation ends a tree rather than a process.",
                "defaultCell": DEFAULT_CELL,
            }
        }
    })
}

/// Instruct the cell, then report the run until it ends.
async fn run_prompt(
    client: &reqwest::Client,
    runtime: &Runtime,
    cell: &str,
    session: &str,
    text: &str,
    active: &Mutex<HashMap<String, String>>,
) -> Result<&'static str> {
    let accepted = post(
        client,
        runtime,
        &format!("/v1/cells/{cell}/instruct"),
        json!({ "goal": text }),
    )
    .await?;
    let run_id = accepted["run_id"]
        .as_str()
        .context("farseer accepted the instruction without naming a run")?
        .to_string();
    // Recorded before the first update, so a cancel arriving on the very next
    // line has something to name.
    active
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session.to_string(), run_id.clone());
    emit(&notify(
        "session/update",
        json!({
            "sessionId": session,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": format!("started run {}\n", &run_id[..8.min(run_id.len())]) },
            },
        }),
    ));

    let mut since = 0i64;
    loop {
        let events: Value = get(
            client,
            runtime,
            &format!("/v1/events?run={run_id}&since={since}&limit=200"),
        )
        .await?;
        for event in events.as_array().into_iter().flatten() {
            since = event["seq"].as_i64().unwrap_or(since).max(since);
            if let Some(update) = as_update(event) {
                emit(&notify(
                    "session/update",
                    json!({ "sessionId": session, "update": update }),
                ));
            }
            // **A turn ends when the manager answers, not when the run ends.**
            // `10 runner inventory` observed a manager on live stdin emitting
            // its own terminal result per turn and staying alive for the next
            // one, which is why `manager_answered` exists at all - so a loop
            // waiting for `run_finished` waits for the operator to close a
            // conversation that ACP models as a single turn, and never
            // answers. Observed here doing exactly that.
            if event["kind"] == "manager_answered" {
                return Ok("end_turn");
            }
            if event["kind"] == "run_finished" {
                // `05 run state model`'s outcomes, in ACP's vocabulary. A
                // cancelled run is never reported as a refusal or an error:
                // somebody chose it.
                return Ok(match event["payload"]["outcome"].as_str() {
                    Some("cancelled") | Some("abandoned") => "cancelled",
                    Some("ok") => "end_turn",
                    _ => "refusal",
                });
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
    }
}

/// One record event as an ACP session update, or nothing.
///
/// Only what an editor can render. `05 run state model` split activity from
/// progress and the record holds progress, so this is a narrowing rather than a
/// translation - and a kind with no ACP shape is dropped rather than sent as
/// text nobody asked for.
fn as_update(event: &Value) -> Option<Value> {
    match event["kind"].as_str()? {
        "manager_answered" | "agent_message" => {
            let text = event["payload"]["text"].as_str().unwrap_or_default();
            (!text.is_empty()).then(|| {
                json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": text },
                })
            })
        }
        "tool_call_started" => Some(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": event["event_id"],
            "title": event["payload"]["name"].as_str().unwrap_or("a tool"),
            "status": "in_progress",
        })),
        "tool_result" => Some(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": event["event_id"],
            "status": "completed",
        })),
        _ => None,
    }
}

fn prompt_text(params: &Value) -> String {
    params
        .get("prompt")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

async fn post(
    client: &reqwest::Client,
    runtime: &Runtime,
    path: &str,
    body: Value,
) -> Result<Value> {
    let response = client
        .post(format!("{}{path}", runtime.base))
        .bearer_auth(&runtime.token)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("POST {path}: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("farseer refused {path}: {text}");
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn get(client: &reqwest::Client, runtime: &Runtime, path: &str) -> Result<Value> {
    let response = client
        .get(format!("{}{path}", runtime.base))
        .bearer_auth(&runtime.token)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("GET {path}: {e}"))?;
    Ok(response.json().await.unwrap_or(Value::Null))
}

fn reply(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn reply_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn notify(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

/// One frame per line, flushed. An editor reads line by line, so a frame that
/// sits in a buffer is a frame that has not been sent.
fn emit(frame: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{frame}");
    let _ = out.flush();
}

/// A session id that is unique per process and per call, without a uuid
/// dependency this crate does not otherwise need.
fn uuid_ish() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("{now:x}-{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `06 cell transport` section 3: allowed, and advertised during the
    /// handshake. The whole reason the path is allowed rather than forbidden is
    /// that forbidding it is unenforceable; the whole reason it is announced is
    /// that allowing it silently flattened is worse than either option.
    #[test]
    fn the_handshake_says_farseer_is_an_orchestrator_rather_than_an_agent() {
        let result = initialize_result();
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(result["_meta"]["farseer"]["kind"], "orchestrator");
        assert!(
            result["_meta"]["farseer"]["notice"]
                .as_str()
                .unwrap()
                .contains("not a single agent")
        );
        assert_eq!(
            result["agentCapabilities"]["loadSession"], false,
            "farseer's replay is a cursor over the record, not a session it can rebuild"
        );
    }

    #[test]
    fn a_prompt_is_read_from_its_text_blocks_and_nothing_else() {
        let params = json!({
            "prompt": [
                { "type": "text", "text": "first" },
                { "type": "image", "data": "..." },
                { "type": "text", "text": "second" }
            ]
        });
        assert_eq!(prompt_text(&params), "first\nsecond");
        assert_eq!(prompt_text(&json!({})), "");
    }

    /// A kind with no ACP shape is dropped rather than rendered as text an
    /// editor did not ask for. `05 run state model` already decided which kinds
    /// are progress; this only narrows further.
    #[test]
    fn only_the_events_an_editor_can_render_become_updates() {
        assert!(as_update(&json!({"kind": "run_queued"})).is_none());
        assert!(as_update(&json!({"kind": "manager_answered", "payload": {"text": ""}})).is_none());
        let answered =
            as_update(&json!({"kind": "manager_answered", "payload": {"text": "done"}})).unwrap();
        assert_eq!(answered["sessionUpdate"], "agent_message_chunk");
        assert_eq!(answered["content"]["text"], "done");

        let started = as_update(&json!({
            "kind": "tool_call_started", "event_id": "abc", "payload": {"name": "shell"}
        }))
        .unwrap();
        assert_eq!(started["status"], "in_progress");
        assert_eq!(started["title"], "shell");
    }
}
