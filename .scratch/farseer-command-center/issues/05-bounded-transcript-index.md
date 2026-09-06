# 05: Bounded transcript projection

**Parent:** [Command-center decision map](../map.md).
**What to build:** Make existing transcript attachment/index operations bounded and independently recoverable, with visible pending, complete, truncated, and failed states.
**Blocked by:** None; package approved.
**Status:** partial.
**Execution:** package approved; verify named blockers before implementation.
**Review refs:** R02, R06; R13 applies as targeted cleanup.
**Decision:** [Bounded record views and optional analysis](../decisions/02-data.md).

## Contract choices

Preserve existing reference/copy/copy-plus-index behavior rather than rebuilding custody.
Read at most 16 MiB per source with an enforced streaming cap, including when a file grows.
Use one analysis worker, queue 32, at most 200 candidates, top 20 qualifying edges, and a 32 MiB total textual input budget.
Bounded excerpts and recent candidates are permitted in this first increment and labelled as restricted coverage.
Read inputs briefly under the store guard, compute outside it, then validate source existence/digest/version before committing.
Use existing tables; a new schema is not a prerequisite to bounding this path.
Retried or purged source work must not resurrect stale derived edges.

## Acceptance criteria

- [ ] Reference copies/indexes no bytes, copy indexes nothing, and copy-plus-index returns only scrubbed derived text through search.
- [ ] Oversized/growing sources, full queue, candidate truncation, and missing files produce explicit operator-visible states without an unbounded read or task explosion.
- [ ] Concurrent analysis leaves cancel/status/finalization working; a stale or purged source cannot commit derived results.
- [ ] Existing Work attachment controls display pending/failure/retry outcomes, while task and event history remain usable if analysis is disabled.
- [ ] A fixture containing secrets exposes only scrubbed indexed excerpts with custody, projection version, and coverage labels.

**Exclusions:** Semantic embeddings, remote transcript services, automatic discovery of private harness logs, new storage engines, and general retention tiering.
**Test seam/demo:** Attach fixture files through the existing public command, inspect Work's states, saturate the bounded queue, and cancel an unrelated deterministic run during analysis.

## Evidence

Implemented in `crates/farseer-api/src/work.rs`, `crates/farseer-store/src/work.rs`, and `crates/farseer-store/src/lifecycle.rs`.
Reads enforce the 16 MiB cap while streaming, candidate rows and text are bounded, similarity is capped to the top 20 positive edges, source digests are rechecked before commit, and purge removes orphaned derived data.
Focused bounded-candidate, projection-commit, store, format, and API checks pass.

## Completion correction, 2026-09-06

The earlier implemented label covered bounded reads and projection only.
Background queueing and operator-visible pending/complete/failed analysis states remain unimplemented; finish and verify those acceptance criteria before closure.
