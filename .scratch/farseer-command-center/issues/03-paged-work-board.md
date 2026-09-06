# 03: Paged work board

**Parent:** [Command-center decision map](../map.md).

**What to build:** Make global and project kanban views read durable work through bounded projections that remain usable as farseer monitors itself and many projects.

**Blocked by:** None; package approved.

**Status:** implemented.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R02, R03; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/02-data.md).

**Contract choices:** Kanban is a projection over one task set.
Global and project boards share task facts and differ only by scope.
Task state stays independent from run lifecycle, using the existing validated transitions and actor/reason provenance.
Use additive paged reads with the selected limits and scope-bound keyset cursors; legacy array response shapes remain unchanged.

- [x] Global board and one project board show identical task facts under different scopes.
- [x] Board reads are cursor or page bounded, return stable ordering and continuation metadata, and never request all history by default.
- [x] Existing command transitions still validate allowed state changes and record actor/reason; paging and refresh cannot overwrite a more recently selected task.
- [x] A task with several sequential or concurrent runs remains one board card with visible run summary.
- [x] Empty, loading, stale, and failed reads have compact structured states with retry while preserving the last good projection.

**Exclusions:** No independent per-project task store, task deletion, inferred task state from process liveness, or graph rendering.

**Test seam/demo:** Seed tasks across two projects, move one through the board, refresh from a second client, and verify the same event-backed state and bounded request sizes.

## Evidence

Implemented in `crates/farseer-store/src/work.rs`, `crates/farseer-api/src/work.rs`, and `ui/src/widgets/work.tsx`.
The additive `/v1/tasks/page` endpoint defaults to 100 rows, caps at 500, uses a scope-bound keyset cursor, returns continuation metadata, and enforces the 1 MiB structured response cap.
Each card now includes an additive `run_summary` with total runs, active runs, and the latest terminal outcome, so repeated and delegated runs remain one task card.
The store verifies that a task has identical facts in global and project projections, and the API fixture verifies the same contract through global and project routes.
The API also verifies bounded continuation, scope-bound cursor refusal, transition provenance, and parent/child run summaries.
The Work board now preserves its last projection while refreshing, distinguishes initial loading and empty states, and only labels a failed read stale when a prior projection exists.
Store, API, and UI focused checks pass.
