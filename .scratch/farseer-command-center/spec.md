# Farseer Command Center and Self-Maintenance

> Current status, 2026-09-06: package approved and implementation in progress.
> Original review-gate language below is historical; use [the ticket index](tickets.md) for remaining work.

Status: `proposed-awaiting-single-review`.

This document is the consolidated specification for one review before coding.
It turns the current [project review](../../PROJECT_REVIEW_2026-09-06.md) and local decision record into a staged implementation program.
The linked decision documents are proposed authority pending that review: [runtime](decisions/01-runtime.md), [data](decisions/02-data.md), [workspace](decisions/03-workspace.md), [routing](decisions/04-routing.md), and [maintenance](decisions/05-maintenance.md).
The local Markdown tracker is sufficient; no remote issue publication is required.

## Problem Statement

Farseer already has a Rust runtime, SQLite record, cells, runner adapters, manager delegation, and a client-side canvas.
The product direction is a local command center that supervises projects containing one or more harnesses, while also supervising bounded maintenance work on Farseer itself.
The current gaps are operational boundaries rather than missing screens.
The desktop shell can own a daemon too tightly, startup can accept an unauthenticated or incomplete endpoint, optional analysis can compete with execution-critical writes, and the UI does not consistently preserve explicit project and conversation context.
The record has useful event and work semantics but read projections and graph queries need limits before large histories are safe.
Widgets remain replaceable and isolated, while lifecycle, authorization, workspace custody, and recovery stay dependable without a UI.

The desired experience combines a customizable home canvas with a focused workspace for a selected project, conversation, run, or session.
It should expose usage and resource evidence, sessions, boards, graphs, telemetry, terminals, and explainable runner selection.
It should support a solo project or a bounded team of harnesses and leave room for trading, game, content, research, and automation integrations.
The first slice proves one deterministic local non-code process workflow and the self-maintenance proposal path without paid providers or irreversible external effects.

## Solution

Keep the existing Rust, SQLite, and cell foundations.
Run the daemon as a user-space single instance whose lifetime is independent of the desktop window.
The UI can disconnect, reconnect, or be replaced without changing run ownership.
Closing the UI leaves work running; an explicit shutdown drains active work by default, while force cancellation is a separate operator action.
Startup must establish authenticated health identity and feature set before the client declares the backend usable.

Make authoritative lifecycle, task, run, delegation, cancellation, project authorization, workspace custody, and audit events core.
Expose additive, authenticated, bounded read APIs for work lists, sessions, boards, graph slices, usage, resources, and analytics.
Keep existing array responses stable and add paged routes or negotiated envelopes rather than silently replacing their shapes.
Use cursor and limit parameters with proposed tunable defaults of 100 rows per page and 500 rows maximum, with graph pages defaulting to 100 nodes and 300 edges and capped at 500 nodes and 1,500 edges.
Store-critical projections may run synchronously, but similarity, indexing, aggregation, and graph enrichment run outside the store lock with bounded work and explicit stale or unavailable states.

Keep the canvas as home and add a focused workspace with optional navigation, main, and inspector panes.
The workspace is a feature shell, so each feature supplies only the detail it needs.
Persist layout and explicit selection, move sizing controls into layout mode, isolate widget failures, and show useful empty, loading, offline, denied, and unsupported states.
The composer always displays and submits a snapshot of project, conversation, target, and eligible runner context.
Hover may preview context but never retargets a draft.

Use an explicit file-based project profile keyed by an authorized canonical path.
The profile points at a coordinating cell and declares eligible specialists, workspace rules, and defaults without copying cell grants.
The top manager remains the operator ingress.
Project delegation uses configured eligibility and policy narrowing, with no second AI classifier.
Runner and model pins are immutable on each run; a deterministic ordered policy may reject, select, or require operator choice, but a learned cheapest-model promise and Switchyard-style router are deferred.

Treat terminal access as an optional adapter around supervised Windows ConPTY sessions.
Start with one embedded session per terminal view, direct executable and argument profiles for PowerShell, cmd, and Git Bash, bounded output, and explicit close and reconnect behavior.
Switching shells starts a new session and never mutates an active harness run.
Arbitrary untracked external process attachment is deferred.

Treat maintenance as ordinary project work producing a source proposal.
Promotion to an installed runtime is a separate staged operation requiring versioned artifacts, a matching database backup and migration plan, startup validation, and an independent rollback path.
No maintenance agent is required for ordinary execution, recovery, or viewing the record.
External effects belong to their integration and must enforce authorization, idempotency, and reconciliation at the real execution point.
There is no generic MCP gateway that implies authority it cannot enforce.

## User Stories

Core operation:

1. **US01 - Independent operation:** As an operator, I want UI closure to preserve active runs, so that reconnect restores observation of the same work.
2. **US02 - Predictable shutdown:** As an operator, I want distinct drain and force-cancel operations, so that a shutdown deadline cannot silently cancel work.
3. **US03 - Trusted connection:** As a client, I want authenticated runtime identity and feature verification, so that controls never target an incompatible listener.
4. **US04 - Recoverable core:** As an operator, I want run commands through the API or CLI, so that desktop failure does not remove control.

