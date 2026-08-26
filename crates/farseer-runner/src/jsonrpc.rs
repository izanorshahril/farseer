//! One request-and-wait loop, for the two protocols that need one.
//!
//! ACP and the Codex app-server are both JSON-RPC 2.0 over stdio, and both need
//! the same thing farseer's one-shot runners never did: **write a request, then
//! read past everything else until its answer arrives**.
//!
//! Extracted when the second one appeared rather than the first, per
//! `08 generalization test`'s standard - a shape with one implementation is a
//! wrapper, and this one now has two.
//!
//! Everything that is not the awaited answer goes to the caller's sink.
//! `05 run state model` says any bytes are activity, and that must keep holding
//! while farseer is blocked waiting on a reply it asked for: `goose acp` sends a
//! `usage_update` before the first prompt and `codex app-server` reports the
//! operator's own hooks starting, both during a handshake.

use crate::claude_code::{ParseError, RunnerSignal};
use crate::spawn::SupervisedProcess;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("could not start the agent: {0}")]
    Spawn(#[from] crate::spawn::SpawnError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The agent closed stdout before answering. Usually a misconfigured agent
    /// writing its complaint to stderr, which this crate discards.
    #[error("the agent stopped before answering {method}")]
    Closed { method: &'static str },
    /// A JSON-RPC error response to something farseer asked for.
    #[error("the agent refused {method}: {message}")]
    Refused {
        method: &'static str,
        message: String,
    },
    /// An answer arrived and did not carry the one field it exists to carry.
    #[error("the agent answered {method} without a {field}")]
    Missing {
        method: &'static str,
        field: &'static str,
    },
}

/// Write one request and read until its answer, forwarding everything else to
/// `on_line` as the parser for that protocol sees it.
pub fn request<F>(
    process: &mut SupervisedProcess,
    frame: &str,
    id: i64,
    method: &'static str,
    parse: fn(&str) -> Result<Vec<RunnerSignal>, ParseError>,
    on_line: &mut F,
) -> Result<Value, RpcError>
where
    F: FnMut(Result<Vec<RunnerSignal>, ParseError>),
{
    process.write_line(frame)?;
    while let Some(line) = process.read_line()? {
        let parsed: Option<Value> = serde_json::from_str(&line).ok();
        let is_answer = parsed
            .as_ref()
            .and_then(|v| v.get("id"))
            .and_then(Value::as_i64)
            == Some(id);
        if !is_answer {
            // Somebody else's line: activity, and possibly a signal. A handshake
            // is not a quiet period.
            on_line(parse(&line));
            continue;
        }
        let value = parsed.expect("an id was read out of it");
        if let Some(error) = value.get("error") {
            return Err(RpcError::Refused {
                method,
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("no message")
                    .to_string(),
            });
        }
        return Ok(value);
    }
    Err(RpcError::Closed { method })
}

/// Hand out request ids nobody else has used.
#[derive(Debug, Default)]
pub struct Ids(i64);

impl Ids {
    pub fn starting_at(next: i64) -> Self {
        Self(next)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i64 {
        self.0 += 1;
        self.0
    }

    /// The id the next call would hand out, for a caller that shares the
    /// counter with something else - a steer, in practice.
    pub fn peek(&self) -> i64 {
        self.0 + 1
    }
}
