# 01: Verified startup

**Parent:** [Command-center decision map](../map.md).

**What to build:** Make startup prove which local runtime and feature set the desktop client is connected to before the command center becomes interactive.

**Blocked by:** None; package approved.

**Status:** complete.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R01; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/01-runtime.md).

**Contract choices:** Preserve Rust, SQLite, cells, and the authenticated HTTP/SSE boundary.
Use an OS-held exclusive per-data-directory ownership lease with an authenticated startup feature handshake.
Keep unsupported features visible as unavailable rather than silently emulating them.

- [x] A fresh desktop launch discovers or starts one owner per data directory and verifies authenticated runtime identity, directory fingerprint, build provenance, API version, and enabled features.
- [x] A stale, malformed, mismatched, or unauthorized runtime record produces an actionable recovery state and never sends operator commands to an unknown process.
- [x] Missing optional capabilities disable only affected views/actions; missing required compatibility yields an actionable connection failure.
- [x] Two simultaneous launches converge on one user-space runtime without requiring elevation or a machine-wide service.
- [x] Wrong listener, child exit, and the 20-second startup deadline return distinct errors without a port-zero success; cleanup affects only the child this launch owns.

**Exclusions:** No cloud discovery, installer service, plugin ABI, or new transport.

**Test seam/demo:** A local fake runtime and desktop smoke test cover valid, stale, mismatched, and missing capability handshakes.

## Evidence

Implemented in `crates/farseer-api/src/security.rs`, `crates/farseer-api/src/lib.rs`, and `crates/farseer-shell/src/runtime.rs`.
`cargo test -p farseer-shell runtime::tests` passes all three handshake tests.
The shell rejects empty ports, empty tokens, mismatched data fingerprints, incompatible identity, wrong listeners, and missing required features, and leaves successful sidecars alive after UI exit.
The authenticated discovery identity now carries an optional owner process id, and a launcher kills its own child before attaching when the record belongs to another process; legacy records without that field remain attachable.
Startup timeout errors now retain the last observed cause, such as an unpublished runtime file, a previous-runtime identity, or an unauthenticated health response, instead of collapsing every failure into one opaque timeout.
Malformed discovery files now remain explicit startup errors before a new child is spawned rather than being treated as an absent runtime, with a focused regression for the malformed-file path.
Discovery records for an answered wrong listener or unauthorized runtime now fail distinctly; only unreachable stale endpoints are retryable during launch convergence.
When two shells race, the loser now waits briefly for the winner that holds the data-directory lease, re-verifies its authenticated runtime identity, and attaches without claiming ownership of the winner's child.
`runtime::tests::only_unreachable_discovery_errors_are_retryable` and `runtime::tests::a_losing_launch_reuses_the_verified_owner_after_its_child_exits` cover the classification and retry seams, with the desktop smoke recorded below.
The isolated startup wait regression now covers an owned child exiting and a bounded deadline, and both return distinct errors after the helper cleans the child it owns.
The 2026-09-06 fresh desktop smoke started a new daemon from an absent runtime file, served the canvas on a shell-owned loopback origin, and returned HTTP 200 for both the canvas and compiled widget manifest.
Optional resource collection remains local to run detail and settings: `RunWidget` keeps the run projection usable when its resource read is unavailable, while `SettingsWidget` hides the resource toggle when the optional runtime route is absent.
The Windows health probe no longer shuts down its request side before reading the response, because that half-close made the real Axum listener return an empty response even though curl and PowerShell succeeded.
The fake listener now answers after the HTTP header terminator, and a real `cargo run -p farseer-shell` attached to the live daemon and served its canvas after the fix.

## Status correction, 2026-09-06

The authenticated owner identity now carries Windows process creation time beside the PID.
The shell compares the `(pid, creation_time)` pair when both sides can query it and deliberately makes no PID-only decision for legacy records or unavailable OS queries.
Focused shell coverage proves creation-time mismatch rejection and the no-PID-only fallback, and the workspace gate passes outside the restricted Windows ACL sandbox.
