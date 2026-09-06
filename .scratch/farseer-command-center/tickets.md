# Implementation ticket index

**Status:** approved; implementation in progress.
**Parent:** [Command-center decision map](map.md).
**Review:** [Read the short decision review](REVIEW.md) before opening individual tickets.

Approval is a package-wide gate and is not repeated as a dependency edge below.
Approval is recorded in the parent map; select an unfinished ticket whose named blockers are complete.
Each ticket is one complete observable slice through the necessary layers, not a separate schema/API/UI assignment.
Refactoring from review packet R13 stays within the slice that requires it.
The five adopted decisions govern contracts; the specification governs behavior; each ticket defines its acceptance.

Current snapshot: the latest status correction at the bottom of this file is authoritative for implementation progress.
Earlier status blocks remain as append-only history; they are not a second backlog or a claim that every ticket is complete.

## Runtime and bounded reads

| Ticket | Blocked by | Review packets |
| --- | --- | --- |
| [Verified startup](issues/01-verified-startup.md) | None; package approved. | R01 |
| [Independent runtime lifecycle](issues/02-independent-runtime.md) | [Verified startup](issues/01-verified-startup.md) | R01 |
| [Paged work board](issues/03-paged-work-board.md) | None; package approved. | R02, R03 |
| [Scoped orchestration graph](issues/04-scoped-graph.md) | [Paged work board](issues/03-paged-work-board.md) | R02, R06 |
| [Bounded transcript projection](issues/05-bounded-transcript-index.md) | None; package approved. | R02, R06 |

## Reliable operator views

| Ticket | Blocked by | Review packets |
| --- | --- | --- |
| [Widget recovery and isolation](issues/06-widget-recovery.md) | None; package approved. | R03 |
| [Explicit composer context](issues/07-explicit-composer-context.md) | None; package approved. | R04 |
| [Responsive canvas layout](issues/08-responsive-canvas.md) | None; package approved. | R05 |
| [Privacy presentation mode](issues/09-privacy-mode.md) | None; package approved. | R05 |
| [Focused workspace](issues/10-focused-workspace.md) | [Widget recovery and isolation](issues/06-widget-recovery.md), [Explicit composer context](issues/07-explicit-composer-context.md), [Responsive canvas layout](issues/08-responsive-canvas.md) | R05 |

## Sessions and orchestration

| Ticket | Blocked by | Review packets |
| --- | --- | --- |
| [Session explorer](issues/11-session-explorer.md) | [Bounded transcript projection](issues/05-bounded-transcript-index.md), [Focused workspace](issues/10-focused-workspace.md) | R06 |
| [Attributed usage](issues/12-attributed-usage.md) | [Paged work board](issues/03-paged-work-board.md) | R07 |
| [Explainable routing](issues/13-explainable-routing.md) | [Attributed usage](issues/12-attributed-usage.md) | R08 |
| [Project team profiles](issues/14-project-teams.md) | [Explicit composer context](issues/07-explicit-composer-context.md), [Explainable routing](issues/13-explainable-routing.md) | R09 |
| [Terminal profiles](issues/15-terminal-profiles.md) | [Independent runtime lifecycle](issues/02-independent-runtime.md), [Focused workspace](issues/10-focused-workspace.md) | R10 |

## Recovery, maintenance, and integration

| Ticket | Blocked by | Review packets |
| --- | --- | --- |
| [Recoverable schema and backup](issues/16-recoverable-schema.md) | None; package approved. | R14 |
| [Bounded maintenance source proposals](issues/17-maintenance-proposals.md) | [Independent runtime lifecycle](issues/02-independent-runtime.md), [Project team profiles](issues/14-project-teams.md). | R11 |
| [Safe staged runtime promotion](issues/18-safe-runtime-promotion.md) | [Independent runtime lifecycle](issues/02-independent-runtime.md), [Recoverable schema and backup](issues/16-recoverable-schema.md), [Bounded maintenance source proposals](issues/17-maintenance-proposals.md). | R11, R14 |
| [Honest tool authority](issues/19-honest-tool-authority.md) | None; package approved. | R15 |
| [Noncoding artifact-manifest pipeline](issues/20-noncoding-pipeline.md) | [Project team profiles](issues/14-project-teams.md), [Honest tool authority](issues/19-honest-tool-authority.md). | R12 |

## Optional resource observations

