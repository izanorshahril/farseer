# Project teams and economical routing

**Parent:** [Command-center decision map](../map.md).
**Label:** wayfinder:grilling.
**Status:** recommendation selected, awaiting the single package review.
**Selection authority:** the operator delegated nominee selection to the agent.
**Blocked by:** [Runtime ownership and optional features](01-runtime.md), [Bounded record views and optional analysis](02-data.md).

## Question

How do projects use multiple harnesses without duplicating the cell model or claiming an unknowable cheapest route?

## Nominees

| Nominee | Existing model fit | Cost predictability | Maintenance | Decision |
| --- | --- | --- | --- | --- |
| File-based project profile referencing cells; deterministic eligible runner choice | Reuses policy and lineage | Explainable baseline | Small configuration addition | Selected |
| New independent team/agent/task engine | Duplicates current ownership | More coordination overhead | Two systems to reconcile | Rejected |
| Direct input to any project manager immediately | Changes top-manager ownership | Can reduce relay cost | Requires new authority decision | Deferred |
| Learned classifier or Switchyard as the fleet router | Model routing differs from runner ownership | Adds classification/translation costs | Premature integration | Deferred |

## Proposed resolution

A project remains a canonical authorized directory; a profile is configuration about that path, not another durable work entity.
Store profiles as versionable files in the existing configuration repository, with validate/reload and a read projection.
Keep path-specific local configuration private to that installation when appropriate; do not seed private paths into shipped examples.
Profiles reference a coordinating cell and optional existing specialist cells and narrow preferences/limits.
They do not copy rosters, tool grants, credentials, or independent budget pools.
An absent profile uses existing cell-zero defaults.

Precedence is explicit request, then conversation pin, then project preference, then cell default, with all choices intersected with current authorized candidates and policy ceilings.
An explicit unsupported pin fails; it never silently falls through.
Profile preferences can narrow authority but cannot widen a cell grant.
Removal or revocation affects new work; a started run retains its pinned contract and defined cancellation authority.
Renaming/moving a directory does not silently transfer its profile or authorization to another path.

Top-manager ingress and the existing manager-scoped cell-call path stay in the first delivery.
Project configuration makes the intended eligible coordinating cell clear and removes the need for a separate classifier call.
It does not eliminate the actual top-manager turn or its cost.
Do not fabricate a manager actor for a runtime routing decision.
If measured coordination overhead later justifies direct project-manager ingress, it is a separate correction to the operator-surface rule.

A task can include several harness runs concurrently under one bounded ownership tree.
Code workers use isolated worktrees; non-code workers stage separate artifacts.
One integration owner selects/promotes the result, with provenance.
A solo project uses the same path without placeholder workers or permanent agents.
Harness-native subagents remain observations unless Farseer actually spawned and supervises separate worker runs.

Runner routing filters capability, authorization, availability, and pins before ordered preference.
Provider/model routing is a separate optional adapter capability.
The initial router records considered/rejected candidates, selected route, observation freshness, policy version, and resulting run identity.
Unknown quota is neither zero capacity nor guaranteed capacity.
No automatic downgrade or retry occurs without a standing policy; initial retry policy is zero automatic retries unless configured, with a finite cap.
An unavailable eligible pool returns an explicit wait/refusal outcome; durable auto-resume queues are not implied by a status label.

Usage separates provider-reported currency, list-price estimates, tokens, quota windows, and optional process measurements.
Compute accepted-task cost including attempts and coordination, without summing a parent aggregate over the same child usage twice.
The model that was requested and the model actually observed are separate values.
Strict pre-spend caps remain refused where the runner cannot demonstrate enforcement.

## Verification and justification

Exercise a solo project and a project using two distinct configured runner adapters with deterministic fixtures.
Show one task tree, independent workspaces, cancellation propagation, and one artifact integration owner.
Route fixtures cover exhausted/unknown quota, explicit invalid pins, revoked profile access, and differing cost bases.
Compare measured accepted-task outcomes to the ordered baseline before introducing learned policy.

This addresses multiple-harness readiness using existing primitives and exposes the coordination-cost tradeoff instead of promising global optimality.

## Existing decisions affected

[Installed project context](../../farseer/issues/39-what-an-installed-farseer-points-at.md) needs a profile-configuration extension after approval.
[Work and session explorer](../../farseer/issues/40-work-and-session-explorer.md) retains path snapshots, task/run distinctions, and top-manager input ownership.
Runner-routing decisions gain a provenance extension rather than a new scheduler.

## Revisit condition

Consider a model gateway only for a verified compatible harness/provider path and an evaluation showing better accepted-task economics.
Consider direct project ingress only after measuring the top-manager relay cost and specifying authority/provenance without ambiguity.
