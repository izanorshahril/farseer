# Record scope: global with visibility, or private per cell?

Type: grilling
Status: closed
Blocked by: none

## Question

`BRIEF.md` promises one record shared across all harnesses.
`ARCHITECTURE.md` gives each cell a record scope.
These are in tension, and the resolution sets the ID strategy, the query surface and the MCP face.

- One event log with cell-scoped visibility rules, or a log per cell with a federated read?
- May a coding cell read a social media cell's lessons? Should it?
- Which memory is genuinely global (operator preferences, tool gotchas, Windows workarounds) versus cell-local (domain conventions, brand voice)?
- Does a cell keep its record when archived or deleted?
- Per-machine, or syncable later? This fixes UUIDv7 versus autoincrement now, and is expensive to change later.

## Carried from 05

`05` split one concept into two, and the split lands here.

- **progress** - tool call start, tool result, status change. **Goes to the record.**
- **activity** - any bytes from the adapter, including token streams and adapter heartbeats. **Liveness bookkeeping only, never a record entry.**

Activity is high-volume and semantically empty, so recording it would swamp the record to no benefit.
Only `last_activity_at` matters, and that is a single mutable timestamp on the run, not history.

Also inherited: a run is one contract execution and exactly one record entry, and the contract envelope is **immutable**, so the record stores it once rather than as a timeline.
Steering appends a `manager_steered` event; intervention appends `operator_intervened`; the result carries `operator_touched`.

## Carried from 16

The event stream is **explicitly lossy** and the record is its source of truth, so the record has a query requirement and not just a storage one.

**The record must support cursor-based reads from an arbitrary `event_id`.**
A client that falls behind receives a `gap` event with the last contiguous id and refetches the middle from the record, and attach-mid-run replays from a cursor before going live.
Both depend on the record being able to answer "everything after id X, in order" cheaply.

Note this interacts with the tiering question already in the fog: if old events move to Parquet, a cursor read may span both tiers, and the API must not expose that seam.

## Carried from 06

A cell call is a **run in the calling cell**, so it is one record entry there.
The callee cell has its own record, and the two are **not merged** - `01` made record scope a per-cell property.

Open for this ticket: does the caller's entry carry a link to the callee's record, and if so what does that link survive?
A cross-record reference is cheap to write and expensive to guarantee, so decide whether it is a hint or an invariant.

## Resolution

Resolved 2026-08-23 by grilling.

### 1. The tension between `BRIEF.md` and `ARCHITECTURE.md` was false

`BRIEF.md` promises one record shared across all harnesses.
`ARCHITECTURE.md` gives each cell a record scope.
These conflict only if storage and visibility are the same thing, and they are not.

**One physical append-only log, with cell-scoped visibility.**

`16` requires the record to answer "everything after id X, in order" cheaply.
Federating that across N logs means either standing up a global sequencer anyway - one log with extra steps - or giving up total ordering, which breaks the cursor that `16` and `07` both depend on.
One log also means one tiering pipeline, one backup, one integrity story.

**This does not contradict `06`.**
"The callee's record is not merged" is a statement about scope and visibility: the caller's run entry never absorbs the callee's events.
Where the bytes physically live is orthogonal.
Worth stating explicitly, because the two read like a contradiction otherwise.

### 2. The log is not session history, and the difference is load-bearing

This was the operator's question, and it deserves to be written down rather than assumed.

| | the log | session history |
| --- | --- | --- |
| owned by | farseer | the harness |
| contains | run entries, progress events, contract envelopes, results | system prompt, every message, full tool payloads |
| size | small, queryable | enormous |
| secrets | only what farseer chose to record | everything the agent ever read |

`05`'s split already implied this: token streams are **activity** and never recorded, tool calls are **progress** and are.
A transcript is mostly activity.

Conflating them would break the record three ways at once - swamped by volume, coupled to session formats each harness may change at will, and full of unscrubbed secrets.

So the log **references** session history rather than containing it.
That yields a third category alongside events and memory: **attachments**.

### 3. Three categories, and they have different rules