| Ticket | Blocked by | Review packets |
| --- | --- | --- |
| [Optional supervised resource monitor](issues/21-resource-monitor.md) | [Independent runtime lifecycle](issues/02-independent-runtime.md), [Attributed usage](issues/12-attributed-usage.md), [Recoverable schema](issues/16-recoverable-schema.md) | R07 |

## Review packet coverage

| Review recommendation | Implementation slices |
| --- | --- |
| R01 Runtime ownership/startup | [Verified startup](issues/01-verified-startup.md), [Independent runtime lifecycle](issues/02-independent-runtime.md) |
| R02 Bounded projections | [Paged work board](issues/03-paged-work-board.md), [Scoped orchestration graph](issues/04-scoped-graph.md), [Bounded transcript projection](issues/05-bounded-transcript-index.md) |
| R03 Widget reliability | [Paged work board](issues/03-paged-work-board.md), [Widget recovery and isolation](issues/06-widget-recovery.md) |
| R04 Explicit context | [Explicit composer context](issues/07-explicit-composer-context.md) |
| R05 Canvas and focused workspace | [Responsive canvas layout](issues/08-responsive-canvas.md), [Privacy presentation mode](issues/09-privacy-mode.md), [Focused workspace](issues/10-focused-workspace.md) |
| R06 Session/graph exploration | [Scoped orchestration graph](issues/04-scoped-graph.md), [Bounded transcript projection](issues/05-bounded-transcript-index.md), [Session explorer](issues/11-session-explorer.md) |
| R07 Usage/resources | [Attributed usage](issues/12-attributed-usage.md), [Optional supervised resource monitor](issues/21-resource-monitor.md) |
| R08 Routing | [Explainable routing](issues/13-explainable-routing.md) |
| R09 Project teams | [Project team profiles](issues/14-project-teams.md) |
| R10 Terminal | [Terminal profiles](issues/15-terminal-profiles.md) |
| R11 Maintenance/promotion | [Bounded maintenance source proposals](issues/17-maintenance-proposals.md), [Safe staged runtime promotion](issues/18-safe-runtime-promotion.md) |
| R12 Domain integration | [Noncoding artifact-manifest pipeline](issues/20-noncoding-pipeline.md) |
| R13 Maintainability | Targeted cleanup in every changed slice; no standalone rewrite |
| R14 Versioned storage | [Recoverable schema and backup](issues/16-recoverable-schema.md), [Safe staged runtime promotion](issues/18-safe-runtime-promotion.md) |
| R15 Tool authority | [Honest tool authority](issues/19-honest-tool-authority.md) |

## Execution contract

1. Confirm package approval and completed blockers; claim one eligible ticket.
2. Load its selected decision, relevant specification section, and current module/test surface.
3. Reproduce the failure or demonstrate the new behavior through the highest public seam before editing.
4. Implement the bounded slice, append required decision corrections, and run targeted plus ordinary repository checks.
5. Record acceptance evidence and remaining limits before marking the ticket complete.

Use GPT-5.6 Luna for any delegated work and reasoning no higher than xhigh, as the operator requested.
No paid or external action is inferred from a deterministic test requirement.
New dependencies must be checked, exact-pinned, and optional where this plan says they are optional.

Done: 21 approved slices linked to all 15 review packets; implementation remains partial.
The current implementation has completed the Independent runtime lifecycle, Paged work board, Scoped orchestration graph, Bounded transcript projection, Recoverable schema and backup, and Honest tool authority tickets.
The Verified startup ticket has the authenticated handshake and diagnostic timeout increment, but its launch-convergence and desktop smoke acceptance remains open.
Widget recovery and isolation, Explicit composer context, Responsive canvas layout, Privacy presentation mode, Focused workspace, Session explorer, Attributed usage, Explainable routing, and Project team profiles have bounded work in progress and are not complete until their acceptance evidence is recorded.
Terminal profiles, Bounded maintenance source proposals, Safe staged runtime promotion, Noncoding artifact-manifest pipeline, and Optional supervised resource monitor had no implementation in that historical snapshot.
Next in that historical snapshot was to finish the in-progress operator, session, routing, and team slices before starting terminal, maintenance, promotion, manifest, or resource work.


## Status clarification, 2026-09-06

