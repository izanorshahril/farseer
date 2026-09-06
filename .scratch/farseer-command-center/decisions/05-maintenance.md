# Self-maintenance and domain integration

**Parent:** [Command-center decision map](../map.md).
**Label:** wayfinder:grilling.
**Status:** recommendation selected, awaiting the single package review.
**Selection authority:** agent recommendation chosen under the operator's instruction, pending review.
**Blocked by:** [Runtime ownership and optional features](01-runtime.md), [Project teams and economical routing](04-routing.md).

## Question

How can Farseer maintain itself and supervise other domains without depending on an agent for its own recovery?

## Nominees

| Nominee | Recovery when new code fails | Ongoing token cost | Domain isolation | Decision |
| --- | --- | --- | --- | --- |
| Ordinary maintenance cell produces candidates; deterministic promotion/rollback | Independent previous-version recovery | Event-triggered, bounded | Reuses existing seams | Selected |
| Agent hot-patches the running installation | Same system changes and judges itself | Unbounded without new controls | Couples policy and execution | Rejected |
| Permanent LLM global board/maintenance team | No independent recovery improvement | Idle reasoning and relay costs | Adds mandatory agents | Rejected |
| General plugin/MCP gateway for every domain | Broad new enforcement surface | More plumbing and observation | Unproven abstraction | Rejected |

## Proposed resolution

Farseer's own repository is an ordinary authorized project with a maintenance cell.
Trigger a bounded maintenance task from an explicit request or deduplicated relevant event.
Initially allow one active maintenance task, no automatic retry after failure, and suppression of events produced by that same maintenance lineage.
Persist the triggering identity and attempt result so restart does not duplicate the proposal.
Maintenance can pause independently of all other projects.
Deterministic board transitions and ordinary monitoring do not require a model call.

The first maintenance delivery only creates an isolated candidate branch/artifact with reproduction, validation, and previous-revision evidence.
It never promotes that candidate merely because its author says it succeeded.
The installed daemon keeps executing the previous version.
Runtime promotion is a separate later operation with explicit operator authorization in the product until a narrower automatic policy is independently specified.
The current single planning approval authorizes neither a real future deployment nor external effects.

Version storage explicitly before automated installation changes.
Migrations are ordered, repeat-safe, and guarded by a schema compatibility check.
A database backup uses a consistent SQLite mechanism and includes a manifest for externally held attachment bytes.
An old incompatible binary refuses a newer schema rather than modifying it.
Test restoring the backup with its matching binary; a backup file's existence is insufficient evidence.

Promotion stages a new version beside the active version, drains with the runtime contract, backs up consistent state, migrates if required, starts the candidate, and checks authenticated health and a deterministic smoke scenario.
Any pre-switch failure leaves the old version active.
Post-switch failure restores the matched prior binary and data before admitting work.
If restoring fails, remain stopped with actionable recovery information instead of repeatedly cycling versions.
New task admission resumes only after health/recovery succeeds.
The recovery command operates without the candidate Farseer process being healthy.
There is no in-place executable replacement and no assumption that downgrading the binary alone reverses a migration.

The first non-coding integration is a deterministic local process that writes a staged artifact in a plain-directory workspace.
It proves start, output/progress, cancel, result inspection, and promotion/cleanup without paid credentials or irreversible external effects.
Long operations are supervised workers; returning calls are tools; external orchestrators making delegation decisions are peers.
Trading, publishing, social collection, and a full workflow designer remain future domain projects using those seams.

Respect the existing decision against a general third-party tool gateway.
A roster entry with no callable serving path is visibly non-enforcing.
Keep the existing explicit cell shell grant and ToolLevel, and clarify their different roles: the grant authorizes shell reach while ToolLevel requests a runner restriction.
Effective reach must satisfy both; a default ToolLevel of Shell cannot create a missing explicit grant.
The shell-capable roster metadata can affect launch authorization even though the named tool has no callable Farseer verb.
Do not remove that guard as a cosmetic deduplication.
Domain adapters own external-action authorization, idempotency, and reconciliation unless an explicitly specified Farseer operation actually sits in that execution path.
An unknown result after a timeout is not proof that an external effect did not occur.

## Verification and justification

The maintenance pilot must produce a reviewable candidate for one seeded reproducible defect without changing the active binary.
Restart/repeated events must not duplicate the task, and self-generated events must not recurse.
Promotion fixtures exercise failed migration, failed startup, failed smoke test, and successful prior-version restoration.
The local non-coding pilot is cancelled mid-operation and leaves no owned process or promoted partial artifact.

This makes self-improvement useful while leaving supervision and recovery deterministic.
Hermes-style reusable memory/skills are a reference for candidate improvement, not proof of safe supervisor replacement.

## Existing decisions affected

The original map's self-modification and migration fog becomes this bounded proposal after approval.
[The tool verb](../../farseer/issues/38-the-tool-verb.md) keeps its no-general-gateway ruling; only the duplicated shell declaration and honest presentation need resolution.
Memory promotion and raw transcript custody retain their existing authority rules.

## Revisit condition

Consider automatic promotion only after repeated successful rollback drills and an explicitly narrowed change category.
Generalize an integration manifest only after a second real integration demonstrates the same varying interface.
