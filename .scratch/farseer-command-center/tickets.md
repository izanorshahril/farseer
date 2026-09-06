# Implementation ticket index

**Status:** approved; implementation in progress.
**Parent:** [Command-center decision map](map.md).
**Review:** [Read the short decision review](REVIEW.md) before opening individual tickets.

Approval is a package-wide gate and is not repeated as a dependency edge below.
Approval is recorded in the parent map; select an unfinished ticket whose named blockers are complete.
Each ticket is one complete observable slice through the necessary layers, not a separate schema/API/UI assignment.
Refactoring from review packet R13 stays within the slice that requires it.
The five adopted decisions govern contracts; the specification governs behavior; each ticket defines its acceptance.

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
The current implementation has completed tickets 01, 02, 03, 04, 05, 16, and 19.
Tickets 06, 07, 08, 09, 10, 11, 12, 13, and 14 have bounded work in progress and are not complete until their acceptance evidence is recorded.
Tickets 15, 17, 18, 20, and 21 have no implementation in this branch.
Next: finish the in-progress operator, session, routing, and team slices before starting terminal, maintenance, promotion, manifest, or resource work.


## Status clarification, 2026-09-06

Approval is complete; earlier proposal and execution-gate wording is historical.
The implementation snapshot now records tickets 01, 02, 03, 04, 05, 16, and 19 as implemented.
Widget recovery and Explicit composer context remain in progress, with focused workspace, privacy, session, usage, routing, and project-team increments also present but incomplete.
Tickets 15, 17, 18, 20, and 21 have partial implementation; dependency completion still governs eligibility.
This is the current branch status, not a claim that every acceptance item is complete.

## Status clarification, 2026-09-06 implementation wave

Tickets 06, 07, 08, 09, 10, 11, 12, 13, and 14 retain their bounded implementation increments and their named browser or policy evidence gaps.
Ticket 15 now has the optional terminal API and supervised profile adapter, with the ConPTY and workspace-lease follow-ups still open.
Ticket 20 now has the deterministic local manifest worker, durable artifact rows, cancellation, and Work task display, with ordinary-route and live cancellation demonstrations still open.
Ticket 21 now has first/final Windows Job Object samples, durable retention, a public run-resource read, and run-detail labels, with periodic sampling, a runtime toggle, and PID-reuse fixtures still open.
Tickets 17 and 18 remain bounded store primitives without public maintenance task or promotion commands.
No ticket is marked complete solely from source presence; acceptance checkboxes record the verified boundary and the remaining demonstration.