Approval is complete; earlier proposal and execution-gate wording is historical.
The implementation snapshot then recorded the Verified startup, Independent runtime lifecycle, Paged work board, Scoped orchestration graph, Bounded transcript projection, Recoverable schema and backup, and Honest tool authority tickets as implemented.
Widget recovery and Explicit composer context remain in progress, with focused workspace, privacy, session, usage, routing, and project-team increments also present but incomplete.
The Terminal profiles, Bounded maintenance source proposals, Safe staged runtime promotion, Noncoding artifact-manifest pipeline, and Optional supervised resource monitor tickets have partial implementation; dependency completion still governs eligibility.
This is the current branch status, not a claim that every acceptance item is complete.

## Status clarification, 2026-09-06 implementation wave

Tickets 06, 07, 08, 09, 10, 11, 12, 13, and 14 retain their bounded implementation increments and their named browser or policy evidence gaps.
The Terminal profiles ticket now has the optional terminal API and supervised profile adapter, with the ConPTY and workspace-lease follow-ups still open.
The Noncoding artifact-manifest pipeline ticket now has the deterministic local manifest worker, durable artifact rows, cancellation, Work task display, and an ordinary-route completion regression test; the live cancellation demonstration remains open.
The Optional supervised resource monitor ticket now has first/final and optional periodic Windows Job Object samples, durable retention, a public run-resource read, a runtime toggle, and run-detail labels; deterministic PID-reuse/parent-child fixtures and a live supervised-run demonstration remain evidence follow-ups.
The Bounded maintenance source proposals and Safe staged runtime promotion tickets then remained bounded store primitives without public maintenance task or promotion commands.
No ticket is marked complete solely from source presence; acceptance checkboxes record the verified boundary and the remaining demonstration.

## Status correction, 2026-09-06

The current branch status is the later wave plus the review fixes, not the older snapshot above.
The Verified startup ticket remains partial because child-owned cleanup, recovery-state presentation, capability gating, and desktop smoke evidence are open; competing launches now converge on the verified owner.
The Paged work board ticket has bounded board reads and transition coverage, but its cross-scope and UI acceptance demo remains open.
The Attributed usage ticket now has explicit parent/child de-duplication coverage; its broader two-task acceptance demo remains open.
The Terminal profiles ticket now serializes terminal leases and ignores naturally exited sessions when deciding teardown; the ConPTY/manual demonstration remains open.
The Recoverable schema and backup ticket remains implemented after backup consistency and restore publication hardening.
The Honest tool authority ticket's acceptance evidence is complete.

## Status correction, 2026-09-06 implementation wave 2

The Paged work board now has public global/project scope coverage and additive per-card run summaries for repeated or delegated runs; its manual empty/loading/failure demonstration remains open.
The Session explorer now labels referenced, rotated, and unavailable logs and verifies project/conversation/task/run filters plus search pagination; session-detail topology and restart demonstration remain open.
The Project team profiles now expose manager and roster details and refuse missing specialist cells before work creation; solo/multi-cell transition history and board demonstration remain open.

The follow-up review also closed the board read-state acceptance with UI loading/empty/stale behavior and corrected transcript search cursors under the byte budget.
Project profile reads now expose their recorded transition history; the profile-switch/future-task and multi-cell demonstration remains open.
Session detail now links the canonical run/task, parent topology, transcript custody, and indexed excerpt; the two-protocol restart demonstration remains open.

## Status correction, 2026-09-06 implementation wave 3

The Verified startup ticket now carries an optional owner process id in the authenticated discovery identity.
Only unreachable stale endpoints are retryable during startup; an answered wrong listener or unauthorized runtime fails distinctly, and a losing launcher terminates its non-owner child before attaching.
The Safe staged runtime promotion ticket now runs explicit bounded migration and candidate-startup commands from disposable fixtures without shell interpolation, records observations in the promotion journal, and preserves recovery metadata on failure.
The public browser smoke demonstrations and some live supervised-run evidence remain open by design; these are evidence gaps, not reasons to retain obsolete planning drafts.

## Status correction, 2026-09-06 implementation wave 4

The Safe staged runtime promotion ticket now has a public CLI regression for a failing candidate startup, including restored active identity, quarantined candidate, command observation, and durable recovery journal evidence.

## Status correction, 2026-09-06 implementation wave 5

The Project team profiles ticket now has a deterministic multi-cell regression that records the project specialist set on accepted calls and refuses non-specialist calls before creating a run.

## Status correction, 2026-09-06 implementation wave 6

The shell startup health probe now keeps its write side open until the authenticated response is read, fixing a Windows-only empty-response failure against the real Axum listener.
The focused shell startup suite passes nine tests, and a real `cargo run -p farseer-shell` attached to the running daemon and served the canvas.
