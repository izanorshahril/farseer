//! Runner adapters: farseer's worker control channel implementations, per `20`.
//!
//! `20` and `10` chose two for v1 - an ACP runner as the default path, and one
//! native runner because "a runner interface with one implementation is not
//! an interface, it is a wrapper". This crate starts with the native one,
//! **Claude Code**, since `10` measured it as the strongest on the contract
//! `05` wrote: activity and progress both pass, quota and cost arrive in band,
//! and it is the operator's primary harness.
//!
//! What ships here: `PATHEXT`-safe executable resolution
//! ([`resolve`]), the line-by-line mapping from Claude Code's stream-json
//! onto the contract ([`claude_code`]), and a Job-Object-supervised child
//! process with piped, line-buffered I/O ([`spawn`], Windows only). Together
//! these can run and reap one process and read its lines - the primitive a
//! runner needs, not yet a runner. **Not yet built**: anything that actually
//! drives one - the manager loop that constructs a `claude` invocation from a
//! `WorkerContract`, feeds `spawn::SupervisedProcess::read_line` through
//! `claude_code::parse_line` into the record, and writes steer instructions
//! back through `write_line` - plus the ACP runner `20` chose as the default
//! path.

pub mod claude_code;
pub mod resolve;

#[cfg(windows)]
pub mod spawn;