| Category | Written by | Scrubbed | In the queryable record |
| --- | --- | --- | --- |
| **events** | the runtime, from what it observed | yes, on write | yes |
| **memory** | agents, as claims | yes, on write | yes |
| **attachments** | nobody - referenced out of band | **no** | no, pointer only |

### 4. Memory is scoped by kind, not by cell

- **global** - operator preferences, tool gotchas, Windows workarounds. Readable by every cell. **This is what `BRIEF.md` was actually promising**, and it is where nearly all the value sits.
- **cell-local** - domain conventions, brand voice. The default.
- **run-local** - scratch, dies with the run.

Cross-cell reads beyond the global tier are **opt-in via the reader's definition**, never blanket.
A coding cell inheriting brand voice is noise.
A coding cell inheriting "this `.cmd` shim needs explicit resolution on Windows" is the entire point, and that fact came out of `03` rather than out of any one cell's domain.

### 5. Two ids, because one field cannot honestly do both jobs

- **`event_id`: UUIDv7.** Globally unique with no coordinator, time-ordered, survives the syncable-later question and multiple writers. Autoincrement requires a single writer, which farseer has today but which `06`'s A2A endpoint erodes.
- **`seq`: a monotonic per-log integer. This is the cursor.**

The reason both are needed: **UUIDv7 is only k-sortable.**
Two events in the same millisecond have a random tail and therefore no deterministic order, which silently breaks `16`'s "everything after X, in order" exactly when a run is busiest.

`seq` is local and never leaves the machine.
`event_id` is the portable identity.

### 6. An event

`{ seq, event_id, ts, cell_id, run_id, kind, actor, payload }`

`actor` is `manager` / `worker` / `operator` / `system`, and it is the field that is easy to omit and expensive to add later.
`05` and `07` both made provenance load-bearing - `operator_touched`, `manager_steered`, operator-initiated re-scope.
Deriving actor from `kind` works right up until one kind can come from two sources, and at that moment every historical query becomes quietly wrong with no error to notice.

`payload` is kind-specific and independently versioned, so adding a field to one event kind never touches the others.

### 7. The record outlives the cell

**Archiving a cell keeps its record. Deleting a cell also keeps its record.**

Deleting a cell removes the running cell and the definition binding, not the log.

The asymmetry is the whole argument: a cell definition is a file in git, so deleting it is cheap and reversible.
Its history is not reversible, and it is the substrate `11 analytics questions` runs on.

Purging a record is a **separate, louder verb** that names what it is destroying.

### 8. The MCP face: query and memory-write, never raw event append

Agents may search the record and write lessons into the three memory tiers.
Agents may **not** append arbitrary events.

The reason is the record's entire purpose.
**An agent that can forge events can rewrite its own history**, and at that point the record stops being evidence and becomes a story the agent tells about itself.

Events are written by the runtime, from things it observed.
Memory is written by agents, and is marked as **claims** rather than observations.
That distinction should survive into the query surface so a reader can tell them apart.

### 9. Secret scrubbing happens on write

This was sitting in the fog with no owner. It has one now.

**Scrub on the path into the record. Never at read time.**

Read-time scrubbing means the secrets are on disk and one query bug away from exposure.
Write-time means the record never holds them.

The cost is that over-matching is irreversible.
Accept it: **a false positive loses one field, a false negative leaks a key that is now in every backup.**

The corollary must be stated plainly: **attachments are not scrubbed.**
That is exactly why farseer does not take custody of them by default.

### 10. Attachments: reference always, custody on request

- The **transcript pointer** is always recorded. A path costs nothing and answers "where do I look" months later.
- **Copying the transcript into farseer's store is opt-in.** Custody means storing megabytes of unscrubbed secrets per run, by default, forever.
- **Raw PTY casts are off by default**, opt-in per cell. Genuinely useful forensics, genuinely niche.

Consistent with the cross-record link below: the pointer is a **hint**.
Harnesses rotate and garbage-collect their own session files, so it will sometimes dangle.
That is an accepted degradation, not something to guarantee.

