//! Runner adapters: farseer's worker control channel implementations, per `20`.
//!
//! `20` and `10` chose two for v1 - an ACP runner as the default path, and a
//! menu of native runners because "a runner interface with one
//! implementation is not an interface, it is a wrapper". Two native runners
//! ship here: **Claude Code** ([`claude_code`], [`invocation`]), the
//! strongest on `05`'s contract per `10` - quota and cost both arrive in
//! band - and **Codex** ([`codex`]), `20`'s second choice, whose fine-grained
//! `item.*` progress mapping stays shallow because no ticket captured a
//! literal payload for it; only the verified terminal shape and its `usage`
//! fields are mapped.
//!
//! Three native runners ship here: **Claude Code** ([`claude_code`],
//! [`invocation`]), **Codex** ([`codex`]), and **cursor-agent**
//! ([`cursor_agent`]) - `10`'s three evaluated CLIs, none of whose progress
//! mapping past its verified terminal shape is guessed at beyond what a
//! literal payload backs.
//!
//! What ships here: `PATHEXT`-safe executable resolution ([`resolve`]), each
//! runner's invocation-building and stream-json mapping, and a
//! Job-Object-supervised child process with piped, line-buffered I/O
//! ([`spawn`], Windows only). Together these can run and reap one process
//! and read its lines - the primitive a runner needs. **Not yet built**: the
//! ACP runner `20` chose as the default path. Only Claude Code has a steering
//! path - Codex and cursor-agent both restart into a new process on
//! resume/continue, per `10`.

pub mod claude_code;
pub mod codex;
pub mod cursor_agent;
pub mod invocation;
pub mod resolve;

#[cfg(windows)]
pub mod drive;
#[cfg(windows)]
pub mod spawn;
#[cfg(windows)]
pub mod workspace;
