//! The half of the ACP runner that talks back.
//!
//! Every other runner in this crate is **one-shot**: spawn it, read its lines,
//! it exits. [`crate::drive::drive`] is shaped for exactly that - it reads
//! stdout and never writes. ACP is a **conversation**, so it needs a
//! counterpart that owns request ids, the session id, and the order the
//! handshake has to happen in.
//!
//! ```text
//! initialize      -> capabilities, and farseer's refusal of fs/terminal
//! session/new     -> sessionId, and the modes this agent will accept
//! session/set_mode-> a mode that does not ask, per `12 autonomy and deny list`
//! session/prompt  -> the goal; from here `drive_turn` reads the answer
//! ```
//!
//! # Every line is activity, including during the handshake
//!
//! `05 run state model` says any bytes are evidence of life, and that must keep
//! holding while farseer is waiting for a response to a request it sent. So the
//! handshake takes the **same sink** [`crate::drive::drive`] takes, and hands it
//! every line that is not the response being waited for. A `usage_update` that
//! arrives before the first prompt - `goose acp` sends one - reaches the record
//! through that path rather than being swallowed by the handshake.
//!
//! # What this does not solve
//!
//! An agent that never answers `initialize` is a hang, and the watchdog cannot
//! see it yet: the manager's supervision loop has not started. Bounded only by
//! the process being killable. Recorded here rather than papered over, because
//! `28 operator surface` has already paid twice for a silent live process.

use std::path::Path;