This resolves the map's fog entry **"Raw PTY stream as a forensic attachment"**, which was explicitly addressed to this ticket.
The answer generalised: it is not a special case for PTY, it is the attachment category, and harness transcripts use the same slot.

### 11. The cross-record link from `06` is a hint

Store `callee_cell_id` and `callee_run_id` on the caller's entry. Guarantee nothing about resolving them.

An invariant would mean referential integrity across scopes that can be purged independently, retention-tiered differently, and later live on separate machines.
A hint that resolves almost always and degrades to "callee record unavailable" is honest.
**An invariant that cannot be enforced is a lie that surfaces later as a crash.**

### Tickets this informs

- `09 store decision` - now unblocked, and heavily constrained. It needs: one append-only log, cheap `WHERE seq > X ORDER BY seq` range scans, UUIDv7 plus monotonic integer, three memory tiers, out-of-band attachment storage, and a tiering path that a cursor read may span without exposing the seam.
- `11 analytics questions` - the record outlives deleted cells, so historical questions can span cells that no longer exist. Note that `cell_id` may not resolve to a live definition.
- `17 cell lifecycle` - delete removes the cell, not the record. Purge is a separate verb this ticket should name and gate.
- `12 autonomy and deny list` - the deny list governs what a worker may do; scrubbing governs what the record may keep. Related but not the same list, and they should not be merged.

## Carried from 11

One new event kind: **`memory_consulted`**, emitted when a worker reads memory through the MCP face.
It carries the `run` and the `memory` id, and it is what makes `11`'s fourth question - which lessons actually reduced failure rate - answerable at all.

This is a new event kind, not a new category. Memory reads are runtime observations, so they are events rather than claims.

## Amended 2026-08-23: `context_compacted`

Findings: [context-compaction.md](../research/context-compaction.md).

One more event kind: **`context_compacted`**, carrying the trigger (`auto` or `manual`) and, where the harness supplies them, token counts either side.
Claude Code already emits this as a `system` event with subtype `compact_boundary`.

It is a **progress** event by this ticket's definition - a runtime-observable status change, small and semantically meaningful.

Two reasons it earns its place.

Compaction is **invisible in the artifact and highly visible in the outcome**: a run that quietly lost half its context and then produced something odd is otherwise indistinguishable from a model having a bad day.

And it reinforces this ticket's core distinction. **The log is not session history**, and compaction makes that sharper: OpenAI's server-side compaction returns an **AES-encrypted blob farseer can never read**, so even the harness's own transcript is a lossy artifact of a process farseer does not control.
Farseer can record **that** a compaction happened and **when**. It can never record **what was dropped**. State that limit rather than implying completeness.

## Confirmed by 09

This ticket's requirements were benchmarked and hold. See [09](09-store-decision.md) and [spikes/storebench](../spikes/storebench/README.md).

The insistence on **`seq` as a monotonic integer alongside UUIDv7** turned out to be the load-bearing decision.
`seq` becomes `INTEGER PRIMARY KEY`, therefore the rowid, so a cursor range scan is a b-tree seek plus a sequential walk - **p99 425us at 100x the target scale**.
A UUID primary key would have made the cursor a secondary-index lookup and given up that locality.

One addition: **`seq` is not contiguous after a purge**, so nothing may infer "how many events happened" from a `seq` delta.

## Amended by 24

**UI state is a fourth category, and it is explicitly not the record.**

This resolution gave three categories - events, memory, attachments.
`24` added a place for a canvas layout and widget arrangement, which is none of those: not a thing that happened, so the append-only log is the wrong home.

It differs from all three on every rule this ticket set: **mutable, last-write-wins, no `seq`, no scrub, and no event emitted on write** - a cursor drag is not history, and logging one would flood the log.

It is the **second unscrubbed thing** after attachments, and for the same reason: farseer cannot scrub what it will not read.

Named here so nobody later mistakes it for a memory tier.

## Amended by 14

This ticket closed before `14 Vocabulary and naming lock`.

Where this resolution says **"contract envelope"** - in what a run entry contains, and in the record-versus-transcript table - the term is **worker contract**.
`14` retired `envelope` as a noun because two tickets used it for two different payloads.