Projects and routing:

1. **US05 - Project context:** As an operator, I want explicit project/conversation/runner selection, so that submission uses the context I reviewed.
2. **US06 - Harness teams:** As a project owner, I want solo and multi-harness work under the same task model, so that parallel work retains isolation and accountable ownership.
3. **US07 - Explainable choice:** As an operator, I want reasons for selected and rejected runners/models, so that I can assess quality and cost tradeoffs.
4. **US08 - Safe delegation:** As a manager, I want policy-narrowed worker and cell delegation, so that authority, budgets, cancellation, and lineage remain intact.

Observation and workspace:

1. **US09 - Usage and resources:** As an operator, I want attributed provider and optional process measurements with provenance, so that unknown usage is not presented as savings or zero.
2. **US10 - Session explorer:** As an operator, I want bounded session, transcript, board, and causal-graph navigation, so that large histories remain usable.
3. **US11 - Focused command center:** As an operator, I want a customizable home and a focused workspace, so that overview and sustained work share the same selected subject.
4. **US12 - Terminal continuity:** As an operator, I want supervised shell profiles and reconnectable sessions, so that terminal presentation can change without mutating harness runs.

Maintenance and extension:

1. **US13 - Self-maintenance proposal:** As a maintainer, I want bounded source proposals with validation evidence, so that improvement work leaves the active installation untouched.
2. **US14 - Controlled promotion:** As an operator, I want staged promotion and independent matched binary/data rollback, so that a failed candidate remains recoverable.
3. **US15 - Optional integrations:** As an integration author, I want existing runner/widget/tool/peer seams, so that my feature can fail or be disabled without stopping unrelated work.
4. **US16 - First non-code workflow:** As an operator, I want a local artifact-manifest pipeline with cancellation and recorded outputs, so that plain-directory work proves domain independence without paid or irreversible effects.

Reliability and privacy:

1. **US17 - Partial availability:** As an operator, I want a broken widget or optional query to fail locally, so that the board and composer remain usable.
2. **US18 - Privacy presentation:** As an operator, I want a persistent masking mode with explicit reveal, so that sharing the UI does not expose account or path identifiers.
3. **US19 - Rebuildable analysis:** As an operator, I want bounded rebuildable lexical projections with coverage labels, so that analysis can improve without rewriting observed history.
4. **US20 - Configuration continuity:** As an operator, I want persisted layout and file-based project defaults, so that reconnecting restores preferences without creating a second authority model.

## Implementation Decisions

### Runtime and identity

The daemon is the authority for process ownership and state.
The desktop shell owns only the connection.
A single-instance lock, authenticated health response, protocol version, feature set, and explicit startup failure are mandatory.
A health response must not accept a process merely because a socket answers.

### Lifecycle

Default shutdown is drain with a tunable 30-second deadline; expiry reports remaining runs and leaves the daemon alive and draining.
Force cancellation is explicit and visible.
Reconnect uses the existing exclusive event cursor, handles gap/purge signals explicitly, and resumes durable replay where retained history permits.

### Data and read safety

SQLite remains authoritative.
Read APIs are additive and bounded, and every expensive projection reports freshness and truncation.
The proposed defaults are tunable: event and list pages of 100 rows with a 500 maximum, graph pages of 100 nodes and 300 edges with a 500-node and 1,500-edge maximum, and responses of at most 1 MiB excluding raw transcript streams.
One writer handles synchronous store-critical writes and lifecycle transitions; optional analysis uses one worker and a queue of 32, is bounded, cancellable, and never blocks a run indefinitely.
Index reads use existing index tables and bounded candidates rather than scanning all bodies under the store lock.
Source input is capped at 16 MiB while it is read, each job has a 32 MiB input budget, candidates are capped at 200, and results return the top 20 with a threshold label.
Similarity is labeled lexical or hash-based with its projection metadata.
Semantic embeddings are deferred until a measured use case and retention policy justify them.

### Projects and workspaces

A versionable file-based profile references an authorized canonical path and a cell; it does not copy grants into a second policy system.
Each spawned worker follows its sealed workspace strategy, with concurrent artifact writers isolated; recorded task/run context and runner/model pins do not change in place.
Artifact integration is explicit, recorded, and idempotent.

### Routing and teams

The top manager remains the only operator AI ingress.
Project profiles provide the eligible and default coordinating cell and worker candidates, validated server-side before the top manager acts.
The routing stage is a deterministic ordered constraint evaluator: explicit pin, project eligibility, capability, health, quota, budget, then configured cost preference.
It selects defaults and explains constraints but does not replace top-manager coordination or fabricate a second manager actor.
Top-manager coordination tokens remain part of measured cost.
It must never silently downgrade, loop, or claim billed savings from list prices.
Configured team composition is a project policy, not a new execution engine.

