# Store: SQLite edge tables and CTEs, or an embedded graph engine?

Type: research
Status: closed
Blocked by: none

## Question

Decide the graph substrate, informed by the record scope decision and the operator's real analytics questions.

- At the honest target scale (thousands of tasks, tens of thousands of events), do recursive CTEs over SQLite edge tables answer every query from ticket `11`?
- If not, what is the cheapest engine that does? LadybugDB is the live successor after Kuzu was archived in October 2025 and its team acquired.
- Single-writer is the recurring constraint for SQLite and most embedded graph engines. Confirm the one-owning-writer design holds under a realistic worker fleet.
- Is DuckDB over exported Parquet the right answer for the operator analytics half, keeping the live store small?
- Windows install risk: anything needing build tools or native Python wheels is a recurring failure and counts heavily against.

Bench, do not speculate. Produce numbers on the actual query shapes.

## Carried from 02

`02 Record scope` closed and constrains this heavily. The store must support:

- **One append-only log**, cell-scoped by visibility rather than by file.
- **Cheap `WHERE seq > X ORDER BY seq` range scans.** This is the cursor `16` and `07` both depend on, so it is a hard requirement rather than a nice-to-have.
- **`event_id` as UUIDv7 plus `seq` as a monotonic integer.** UUIDv7 alone is only k-sortable and cannot carry the cursor.
- **Three memory tiers** - global, cell-local, run-local - with the reader's definition controlling cross-cell access.
- **Attachments stored out of band**, referenced by pointer, never inline.
- **A tiering path that a cursor read may span without exposing the seam.** Old events moving to Parquet must not break a range scan that starts before the boundary and ends after it.

Note the shape this implies: heavy sequential append, range scans by integer, occasional point lookups, and one analytical tier.
That is not a general-purpose database workload, and choosing as if it were is how this decision goes wrong.

## Carried from 11

`11` cut the analytics surface down to **three entities, two edge kinds, four queries**.

- `run` - `cost`, `tokens`, `runner`, `model`, `cell_id`, `outcome`, `ts`
- `task` - groups runs
- `memory` - with a `consulted` edge back to `run`
- Edges: `run -> re-scoped-from -> run`, and `run -> consulted -> memory`

**That is a strong argument against reaching for a graph database.**
Two edge kinds over three entities is a join, not a graph problem.

The heavy workload is still `02`'s: sequential append, and `WHERE seq > X ORDER BY seq` range scans that may span a tiering boundary.
The analytical workload is four aggregate queries a human runs occasionally.
Optimise for the first and make sure the second is merely possible.

## Resolution

Resolved 2026-08-23 by benchmark, as the ticket required.
Bench code: [spikes/storebench](../spikes/storebench/README.md).
Environment: `x86_64-pc-windows-msvc`, `rusqlite` with the `bundled` feature, WAL, `synchronous = NORMAL`.

### Verdict

**SQLite. Not close.**

No embedded graph engine, no DuckDB in v1, no second store.

### The numbers

Two scales. "10x" and "100x" are relative to the ticket's honest target of thousands of tasks and tens of thousands of events.

| | 10x (200k events) | 100x (2M events) |
| --- | --- | --- |
| db size | 28.7 MB | 291.1 MB |
| bytes per event | 144 | 146 |
| append, batched | 319k events/sec | 308k events/sec |
| append, one event one commit | p50 22.6us, p99 201us | p50 23.3us, p99 178us |
| **cursor scan p50** | **131us** | **227us** |
| **cursor scan p99** | **299us** | **425us** |
| cursor scan max | 390us | 523us |

Cursor scan is `SELECT ... WHERE seq > ? ORDER BY seq LIMIT 500`, sampled over 1000 random cursor positions.

**A 10x increase in data moved the cursor p99 from 299us to 425us.**
That is the whole argument. `seq` is `INTEGER PRIMARY KEY`, therefore the rowid, so a range scan is a b-tree seek plus a sequential walk of 500 rows. The seek is logarithmic and the walk is fixed, so the hot path is effectively scale-invariant.

### The analytics queries, cold

All four from `11`, one run each, no cache warming.

| Query | 10x | 100x |
| --- | --- | --- |
| Q1 cost per successful run, by runner and model | 11.9ms | 222ms |
| Q2 intervention rate, by cell | 4.7ms | 86ms |
| **Q3 rework depth per chain, RECURSIVE CTE** | **25.6ms** | **790ms** |
| Q4 lessons vs outcome, two joins plus group by | 15.9ms | 206ms |
| full table scan, for reference | 119ms | 1.51s |

Q3 is the one that matters, because it is the query a graph engine would supposedly be needed for: a recursive walk up the `rescoped_from` chain, producing 30,165 chains at 100x scale.

**A recursive CTE answers it in 790ms at 100x the target scale.**
At the actual target it is single-digit milliseconds.

These are queries a human runs occasionally, not a hot loop. Sub-second is not merely acceptable, it is invisible.

### Reader latency under a live writer

