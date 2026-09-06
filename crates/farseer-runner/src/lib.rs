//! Runner adapters: farseer's worker control channel implementations, per `20 worker control channel`.
//!
//! `20 worker control channel` and `10 runner inventory` chose an ACP path plus
//! a menu of native runners because "a runner interface with one implementation
//! is not an interface, it is a wrapper". The verified set now includes Claude
//! Code, Codex exec and app-server, cursor-agent, Goose, pi, omp, agy and ACP
//! faces. Each adapter ships only mappings backed by a captured payload.
//!
//! What ships here: `PATHEXT`-safe executable resolution ([`resolve`]), each
//! runner's invocation-building and stream-json mapping, and a
//! Job-Object-supervised child process with piped, line-buffered I/O
//! ([`spawn`], Windows only). Together these can run and reap one process
//! and read its lines - the primitive a runner needs. The ACP runner
//! `20 worker control channel` chose as the default path is now here
//! ([`acp`]), built against a captured `goose acp` transcript rather than
//! against the spec alone, with [`acp_drive`] holding the conversation it
//! needs - every other runner here is one-shot, so `drive` reads stdout and
//! never writes, and ACP's handshake had to grow its own counterpart. Only Claude Code has a steering
//! path - Codex, cursor-agent and goose all restart into a new process on
//! resume/continue rather than continuing a live one, per `10 runner inventory` (Codex,
//! cursor-agent) and this crate's own goose probe.

pub mod acp;
pub mod agy;
pub mod claude_code;
pub mod codex;
pub mod codex_app_server;
pub mod cursor_agent;
pub mod goose;
pub mod invocation;
#[cfg(windows)]
pub mod jsonrpc;
pub mod omp_usage;
pub mod pi;
pub mod resolve;

#[cfg(windows)]
pub mod acp_drive;
#[cfg(windows)]
pub mod drive;
#[cfg(windows)]
pub mod spawn;
#[cfg(windows)]
pub mod workspace;
