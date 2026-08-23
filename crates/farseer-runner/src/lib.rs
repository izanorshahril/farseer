//! Runner adapters: farseer's worker control channel implementations, per `20`.
//!
//! `20` and `10` chose two for v1 - an ACP runner as the default path, and one
//! native runner because "a runner interface with one implementation is not
//! an interface, it is a wrapper". This crate starts with the native one,
//! **Claude Code**, since `10` measured it as the strongest on the contract
//! `05` wrote: activity and progress both pass, quota and cost arrive in band,
//! and it is the operator's primary harness.
//!
//! What ships here: the line-by-line mapping from Claude Code's stream-json
//! onto the contract, and `PATHEXT`-safe executable resolution. Both are pure
//! and unit-tested. **Not yet built**: the Job Object process spawn itself and
//! the ACP runner - `jobspike` proved the reap mechanism works, but wiring a
//! live child process, a manager loop to drive it, and the record writes on
//! the other end of [`claude_code::parse_line`] is a separate, larger seam.

pub mod claude_code;
pub mod resolve;