This was the real risk to `16`'s event stream: a client tailing the cursor while the runtime appends.

| | p50 | p95 | p99 | max |
| --- | --- | --- | --- | --- |
| idle | 227us | 324us | 425us | 523us |
| **writer appending continuously** | **236us** | **348us** | **478us** | **908us** |

**Roughly 13% degradation at p99.**

Note where the cost landed: the writer slowed to about 2,900 events/sec from its uncontended 43,000, while the reader barely moved.
That is the correct direction for farseer, because `16` already decided a slow client must never slow a worker - and here the inverse also holds, a busy worker does not slow a reader.

### On the single-writer constraint

The ticket asked whether one-owning-writer holds under a realistic worker fleet.

**It holds by construction, and it is not a compromise.**

`02` gave farseer **one physical append-only log** with cell-scoped visibility.
Workers do not write to it. Workers emit events to the runtime, and the runtime writes.
So the fleet is many producers into one process, and one process into one writer - which is exactly what SQLite WAL wants: one writer, many concurrent readers.

An architecture needing N concurrent writers would have been a reason to look elsewhere. Farseer does not have one.

### Why not a graph engine

The landscape question in the ticket was live and is now settled.

**Kuzu was archived on 2025-10-10 without warning**, and a European Commission filing later confirmed Apple acqui-hired the team. Open source development stopped: no features, no fixes, no community support.

**LadybugDB is the healthy successor** - a community fork with over 1,000 commits, 20+ releases and 80+ contributors, aiming to be a graph lakehouse interoperating with DuckDB storage and Arrow/Parquet.

So the option is real. It is still the wrong choice, for a reason that has nothing to do with its quality:

**`11` cut the graph to three entities and two edge kinds.**
Two edge kinds is a join. The recursive CTE benchmarked above is the entire graph workload, and it costs 790ms at a scale farseer will not reach.

Adopting a graph engine would mean a second store, a second query language, a second failure mode and a second thing to keep alive - to make a 790ms query faster, when nobody is waiting on it.

There is also a governance lesson worth carrying: **the leading embedded graph engine was archived with no warning and no successor from its owner.** Farseer is a single binary with no required external services, and taking a dependency in a category that just demonstrated that failure mode is a poor trade for a query nobody is waiting on.

### Why not DuckDB over Parquet in v1

Deferred, deliberately, and the design hook stays.

At 100x scale a **full table scan is 1.51 seconds**, and none of the four real queries is a full scan.
DuckDB would be solving a problem farseer does not have yet, at the cost of a second engine and an export pipeline.

What survives from `02`: the tiering path must exist **as a design**, so a cursor read can span a boundary without exposing the seam. Concretely that means old events move by a `seq` boundary and the read path knows how to stitch. Building the mover is not v1 work; foreclosing it would be a mistake.

Revisit when a real query becomes slow, not before.

### Windows install risk

The ticket weighted this heavily, and it is worth reporting because it is a real result rather than a prediction.

`rusqlite` with the `bundled` feature **compiles SQLite from C source using the MSVC toolchain from `19`**, with the two components already installed - the linker and the Windows SDK. It built clean with no additional install, no Python, no native wheel, no vendored binary.

That is the strongest practical argument of all: **the store adds zero to the install surface.**
Any engine requiring a separate native artefact would be adding exactly the class of Windows failure `BRIEF.md` catalogues, to fix a query nobody is waiting on.

### Schema as benched

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE events (
    seq       INTEGER PRIMARY KEY,   -- the cursor; rowid, so range scans are b-tree walks
    event_id  BLOB NOT NULL,         -- UUIDv7, the portable identity
    ts        INTEGER NOT NULL,
    cell_id   INTEGER NOT NULL,
    run_id    INTEGER NOT NULL,
    kind      TEXT NOT NULL,
    actor     TEXT NOT NULL,         -- manager / worker / operator / system
    payload   TEXT NOT NULL
);
CREATE INDEX events_run ON events(run_id, seq);
```

Plus `runs`, `memories`, and the two edge tables `consulted` and `rescoped_from`.

Two notes on the shape.

`seq` as `INTEGER PRIMARY KEY` is the single most important decision here, and it is why `02`'s insistence on a monotonic integer alongside UUIDv7 was correct. A UUID primary key would have made the cursor a secondary-index lookup and given up the sequential locality that makes this fast.

`payload` is `TEXT` holding JSON. SQLite's JSON functions can index into it if a query ever needs to, which keeps `02`'s per-kind payload versioning free.

### Tickets this informs

- `17 cell lifecycle` - purge is a `DELETE FROM events WHERE cell_id = ?`, which leaves the `seq` sequence with holes. Cursor reads must tolerate gaps in `seq`, not assume contiguity. `16` already emits a `gap` event, so the machinery exists, but the semantics differ: a purge gap is permanent, a backpressure gap is recoverable. Decide whether the reader can tell them apart.
- `02 record scope` - confirmed viable as specified, with one addition: **`seq` is not contiguous after a purge**, so nothing may infer "how many events happened" from a `seq` delta.
