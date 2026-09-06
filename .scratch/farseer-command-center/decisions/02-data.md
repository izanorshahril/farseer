# Bounded record views and optional analysis

**Parent:** [Command-center decision map](../map.md).
**Label:** wayfinder:grilling.
**Status:** recommendation selected, awaiting the single package review.
**Selection authority:** delegated recommendation selection, not a recorded human interview.
**Blocked by:** none.

## Question

How should boards, sessions, graphs, and similarity grow without slowing authoritative execution?

## Nominees

| Nominee | Execution isolation | Complexity | Useful first result | Decision |
| --- | --- | --- | --- | --- |
| Bounded SQLite projections and separately scheduled analysis | Short writer-lock work; removable analysis | Extends existing system | Paged board and scoped graph | Selected |
| Fetch everything and optimize rendering | Does not reduce backend work | Superficially small | Fails as record grows | Rejected |
| Dedicated graph/vector database now | Adds another operational dependency | Migration and synchronization burden | No demonstrated need | Deferred |
| Remote embeddings as default | Adds latency, cost, and data transmission | Provider/key lifecycle | Changes local-first assumptions | Deferred |

## Proposed resolution

SQLite remains authoritative for tasks, runs, observations, and edges.
Views are projections of those rows; global and project boards never store competing task states.
Observed topology and derived similarity remain distinct queries and visual layers.

Add paged interfaces without changing the shape or meaning of existing v1 array responses.
Cursors are opaque, bound to the query scope and ordering, and rejected when reused under different filters.
Use stable keyset ordering with an identity tiebreaker.
List pagination is eventually consistent under concurrent task transitions; the response reports freshness and the UI can refresh.
Do not promise a historical snapshot unless a later implementation actually retains one.

| Limit | Proposed initial default | Upper bound |
| --- | --- | --- |
| Task, conversation, or event page | 100 rows | 500 rows |
| Graph neighborhood | 100 nodes, 300 edges | 500 nodes, 1,500 edges |
| Structured response | Within 1 MiB | 1 MiB, excluding separately streamed transcript bytes |
| Analysis concurrency | One active job | One active job and 32 pending requests |
| Transcript source | At most 16 MiB | Enforced while reading, including files that grow |
| Lexical comparison | At most 200 candidates; retain top 20 qualifying edges | At most 32 MiB of total textual input per job |

These are tunable design defaults, not measured performance claims.
Responses expose continuation and truncation; a limited count is never labelled the global total.
A graph response contains only edges whose endpoint identities are included or explicitly represented as outside the visible neighborhood.
Time, project, task, and edge-type filters execute server-side.
The compact Work board does not load a graph in the background.

Tokenization and scoring happen outside the shared store lock.
Read bounded inputs, compute, then validate source existence, digest, and projection version before a short commit.
The first increment may compare bounded excerpts from a bounded recent candidate set using existing tables; it must label that restricted coverage.
It does not require a new schema or pretend to search the whole archive.
Queue saturation returns an actionable busy/pending state rather than silently discarding work or spawning unlimited jobs.
Disabling analysis leaves authoritative reads, run admission, cancellation, and finalization available.

The existing hash term-frequency model is labelled lexical similarity.
Semantic embeddings, external exporters, and richer retrieval are later interchangeable analysis capabilities after relevance evaluation.
Raw transcript custody remains reference, copy, or copy-plus-index; missing source paths remain missing.
Deletion or purge invalidates derived references according to existing record policy.

## Verification and justification

Use seeded local fixtures of 10,000 tasks, 50,000 runs, and 1,000 attachment associations, with representative bounded transcript bodies.
Assert response limits, cursor behavior, and correctness rather than a benchmark result invented in advance.
Measure cancel/status/finalization latency with and without analysis on the same host; the initial target is no more than 20% p95 regression, reported with the sample size and baseline.
A failed target blocks performance acceptance and triggers profiling, not an automatic database replacement.
Use deadline-based checks with documented tolerance for functional tests; keep statistical benchmarks separate from ordinary CI pass/fail timing.

This addresses Q3-Q5 in the [review](../../../PROJECT_REVIEW_2026-09-06.md) without changing the storage decision.

## Existing decisions affected

[Work and session explorer](../../farseer/issues/40-work-and-session-explorer.md) receives the bounded-query and lexical-coverage clarification after approval.
The additive interface and one owning writer promises remain intact.

## Revisit condition

Add a different query/index implementation only after a reproducible representative query misses its target after bounded access and profiling.
Add semantic retrieval only after a judged local corpus shows that lexical results miss a useful operator task.
