# Map: Farseer command center and self-maintenance

Label: `wayfinder:map`.
Status: approved; implementation in progress.
Branch: `codex/command-center-plan`.
Baseline: `e9d2e76975d7802c50b501f79d3184cc0bd0e543` plus the inherited uncommitted review snapshot.

## Destination

One reviewable decision package, specification, and dependency-linked implementation backlog for a robust headless Farseer with customizable operator views, project teams, economical routing, and bounded self-maintenance.
The original planning turn ended at review; the operator subsequently approved implementation.

## Original planning notes (historical; approval recorded below)

Open [the short review](REVIEW.md) for nominees, selected recommendations, costs, and the approval boundary.
Open [the specification](spec.md) for behavior and test seams.
Open [the ticket index](tickets.md) only when selecting implementation work or checking requirement coverage.
The [source-backed project critique](../../PROJECT_REVIEW_2026-09-06.md) carries current evidence and code pointers.
The [original decision map](../farseer/map.md) remains the authority for already adopted behavior.

Use the existing local-Markdown tracker convention: each decision is one file under decisions and each implementation slice is one file under issues.
The Parent link identifies this map; named Blocked by links are local dependencies.
Decision status `adopted` records the selected package decision.
Implementation status `proposed-awaiting-review` is historical for tickets created before approval; unfinished tickets are now eligible when their named blockers are complete.
Preserve native file-link blockers while implementing and update ticket status only after verification.
Claim one eligible ticket before implementation; completing a blocker opens its dependent frontier.

The operator explicitly requested auto-selection and one review, and approved this package on 2026-09-06.
The selected test seams and ticket granularity are included in that one review.
There is no remote issue publication, new tracker service, or permanent planning script.
Existing modified files are inherited and remain untouched by this planning package.
Historical decision corrections are appended by the adopting implementation slice after approval, never silently rewritten now.

## Implementation state after approval

The package was approved for implementation on the command-center branch.
Verified startup, independent runtime lifecycle, paged work-board reads, scoped graph, bounded transcript projection, recoverable schema/backup, and honest tool authority are implemented and tested.
Widget recovery, explicit composer context, responsive canvas, privacy presentation, focused workspace, session explorer, attributed usage, explainable routing, and project team profiles have bounded increments in progress; their remaining acceptance items stay open.
Terminal profiles, the deterministic local artifact manifest, and the optional resource monitor now have bounded runtime/API/UI slices with explicit follow-up limits.
Maintenance proposals now admit ordinary tasks and candidate artifacts through bounded operator routes, and staged promotion has an explicit disposable fixture CLI with bounded migration and candidate-startup command seams; public end-to-end smoke evidence remains open.
All remaining acceptance items stay open until their blockers and evidence are complete.

## Corrections after approval

The operator approved the package for implementation, so the prior awaiting-review gate is closed for this branch.
The selected resolutions in decisions 01 through 05 are adopted for implementation; the original recommendation text remains preserved above.
The package remains the implementation source of truth while work proceeds beyond the original planning-turn boundary.

## Decisions so far

The five decisions are adopted under the package approval recorded above.
The resolutions and their original rationale live in the five decision tickets, reached from the short review.
Adopted resolutions: [runtime ownership](decisions/01-runtime.md), [bounded data](decisions/02-data.md), [operator workspace](decisions/03-workspace.md), [routing](decisions/04-routing.md), and [maintenance](decisions/05-maintenance.md).

## Not yet specified

No product decision remains unanswered within this proposal: the agent selected the nominees requested by the operator.
Bounded implementation choices, dependency selection, and measured performance targets remain work inside their tickets.
An unexpected failure of an acceptance contract is evidence to update that ticket, not permission to widen scope silently.

## Out of scope

| Area | Reason and return condition |
| --- | --- |
| Native plugin ABI, marketplace, general MCP gateway | Existing seams suffice for the pilot; revisit only with two concrete integrations |
| Arbitrary docking/free-placement layout | Initial pane toggles and flow grid cover the selected workflows |
| Learned routing, Switchyard integration, semantic embeddings | Need observed compatible routes and outcome/retrieval evaluation first |
| Direct project-manager ingress | Existing top-manager ownership remains; revisit with measured coordination cost |
| Live trading/publishing, remote hosts, cross-platform rollout | Separate domain/deployment projects; the local manifest pilot proves the interface first |

Done: package approved; implementation partly complete.
Next: select unfinished work from [the ticket index](tickets.md); verify named blockers and acceptance criteria.

## Status correction, 2026-09-06

The earlier implementation-state sentence calling verified startup fully implemented is historical and is superseded by [Verified startup](issues/01-verified-startup.md).
The authenticated handshake, launch convergence, and distinct answered-listener diagnostics are implemented, while child-owned cleanup and desktop smoke evidence remain open.
The current branch also hardened recoverable backup publication, terminal workspace leases, anchor-only project context, session-to-task navigation, parent/child usage accounting, and the honest authority acceptance evidence.
Those increments do not close the remaining browser, live-runner, promotion, and resource-monitor demonstrations.
The bounded maintenance worker now creates an isolated candidate branch, reproducer, and fixed validation evidence; safe runtime promotion remains separate.

The current implementation wave adds additive task-card run summaries, public global/project board scope coverage, SQLite-filtered transcript search paging, explicit rotated-log labels, and project-team roster display/refusal coverage.
These close the corresponding bounded read and profile acceptance items while session-detail topology, multi-cell profile switching, and browser/live demonstrations remain open.

The follow-up review fixed byte-budget cursor advancement for transcript search, added explicit board loading/empty/stale states, exposed append-only project profile transition history, added a linked session-detail projection, and synchronized the README route/tree documentation.
The session explorer now has a file-backed restart regression covering Claude-shaped and ACP-shaped sessions through the bounded public list route; browser/demo evidence and a dedicated search face remain separate follow-up work.
