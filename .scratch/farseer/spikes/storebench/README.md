# storebench

Bench for `09 Store: SQLite edge tables and CTEs, or an embedded graph engine?`.

The ticket said "bench, do not speculate", so this builds the exact record shape `02` and `11` fixed, at 10x and 100x the honest target scale, and times the two workloads that actually matter.

- **Hot path**: append, and `WHERE seq > X ORDER BY seq LIMIT 500` cursor reads. This is `16`'s event stream and `07`'s attach-mid-run.
- **Cold path**: the four analytics queries from `11`, including a recursive CTE over the rework chain - the query a graph engine would supposedly be needed for.
- **Contention**: cursor reads while a writer appends continuously, which is the real shape of a client tailing a live run.

## Run

```
cargo run --release          # 10x target: 10k tasks, 20k runs, 200k events
cargo run --release -- 10    # 100x target: 100k tasks, 200k runs, 2M events
```

Writes `bench.db` in the crate directory - 291 MB at 100x. Delete it when done.

## Notes

`seq` is `INTEGER PRIMARY KEY`, so it is the rowid. A range scan on it is a b-tree walk with no secondary index to maintain, which is why the cursor is scale-invariant.

The RNG is a deterministic xorshift rather than a dependency, so runs are comparable and the crate has exactly two dependencies.

Results are recorded on the ticket, not here.
