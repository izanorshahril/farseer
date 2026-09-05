# Farseer codebase review

**Snapshot:** `426d19a` on `feat/acp-runner`  
**Reviewed:** 2026-08-31  
**Specification:** [`.scratch/farseer/map.md`](.scratch/farseer/map.md) and its linked decision tickets

## Result

Farseer's foundation is substantial, but the codebase is not v1-complete against the local map and tickets.
The core model, store, runner adapters, manager loop, delegation, quota accounting, projects, desktop shell, and much of the canvas are implemented and well tested.
The remaining work includes two safety-boundary defects and several decided surfaces that have not shipped.

A static ticket-to-implementation assessment places the codebase at roughly **70% implemented and evidenced**.
That estimate is directional rather than a ticket count because the tickets vary greatly in size and several are research decisions rather than product features.

| Area | Assessment | Main gap |
| --- | --- | --- |
| Core model and store | Strong | Purge does not implement the decided tombstone and `void` semantics |
| Runner supervision | Strong but unsafe at startup | Process execution begins before Job Object assignment |
| Manager delegation | Broadly implemented | Cross-cell children escape caller cancellation ownership |
| Operator surface | Partial | Attach, takeover, release, cell lifecycle, and gated actions are absent |
| External protocol boundaries | Missing | No inbound ACP server or A2A and Agent Card endpoint |
| Documentation and delivery gates | Inconsistent | README and code comments are stale, and formatting fails |

## Standards

### S1 - Shell proxy bypasses Origin validation

**Severity:** P1  
**Status:** confirmed