### UI and terminal

The canvas is the home screen.
The focused workspace opens by explicit selection or keyboard command and can render optional navigation, main, and inspector regions.
There is no required docking framework.
Widgets are client-side, independently failed, and unable to address a cell directly.
The terminal is a supervised optional feature with proposed tunable defaults of one session per view, 10,000 output lines, and a 1 MiB retained buffer.

### Telemetry and resource evidence

Process-only resource samples are optional and carry source, timestamp, scope, and sampling status.
The first Windows observer reports supervised-job cumulative CPU time and memory high-water mark where the adapter can measure them, at a five-second default interval with bounded retention.
Unsupported values remain absent; cross-platform resource collection is outside this Windows-first delivery.
Provider usage is observation data, not a fabricated global meter.
Retention is specified separately for authoritative events, raw attachments, derived indexes, and process samples.

### Self-maintenance and extension

A maintenance run is an ordinary isolated cell run that produces a proposal, test evidence, and previous-version pointer.
Promotion is separate, staged, and gated by a matching database backup, ordered migration, startup health check, and rollback path that does not require the candidate binary.
Widgets, runners, tools, and peers are optional seams.
External integrations own their actual enforcement and reconciliation.

## Testing Decisions

Test the public seams first, with internal unit tests supporting them.
The runtime and API suite must start the daemon through the CLI, verify authenticated health identity and features, disconnect the UI, reconnect with a cursor, exercise drain and force cancellation, and prove that a failed startup returns a nonzero actionable result.
Include restart, duplicate request, stale cursor, unauthorized project, unavailable runner, budget rejection, and foreign workspace cases.

Exercise bounded reads against generated histories larger than the proposed limits.
Verify that graph, search, indexing, and similarity return bounded pages, freshness metadata, and recoverable unavailable states while a write and a live run continue.
Test migration fixtures, interrupted migration recovery, matching database and attachment backup restore, and refusal by an incompatible older binary.

Use browser-level tests for canvas persistence, explicit context selection, no hover retargeting, responsive focus view, keyboard navigation, widget failure isolation, privacy masking, and empty or offline states.
Test the API error path through the rendered UI rather than asserting only a helper result.
Verify that optional three-pane regions work at narrow and wide viewports without requiring a dock library.

Test terminal profiles through the CLI and browser seam with spaces and non-ASCII paths, resize, input, reconnect, profile switching, close, and child cleanup by verified ownership.
Prove that switching profiles cannot affect a harness run.

The pilot acceptance test uses one deterministic local non-code process with no paid provider and no irreversible effect.
It must start, stream progress, expose resource provenance when available, produce an artifact, support cancellation, recover after UI reconnect, and leave an inspectable task and run lineage.
The maintenance acceptance test produces a source proposal and evidence, cannot recursively schedule itself, and leaves normal execution usable when maintenance is disabled.
Run the existing cargo and bun checks plus these focused API, CLI, browser, migration, and pilot checks before completion.

## Out of Scope

- A universal in-process plugin ABI, arbitrary docking framework, or second execution engine.
- A learned autonomous router, Switchyard integration, guaranteed cheapest model, or semantic embedding platform.
- Generic external MCP authority, arbitrary process attachment, or untracked terminal ownership.
- Automatic live source hot-patching, silent binary replacement, or promotion without independent recovery.
- Paid provider probes, remote execution, trading actions, publishing, social posting, or other irreversible external effects in the first pilot.

Later operational expansion:

- Universal retention tiering, a new graph database, or a percentage derived from incomplete quota observations.
- A mandatory permanent team for every project or a maintenance manager required for core operation.

## Further Notes

The [project review](../../PROJECT_REVIEW_2026-09-06.md) remains the evidence baseline, while the five linked decision documents become authority only after the single review adopts them.
Those decisions must resolve daemon ownership, data/version compatibility, workspace and project authorization, deterministic routing, and staged maintenance promotion before coding tickets are opened.
The first implementation sequence is runtime identity and shutdown, bounded read projections, explicit UI context and focused workspace, project profiles and explainable routing, then the terminal and pilot.
The maintenance proposal path may begin after runtime and data recovery contracts exist, but installed promotion waits for migration and rollback evidence.
If a decision conflicts with an older ticket, append a correction to that ticket and the local map rather than silently changing historical intent.
The selected workspace explicitly amends the old canvas-only detail contract while retaining canvas home and shared state.
The first optional process metrics are Windows supervised-job CPU time and memory high-water mark; portable metrics are deferred.
The selected local pilot creates an artifact manifest containing relative names, sizes, and SHA-256 digests from an authorized fixture directory, using a cancellable local worker and staged output.
It does not alter the input files, access a network, or require Git.
Remaining implementation choices such as a terminal renderer library must meet these contracts and dependency rules; they do not reopen the selected architecture or require another product interview.
No implementation should start while this document retains `proposed-awaiting-single-review`.
