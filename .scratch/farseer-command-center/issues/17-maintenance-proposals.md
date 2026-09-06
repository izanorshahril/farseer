# 17: Bounded maintenance source proposals

**Parent:** [Command-center decision map](../map.md).
**What to build:** Let Farseer maintain its own repository as an ordinary project by creating an isolated candidate change with reproduction and validation evidence.
**Blocked by:** [Independent runtime lifecycle](02-independent-runtime.md), [Project team profiles](14-project-teams.md).
**Status:** bounded worker implemented; promotion remains a separate ticket.
**Execution:** package approved; verify named blockers before implementation.
**Review refs:** R11; R13 applies as targeted cleanup.
**Decision:** [Self-maintenance and domain integration](../decisions/05-maintenance.md).

## Contract choices

Reuse tasks, runs, the maintenance cell, worktrees, and artifact metadata; do not add a competing proposal/task engine.
The first trigger is an explicit operator request or deduplicated seeded failure observation.
Initially allow one active maintenance task and zero automatic retries.
Persist trigger identity and suppress events from the same maintenance lineage so restarts/log output do not recursively schedule work.
A permitted maintenance worker can edit its isolated source workspace, leave a candidate branch, and run the repository's validation.
It cannot alter the active installation, grant itself authority, or infer promotion permission from permission to edit.
Normal project execution remains possible while maintenance is disabled.

## Acceptance criteria

- [x] A seeded reproducible issue becomes one ordinary maintenance task with trigger, actor, source revision, scope, and bounded attempt evidence.
- [x] The worker creates a candidate source artifact/branch plus reproducer and validation results; the active runtime remains on its prior version.
- [x] Duplicate triggers, restart, and self-generated events preserve one lineage and never create an unbounded task loop.
- [x] Work detail exposes the candidate, validation outcome, and previous revision using existing task/run/artifact views.
- [x] Failure or cancellation leaves an inspectable outcome and no promotion; disabling maintenance leaves unrelated work usable.

**Exclusions:** New proposal status engines, automatic merge/promotion, credential changes, and permanent background LLM reasoning.
**Test seam/demo:** A deterministic runner fixture produces a candidate edit in an isolated test repository; inspect the evidence and repeat the trigger without creating another task.

## Evidence

`farseer-store::maintenance::ProposalLedger` now persists proposal metadata, trigger and lineage identity, one bounded attempt, candidate source metadata, and validation evidence.
Duplicate triggers return the existing proposal, a different trigger is refused while one proposal is open, and self-lineage events are suppressible after reload through the JSON ledger.
The operator API now admits a proposal as an ordinary in-progress conversation/task, links the proposal to that task, records one candidate/reproducer/branch and validation attempt as an ordinary run/artifact, and supports explicit cancellation.
The public regression test covers deduplication, task linkage, review transition, the artifact projection, and the task-detail proposal projection.
The task detail now joins the bounded proposal ledger to the ordinary task view and exposes source/previous revisions, candidate metadata, and validation evidence.
Successful evidence is refused unless it carries at least one validation result, and the store repeats that invariant before marking a proposal succeeded.
Validation rows also require non-empty commands and outcomes before they enter the ledger.
Cancellation now records a cancelled maintenance run, artifact, and terminal event through the ordinary task detail projection, without creating a candidate or promotion.
`POST /v1/maintenance/proposals/{id}/execute` runs one deterministic local worker.
It resolves the source revision to a commit, creates a proposal-derived `farseer/maintenance/<id>` branch with an isolated git worktree under the runtime runs directory, writes a candidate and reproducer file, and records `git diff --check`, `cargo fmt --all -- --check`, and the bounded store maintenance regression when the candidate is a Rust workspace.
Non-Rust fixture repositories receive a structural validation row rather than a fabricated cargo result.
Failed setup removes the partial worktree and branch, while the task/run/artifact rows retain the failure and validation detail.
The candidate worktree remains available for review, while the active checkout and runtime revision stay unchanged.
The API regression removes the branch after asserting the retained files and the unchanged active `HEAD`.