The desktop shell exposes `/v1` and settings routes on its own loopback server, discards the request's `Origin`, and attaches the process-wide operator bearer before forwarding the request.
This bypasses the daemon guard that rejects a non-loopback browser origin.
A cross-site simple request can therefore reach bearer-authenticated state-changing routes without satisfying the protection required by [16 local API surface](.scratch/farseer/issues/16-local-api-surface.md#6-security-token-in-a-user-acld-file-and-a-url-fragment-plus-origin-validation).

Evidence:

- [`crates/farseer-shell/src/serve.rs:57`](crates/farseer-shell/src/serve.rs#L57) exposes the proxy and shell settings routes without an equivalent guard.
- [`crates/farseer-shell/src/serve.rs:192`](crates/farseer-shell/src/serve.rs#L192) forwards selected headers but not `Origin`, then injects the bearer.
- [`crates/farseer-api/src/security.rs:58`](crates/farseer-api/src/security.rs#L58) documents the independent `Host` and `Origin` checks and why both exist.

Recommendation: forward `Origin` to the daemon or apply the same loopback-origin guard in the shell before any proxy or shell-local mutation.

### S2 - Process creation violates the locked Job Object ordering

**Severity:** P1  
**Status:** confirmed

[`SupervisedProcess::spawn`](crates/farseer-runner/src/spawn.rs#L174) starts the process normally and assigns it to a Job Object afterwards.
The locked result in [03 spike job objects](.scratch/farseer/issues/03-spike-job-objects.md#three-implementation-facts-to-carry-into-the-runtime) requires `CREATE_SUSPENDED`, Job Object assignment, and only then resuming the primary thread.
A process that runs before assignment can create a descendant outside the job, leaving that descendant unreapable.

Evidence:

- [`crates/farseer-runner/src/spawn.rs:181`](crates/farseer-runner/src/spawn.rs#L181) calls `Command::spawn` with only `CREATE_NO_WINDOW`.
- [`crates/farseer-runner/src/spawn.rs:199`](crates/farseer-runner/src/spawn.rs#L199) creates and assigns the Job Object after the child is already running.
- [`03-spike-job-objects.md:79`](.scratch/farseer/issues/03-spike-job-objects.md#L79) names any other ordering as a rare and unreproducible race.

Recommendation: use a Windows process creation path that exposes the primary thread handle, create suspended, assign to the configured job, and resume only after assignment succeeds.

### S3 - Cross-cell child runs are outside caller cancellation ownership

**Severity:** P1  
**Status:** confirmed

Worker delegation inserts the new run into the manager context's `child_runs` set, but cell delegation does not.
Cancelling the caller collects cancellation handles only from that set, so an active cross-cell manager continues after its owner is cancelled.
This contradicts the documented rule that cancelling a manager cancels every active delegated child.

Evidence:

- [`crates/farseer-api/src/mcp.rs:560`](crates/farseer-api/src/mcp.rs#L560) registers a delegated worker and removes it through `ChildRunRegistration`.
- [`crates/farseer-api/src/mcp.rs:754`](crates/farseer-api/src/mcp.rs#L754) spawns a cell-call manager without equivalent registration.
- [`crates/farseer-api/src/lib.rs:1334`](crates/farseer-api/src/lib.rs#L1334) cancels only the run IDs present in `child_runs`.

Recommendation: register the callee run under the caller's manager context for the lifetime of the cell call and add a cancellation test covering a live cross-cell child.

### S4 - Orphan recovery silently stops after 5,000 rows

**Severity:** P2  
**Status:** confirmed

The orphan sweep promises to close every run left `running` by a previous process, but it queries only the newest 5,000 rows.
Older orphaned rows can therefore remain `running` permanently after enough accumulated history.

Evidence:

- [`crates/farseer-store/src/lib.rs:341`](crates/farseer-store/src/lib.rs#L341) states the all-orphans recovery contract.
- [`crates/farseer-store/src/lib.rs:362`](crates/farseer-store/src/lib.rs#L362) calls `recent_runs(5_000)`.
- [`crates/farseer-store/src/lib.rs:455`](crates/farseer-store/src/lib.rs#L455) applies the SQL `LIMIT`.

Recommendation: query all `running` rows directly or sweep them in bounded pages until none remain.

### S5 - Repository documentation contradicts shipped behavior

**Severity:** P2  
**Status:** confirmed

The README, AGENTS file, and several module comments describe old capability states after the implementation changed.
This is material because the repository requires its compact tree and Mermaid flow to change with structure and behavior.

Examples:

- [`README.md:198`](README.md#L198) says the MCP face has three tools while the code and [`AGENTS.md:78`](AGENTS.md#L78) name four.
- [`README.md:257`](README.md#L257) says delegation reach, the UI, and pi remain unbuilt even though all are implemented.
- [`AGENTS.md:38`](AGENTS.md#L38) says non-Claude manager MCP wiring remains open after ticket 31 closed four transports.
- [`crates/farseer-manager/src/lib.rs:1285`](crates/farseer-manager/src/lib.rs#L1285) still says the ACP runner is unimplemented.

Recommendation: update the repository map sections in README, AGENTS, UI README, and stale module comments in one documentation pass after the correctness fixes.

### S6 - Daemon startup timeout returns fake success

**Severity:** P1  
**Status:** confirmed

When the sidecar does not publish a reachable runtime within the startup window, the shell returns `Ok` with port `0` and an empty token.
The desktop then opens against a nonexistent daemon instead of reporting the startup failure.

Evidence:

- [`crates/farseer-shell/src/runtime.rs:63`](crates/farseer-shell/src/runtime.rs#L63) documents waiting for a real runtime file.
- [`crates/farseer-shell/src/runtime.rs:107`](crates/farseer-shell/src/runtime.rs#L107) converts timeout into an `Ok(Attached)` sentinel.
- [`crates/farseer-shell/src/main.rs:46`](crates/farseer-shell/src/main.rs#L46) treats that value as a successful daemon start.

Recommendation: return an error that includes the child status and captured startup context, and reap the failed sidecar before exiting.

No material Fowler baseline smells were found beyond these documented boundary and correctness breaches.

## Spec

### P1 - Inbound ACP and A2A surfaces are absent

**Severity:** P1  
**Status:** missing

[16 local API surface](.scratch/farseer/issues/16-local-api-surface.md#1-bespoke-http-plus-sse-is-the-substrate) requires an ACP server adapter as a first-class feature.
[06 cell transport](.scratch/farseer/issues/06-cell-transport.md#1-both-with-an-ordering) and [21 A2A conformance](.scratch/farseer/issues/21-a2a-conformance.md) require an optional A2A endpoint, Agent Card, discovery, and per-peer authentication.
The current route table exposes local HTTP, SSE, manager JSON, and MCP only.

Evidence:

- [`crates/farseer-api/src/lib.rs:407`](crates/farseer-api/src/lib.rs#L407) contains the complete router without ACP server, A2A, Agent Card, or discovery routes.
- [`README.md:265`](README.md#L265) explicitly records both protocol surfaces as decided but unbuilt.

### P2 - Attach, takeover, and release are absent

**Severity:** P1  
**Status:** missing

[07 attach semantics](.scratch/farseer/issues/07-attach-semantics.md#3-read-only-by-default-explicit-takeover) requires read-only attach, explicit takeover, release, intervention provenance, and automatic release through client heartbeats.
[28 operator surface](.scratch/farseer/issues/28-operator-surface.md#5-where-each-verb-lives) places `observe`, `take over`, and `release` directly on the run line.
The core control enum exists, but there is no API path or live control implementation and the canvas exposes only `steer` and `cancel`.

Evidence:

- [`crates/farseer-api/src/lib.rs:407`](crates/farseer-api/src/lib.rs#L407) has no attach or control-axis routes.
- [`ui/src/widgets/runs.tsx:55`](ui/src/widgets/runs.tsx#L55) derives only `steer` and `cancel`.
- [`ui/README.md:69`](ui/README.md#L69) explicitly lists attach and takeover as unbuilt.

### P3 - Cell lifecycle is absent and purge has the wrong semantics

**Severity:** P1  
**Status:** missing and incorrect

[17 cell lifecycle](.scratch/farseer/issues/17-cell-lifecycle.md) requires drain, archive, delete, scoped purge, cell-zero protection, purge tombstones, and `void` rather than a recoverable stream gap.
The API exposes none of the cell lifecycle verbs.
The store's available purge function destroys the entire cell slice without a scope, tombstone, or `void` event.

Evidence:

- [`crates/farseer-api/src/lib.rs:407`](crates/farseer-api/src/lib.rs#L407) has only list, read, reload, and instruct cell routes.
- [`crates/farseer-store/src/lib.rs:293`](crates/farseer-store/src/lib.rs#L293) deletes all events, memories, edges, and runs for a cell.
- [`crates/farseer-core/src/event.rs:67`](crates/farseer-core/src/event.rs#L67) defines no `void` event kind.

### P4 - Operator re-run and re-scope provenance is not an event

**Severity:** P2  
**Status:** partial

[16 local API surface](.scratch/farseer/issues/16-local-api-surface.md#5-control-verbs) requires operator-initiated re-run and re-scope to leave an event in the record.
The implementation records only a `rescoped_from` database edge, so the action is available to analytics but absent from the event stream and its actor provenance.

Evidence:

- [`crates/farseer-api/src/lib.rs:1471`](crates/farseer-api/src/lib.rs#L1471) treats the edge as satisfying the event requirement.
- [`crates/farseer-api/src/lib.rs:1523`](crates/farseer-api/src/lib.rs#L1523) calls only `record_rescope` before spawning.
- [`crates/farseer-store/src/lib.rs:408`](crates/farseer-store/src/lib.rs#L408) inserts only into `rescoped_from`.

Recommendation: keep the edge for analytics and append a distinct operator-authored event identifying re-run or re-scope.

### P5 - Job Object assignment does not implement the spike result

**Severity:** P1  
**Status:** incorrect

The process startup race in standards finding S2 is also a direct specification failure.
It invalidates the runtime guarantee derived from the Job Object spike even though the basic cancellation tests pass.

## Known explicit leftovers

These are recorded as incomplete in the tickets and should not be mistaken for newly discovered regressions.

- [32 harness capability floor](.scratch/farseer/issues/32-harness-capability-floor.md#open) records which skills were handed to a run but not which were actually invoked.
- [38 the tool verb](.scratch/farseer/issues/38-the-tool-verb.md#still-open) leaves `zero.toml`'s shell entry and `ToolLevel::Shell` as two statements of one fact.
- [35 notification plane](.scratch/farseer/issues/35-notification-plane.md#still-open) retains notification chattiness and a dangling ticket-34 citation as known follow-up work.
- Operator approval for global memory promotion exists in the store contract but has no API or canvas surface.
- Gated actions remain a decided model without a complete runtime interaction surface.

## Validation

| Check | Result |
| --- | --- |
| `cargo test --workspace` | Passed outside the Codex sandbox: 342 passed, 25 ignored |
| No-cost capability drift tests | Passed outside the sandbox: 6 passed |
| Remaining live and subscription tests | 19 ignored and not run |
| `cargo clippy --workspace --all-targets` | Passed |
| `cargo fmt --all -- --check` | Failed in four hunks |
| `bun run --cwd ui check` | Passed |
| `bun run --cwd ui build` | Passed, including 3 of 3 widgets |
| `git diff --check` | Passed |

The first sandboxed Rust test run failed only on Windows ACL and temporary-file permissions and passed when repeated outside the sandbox.
The first sandboxed capability-drift run could not read opencode's configuration correctly and passed when repeated outside the sandbox.

Formatting differences are currently reported in:

- `crates/farseer-api/src/lib.rs`
- `crates/farseer-api/src/mcp.rs`
- `crates/farseer-api/src/projects.rs`

## Recommended order

1. Fix shell `Origin` handling and add a cross-origin mutation regression test.
2. Replace post-spawn Job Object assignment with suspended creation, assignment, and resume, then test a child that spawns immediately.
3. Link cross-cell children into caller cancellation ownership and add a live-handle test.
4. Implement cell lifecycle and correct purge semantics before exposing any destructive UI.
5. Finish attach and inbound protocol surfaces, then reconcile README, AGENTS, and module documentation with the shipped product.

## Summary

**Standards:** 6 findings.
The worst standards issue is the shell proxy bypassing the authenticated API's browser-origin boundary.

**Spec:** 5 findings.
The worst spec issue is the prohibited process-startup race that can let descendants escape Farseer's Job Object.
