# Runtime ownership and optional features

**Parent:** [Command-center decision map](../map.md).
**Label:** wayfinder:grilling.
**Status:** recommendation selected, awaiting the single package review.
**Selection authority:** the operator asked the agent to choose its recommendations without an interview.
**Blocked by:** none.

## Question

What must survive UI failure, and how should the runtime start, stop, and admit optional features?

## Nominees

| Nominee | Reliability | Local deployment | Maintenance cost | Decision |
| --- | --- | --- | --- | --- |
| Independent user-space daemon with explicit shutdown | UI restart preserves work | Existing cargo/bun application and local data | Requires ownership and recovery tests | Selected |
| Shell-owned daemon | Closing the shell ends its daemon | Simplest current path | Hides shutdown inside presentation lifecycle | Rejected |
| Privileged system service | Independent lifecycle | Requires installer/OS administration | Adds service management before it is needed | Deferred |
| In-process native plugin runtime | Optional code shares supervisor failures | Adds ABI and loading rules | Expands the most sensitive maintenance surface | Rejected |

## Proposed resolution

The operational core owns validated contracts, durable record writes, cancellation, workspace cleanup, authorization, and restart recovery.
The pure domain crate remains free of I/O.
Specific runners, presentation, resource sampling, indexing, routing preferences, triggers, and domain integrations are optional capabilities.
Absence of an optional capability produces an explicit local refusal or unavailable view, while unrelated work remains operable.

Window close hides/closes that client and leaves the daemon running, including when the shell originally launched it.
An explicit shutdown command defaults to drain: stop admitting new work and finish active runs.
Drain has a configurable deadline, initially 30 seconds; deadline expiry leaves the daemon alive in draining state and reports the remaining runs.
Forced shutdown is a separate explicit operation that cancels owned runs, waits for supervised process cleanup, finalizes their records, and exits.
Neither close nor a drain deadline silently becomes cancellation.
Normal cancellation semantics and startup orphan recovery remain authoritative.

One daemon owns each canonical data directory through an OS-held exclusive lease released when its process exits.
Use the existing Windows platform seam for the initial implementation; do not implement a stale PID-file lock or infer ownership from TCP openness.
Concurrent launches must acquire ownership before opening an owning writer.
Different data directories may host independent runtimes.

The shell reads private runtime discovery metadata, then validates an authenticated loopback health response.
The handshake checks runtime identity, data-directory identity, protocol/features required by this UI, and build provenance.
Expose a directory fingerprint rather than raw private paths in ordinary display diagnostics.
The startup wait has a bounded 20-second default and checks child exit while waiting.
Wrong listener, invalid token, incompatible feature set, and failed child remain distinct errors.
Failure cleans only the child that this launch actually owns; it never kills a daemon adopted from another launcher.

The current HTTP interface remains the control seam for UI, CLI, tests, and optional clients.
Use additive features/routes; an old UI can continue reading legacy response shapes.
No new mandatory background service, package manager, database engine, or plugin loader is selected.

## Verification and justification

Exercise launch through the real desktop runtime seam with a deterministic local child, close/reopen clients, and observe the same run.
Exercise startup with a wrong listener, old discovery file, incompatible build, simultaneous launches, and failed child.
Assert final states and process ownership through public commands and Job Object membership, not test timing guesses.
The source-backed lifecycle and startup issues are Q1/Q2 in the [project review](../../../PROJECT_REVIEW_2026-09-06.md).

This preserves the existing deployment while making the user's UI-independence requirement observable.
It is more work than removing one Drop implementation: discovery, writer ownership, explicit shutdown, and recovery must agree.

## Existing decisions affected

[Cell primitive](../../farseer/issues/01-cell-primitive.md) supplies the headless invariant.
[Operator surface](../../farseer/issues/28-operator-surface.md) needs an appended clarification that starting a runtime does not confer window-scoped lifetime.
After approval, the implementation ticket appends the correction and the original map pointer; historical text is preserved.

## Revisit condition

Reconsider system-service deployment only when unattended operation before user login becomes a real requirement.
Reconsider an in-process extension only when two proven integrations cannot satisfy the existing client/runner/tool/peer seams.
