# Map: Farseer command center and self-maintenance

Label: `wayfinder:map`.
Status: `awaiting-single-review`.
Branch: `codex/command-center-plan`.
Baseline: `e9d2e76975d7802c50b501f79d3184cc0bd0e543` plus the inherited uncommitted review snapshot.

## Destination

One reviewable decision package, specification, and dependency-linked implementation backlog for a robust headless Farseer with customizable operator views, project teams, economical routing, and bounded self-maintenance.
This effort ends at the operator's single review; product implementation is not part of this planning turn.

## Notes

Open [the short review](REVIEW.md) for nominees, selected recommendations, costs, and the approval boundary.
Open [the specification](spec.md) for behavior and test seams.
Open [the ticket index](tickets.md) only when selecting implementation work or checking requirement coverage.
The [source-backed project critique](../../PROJECT_REVIEW_2026-09-06.md) carries current evidence and code pointers.
The [original decision map](../farseer/map.md) remains the authority for already adopted behavior.

Use the existing local-Markdown tracker convention: each decision is one file under decisions and each implementation slice is one file under issues.
The Parent link identifies this map; named Blocked by links are local dependencies.
Decision status `recommendation selected` means an agent choice awaiting the package review, not a completed human interview.
Implementation status `proposed-awaiting-review` means a reviewable ticket, not permission to code.
After explicit package approval, record the approval and mark decisions adopted; change eligible implementation tickets to ready-for-agent while preserving native file-link blockers.
Claim one eligible ticket before implementation; completing a blocker opens its dependent frontier.

The operator explicitly requested auto-selection and one review, overriding the skills' repeated interviews, per-session decision closure restriction, and pre-draft approval loops.
The selected test seams and ticket granularity are included in that one review.
There is no remote issue publication, new tracker service, or permanent planning script.
Existing modified files are inherited and remain untouched by this planning package.
Historical decision corrections are appended by the adopting implementation slice after approval, never silently rewritten now.

## Implementation state after approval

The package was approved for implementation on the command-center branch.
Verified startup, paged work-board reads, and recoverable schema/backup are implemented and tested.
Widget recovery and explicit composer context have bounded UI increments in progress; their remaining acceptance items stay open.
All other ticket statuses remain as written until their blockers and acceptance evidence are complete.

## Corrections after approval

The operator approved the package for implementation, so the prior awaiting-review gate is closed for this branch.
The selected resolutions in decisions 01 through 05 are adopted for implementation; the original recommendation text remains preserved above.
The package remains the implementation source of truth while work proceeds beyond the original planning-turn boundary.

## Decisions so far

No new decision is operator-adopted yet.
The proposed resolutions live only in their five decision tickets, reached from the short review.
After approval, append one named pointer per adopted resolution here; keep the detailed rationale in its decision ticket.

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

Done: route selected as a proposal, ready for one package review.
Next: open [the short review](REVIEW.md).
