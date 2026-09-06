# 04: Scoped orchestration graph

**Parent:** [Command-center decision map](../map.md).

**What to build:** Provide a bounded graph projection for project, conversation, task, run, session reference, delegation, cell call, continuation, and rescope relationships, with observed and derived edges kept distinct.

**Blocked by:** [Paged work board](03-paged-work-board.md)

**Status:** implemented.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R02, R06; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/02-data.md).

**Contract choices:** Keep SQLite as truth and use additive read interfaces.
Observed topology and derived similarity are separate edge classes.
No graph database or all-document query runs under the writer lock.

- [x] A graph query accepts scope, time range, entity kind, and page or edge budget, and returns continuation metadata.
- [x] Observed edges identify their source event or run relation, while derived edges carry projection, score, and evidence metadata.
- [x] Selecting a node opens the existing conversation, task, run, or attachment face rather than a graph-only duplicate.
- [x] The UI supports zoom or pan, filters by project and runner, and renders a bounded graph without freezing the canvas.
- [x] Empty, oversized, and unavailable projections explain what is missing and provide a retry or narrower-scope action.

## Evidence

Implemented in `crates/farseer-store/src/work.rs`, `crates/farseer-api/src/work.rs`, and `ui/src/widgets/work.tsx`.
The store test suite covers bounded graph projection, and the workspace check, clippy, format, UI check, and full test suite pass.

**Exclusions:** No semantic embedding service, graph database, raw transcript ingestion, or graph-only domain entity.

**Test seam/demo:** Generate a manager-to-worker and cross-cell chain, query one project and one task scope, and verify topology and derived edges remain visually and semantically separate.