use crate::acp::{self, SessionOpened};
use crate::claude_code::{ParseError, RunnerSignal};
use crate::spawn::{StdinMode, SupervisedProcess};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum AcpError {
    #[error("could not start the ACP agent: {0}")]
    Spawn(#[from] crate::spawn::SpawnError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The agent closed stdout before answering. Usually a misconfigured agent
    /// writing its complaint to stderr, which this crate discards.
    #[error("the ACP agent stopped before answering {method}")]
    Closed { method: &'static str },
    /// A JSON-RPC error response to something farseer asked for.
    #[error("the ACP agent refused {method}: {message}")]
    Refused {
        method: &'static str,
        message: String,
    },
    #[error("the ACP agent answered session/new without a session id")]
    NoSession,
}

/// Walk the handshake on a process somebody else spawned.
///
/// Split out of [`AcpSession::open`] because the manager owns its own
/// supervised process - job object, watchdog clock, cancel token - and must not
/// have a second one wrapped around it. Leaves the session ready for
/// [`prompt_on`].
pub fn handshake<F>(
    process: &mut SupervisedProcess,
    cwd: &Path,
    mode: Option<&str>,
    next_id: &mut i64,
    on_line: &mut F,
) -> Result<SessionOpened, AcpError>
where
    F: FnMut(Result<Vec<RunnerSignal>, ParseError>),
{
    let take = |next_id: &mut i64| {
        let id = *next_id;
        *next_id += 1;
        id
    };

    let id = take(next_id);
    request(
        process,
        &acp::initialize_frame(id),
        id,
        "initialize",
        on_line,
    )?;

    let id = take(next_id);
    let answer = request(
        process,
        &acp::session_new_frame(id, &cwd.to_string_lossy()),
        id,
        "session/new",
        on_line,
    )?;
    let opened = acp::session_opened(&answer.to_string()).ok_or(AcpError::NoSession)?;

    // Only if the agent said it would take it. `opencode acp` advertises **no
    // modes at all**, and asking it to set one is a JSON-RPC error that kills
    // the handshake before the goal is ever sent - so a `goose`-shaped
    // assumption would have failed every `opencode-acp` run, which is what
    // running the second agent is for.
    //
    // The consequence is recorded rather than hidden: an agent with no modes
    // runs in whatever it opened in, and `29 harness protocol` notes that ACP
    // does not standardise the names, so farseer cannot ask for the equivalent
    // by guessing at a synonym.
    if let Some(mode) = mode.filter(|mode| opened.accepts_mode(mode)) {
        let id = take(next_id);
        let frame = acp::set_mode_frame(id, &opened.session_id, mode);
        request(process, &frame, id, "session/set_mode", on_line)?;
    }
    Ok(opened)
}

/// Send one turn. The answer arrives on stdout.
pub fn prompt_on(
    process: &SupervisedProcess,
    session_id: &str,
    id: i64,
    text: &str,
) -> std::io::Result<()> {
    process.write_line(&acp::prompt_frame(id, session_id, text))
}

/// Whether a parsed line ended the turn.
///
/// The read loop of a conversational runner needs this and a one-shot runner's
/// does not, which is the whole distinction: an ACP agent **does not exit when
/// the turn ends**, so end of stream is the wrong thing to wait for.
pub fn ends_turn(parsed: &Result<Vec<RunnerSignal>, ParseError>) -> bool {
    parsed.as_ref().is_ok_and(|signals| {
        signals
            .iter()
            .any(|signal| matches!(signal, RunnerSignal::Finished(_)))
    })
}

/// Write one request and read until its answer, forwarding everything else.
fn request<F>(
    process: &mut SupervisedProcess,
    frame: &str,
    id: i64,
    method: &'static str,
    on_line: &mut F,
) -> Result<Value, AcpError>
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
            // Somebody else's line: activity, and possibly a signal. The
            // handshake is not a quiet period.
            on_line(acp::parse_line(&line));
            continue;
        }
        let value = parsed.expect("an id was read out of it");
        if let Some(error) = value.get("error") {
            return Err(AcpError::Refused {
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
    Err(AcpError::Closed { method })
}

/// A live ACP conversation with one agent, in one workspace.
pub struct AcpSession {
    process: SupervisedProcess,
    opened: SessionOpened,
    next_id: i64,
}

impl AcpSession {
    /// Spawn the agent and walk the handshake, leaving the session ready for a
    /// prompt.
    ///
    /// `mode` is the ACP mode to request, and passing `None` means **accepting
    /// whatever the agent opened in** - which for `goose acp` is `auto` and for
    /// another agent may well be one that stops to ask a question nobody will
    /// answer. Callers running unattended should name a mode.
    pub fn open<F>(
        exe: &Path,
        args: &[String],
        cwd: &Path,
        mode: Option<&str>,
        on_line: &mut F,
    ) -> Result<Self, AcpError>
    where
        F: FnMut(Result<Vec<RunnerSignal>, ParseError>),
    {
        // Live, because the whole point is that farseer writes to it. The
        // `StdinMode::Closed` rule `28 operator surface` learned the hard way is
        // about one-shot runners; this is the other case it exists to
        // distinguish.
        let mut process = SupervisedProcess::spawn(exe, args, cwd, StdinMode::Live)?;
        let mut next_id = 1;
        let opened = handshake(&mut process, cwd, mode, &mut next_id, on_line)?;
        let session = Self {
            process,
            opened,
            next_id,
        };

        Ok(session)
    }

    /// What the agent said about the session it opened - its id, the mode it
    /// chose, and the modes it will accept.
    pub fn opened(&self) -> &SessionOpened {
        &self.opened
    }

    /// Send a turn and return immediately.
    ///
    /// The answer arrives on stdout, and [`Self::drive_turn`] reads it. A steer is
    /// this same call again: `20 worker control channel` made steering
    /// turn-boundary granular, and ACP has no mid-turn channel to disagree with.
    pub fn prompt(&mut self, text: &str) -> std::io::Result<()> {
        let id = self.take_id();
        prompt_on(&self.process, &self.opened.session_id, id, text)
    }

    /// `05 run state model`'s `cancel`, as a notification the agent owes no
    /// reply to. The process stays up; killing it is the caller's separate
    /// decision through [`SupervisedProcess::cancel_token`].
    pub fn cancel(&self) -> std::io::Result<()> {
        self.process
            .write_line(&acp::cancel_frame(&self.opened.session_id))
    }

    /// Read until this turn ends, forwarding every line to `on_line`.
    ///
    /// **[`crate::drive::drive`] cannot do this job**, and finding that out
    /// cost a hung test: `drive` drains until end of stream, and an ACP agent
    /// **does not exit when a turn ends** - the session stays open for the next
    /// prompt, which is the entire point of a session. Waiting for EOF is
    /// waiting forever. Same family as `28 operator surface`'s stdin bug:
    /// machinery correct for a one-shot runner, silently wrong for a
    /// conversational one.
    ///
    /// Returns once a terminal signal has been forwarded, leaving the session
    /// usable for another [`Self::prompt`]. `Ok(())` on a closed stream too -
    /// an agent that exits mid-turn is the caller's `Finished`-shaped problem,
    /// not an IO error.
    pub fn drive_turn<F>(&mut self, on_line: &mut F) -> std::io::Result<()>
    where
        F: FnMut(Result<Vec<RunnerSignal>, ParseError>),
    {
        while let Some(line) = self.process.read_line()? {
            let parsed = acp::parse_line(&line);
            let ended = ends_turn(&parsed);
            on_line(parsed);
            if ended {
                return Ok(());
            }
        }
        Ok(())
    }

    /// The underlying process, so the caller can drive it and cancel it with
    /// the same machinery every other runner uses.
    pub fn process_mut(&mut self) -> &mut SupervisedProcess {
        &mut self.process
    }

    pub fn into_process(self) -> SupervisedProcess {
        self.process
    }

    fn take_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::resolve;
    use farseer_core::run::Outcome;

    /// Live, against the `goose acp` on this machine. Ignored by default for
    /// the same reason every other live test here is: it spends the operator's
    /// subscription and needs a configured provider.
    ///
    /// Run with: `cargo test -p farseer-runner acp_drive -- --ignored --nocapture`
    #[test]
    #[ignore = "spends a real subscription"]
    fn a_real_acp_agent_opens_a_session_answers_a_turn_and_names_its_context_window() {
        let exe = resolve("goose").expect("goose is on PATH");
        let cwd = std::env::current_dir().unwrap();
        let mut signals = Vec::new();
        let mut sink = |result: Result<Vec<RunnerSignal>, ParseError>| {
            if let Ok(parsed) = result {
                signals.extend(parsed);
            }
        };

        let mut session = AcpSession::open(
            &exe,
            &["acp".to_string()],
            &cwd,
            // The mode that does not ask. Nobody is watching a prompt.
            Some("auto"),
            &mut sink,
        )
        .expect("the handshake completes");

        assert!(!session.opened().session_id.is_empty());
        assert!(
            session.opened().available_modes.iter().any(|m| m == "auto"),
            "an agent with no non-asking mode cannot be run unattended"
        );

        session.prompt("Say hello in one short sentence.").unwrap();
        session.drive_turn(&mut sink).unwrap();

        let window = signals.iter().find_map(|signal| match signal {
            RunnerSignal::Usage(usage) => usage.size,
            _ => None,
        });
        assert!(
            window.is_some_and(|size| size > 0),
            "the denominator is the whole reason this runner exists"
        );
        assert!(
            signals
                .iter()
                .any(|signal| matches!(signal, RunnerSignal::Output(_))),
            "a turn that answers must reach the record as text"
        );
        let finished = signals.iter().find_map(|signal| match signal {
            RunnerSignal::Finished(finished) => Some(finished),
            _ => None,
        });
        assert_eq!(finished.map(|f| f.outcome), Some(Outcome::Ok));

        // The session is still open here, which is the thing that makes an ACP
        // agent unlike every other runner in this crate.
        session.into_process().kill();
    }
}
