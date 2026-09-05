# Cell lifecycle: pause, resume, archive, delete

Type: grilling
Status: closed
Blocked by: none

## Question

Graduated from the map's fog once `01 Is the cell the right primitive?` confirmed the primitive and split **cell definition** from **running cell**.
That split is exactly what makes this question answerable, and exactly what makes it non-trivial: the two have different lifecycles and can be operated on independently.

- What does pausing a running cell mean? The manager stops taking new work, or in-flight workers are suspended too. Suspending a running process on Windows is not the same as it not being scheduled.
- Resume after a farseer restart: does a running cell survive the runtime dying, or is every in-flight worker lost and the manager restarted from the record?
- Editing a cell definition while its running cell has workers in flight. Does the running cell pin the definition version it started with, or pick up changes live?
- Archive versus delete. Archive presumably keeps the record and the definition. Delete of what: the definition, the running state, the record slice, or all three.
- Deleting a cell that other cells have called, and whose calls appear in their records. The record must stay readable after the cell is gone.
- Can the last cell be deleted? `ARCHITECTURE.md` requires the runtime to run with exactly one cell, so cell #0 may be undeletable, which needs stating.
- Where does definition versioning and rollback live: plain git on the definition file, or something farseer owns?

## Carried from 16

The API exposes **read**, **validate** and **reload** for cell definitions, but no edit path - definitions stay files in git.
So definition version pinning has a live surface to act on, and this ticket should say what `reload` does to a **running** cell.

Specifically: does reload affect in-flight runs, only the next run, or is it refused while any run is in flight?
Note that `05` made the worker contract **immutable** for the life of a run, which argues that a reload must not reach into a run already executing.

## Carried from 06

**`cell_id` must be stable across a definition reload**, or the record loses its join key and history detaches from the cell that produced it.
That constrains where the id can live: not derived from the definition content, since the content is what changes.

## Carried from 02

**Delete removes the cell, not the record.** Deleting a cell removes the running cell and the definition binding only.
The asymmetry is deliberate: a definition is a file in git and deleting it is reversible, its history is not.

So this ticket must name and gate a separate **purge** verb, louder than delete, that says what it is destroying.
Open here: can purge be partial (one run, one date range), and does purging a cell's record break the cross-record hints other cells hold pointing into it? `02` made those hints rather than invariants precisely so that it does not.

## Carried from 09

**Purge leaves holes in `seq`.**

Purging a cell's record is `DELETE FROM events WHERE cell_id = ?`, so the sequence is no longer contiguous.

Two consequences this ticket owns:

- Cursor reads must tolerate gaps rather than assume contiguity. `16` already emits a `gap` event, so the machinery exists.
- But the **semantics differ**: a backpressure gap is recoverable by refetching from the record, and a **purge gap is permanent**. Decide whether a reader can tell them apart, because a client that retries forever on a purge gap is a bug that only appears after someone purges.

Also from `09`: nothing may infer "how many events happened" from a `seq` delta.

## Resolution

Resolved 2026-08-23 by grilling.

### 1. Pause means drain, never suspend

**Pausing a running cell means the manager stops starting new runs. In-flight runs continue to completion. No process is ever suspended.**

The ticket asked whether in-flight workers are suspended too, and noted correctly that suspending a process on Windows is not the same as it not being scheduled.

The answer is that farseer should not do it at all.

Suspending an agent mid-API-call corrupts it: the socket times out, the provider's side of the conversation does not pause, and resuming lands in a session that is already broken.
And `03` measured reap at **300 to 400 microseconds**, so if the operator actually wants a worker stopped, **cancel is cheap and honest** where suspend is neither.

The pleasing consequence: pause is a **policy flag on the manager**, not a process operation.
The one place Windows process semantics would have bitten, farseer simply does not go.

### 2. A running cell does not survive a farseer restart, and that is a trade already made

**In-flight runs are lost when the runtime dies. Deliberately.**

`03` chose `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, which means every worker tree dies with the runtime.
That is precisely what prevents the orphan storms `BRIEF.md` catalogues, and it is precisely why in-flight work cannot be resumed.

**Farseer chooses no orphans over run survival.**

The alternative is dropping kill-on-close so workers outlive the runtime. `03` measured what that costs: **five of six processes surviving indefinitely**, including every `node.exe` in the tree, none of them noticing their parent had died.
Not worth it.

On restart, farseer finds runs in lifecycle `running` with no live job and marks them **`finished(failed)` with reason `runtime_restarted`**.

`failed` rather than `cancelled`, per `05`: nobody chose this, and `failed` invites the retry that is actually appropriate.
The manager is rebuilt from the record, and the task the runs belonged to is still there to re-run.

### 3. The run pins the definition, not the cell

**Reload always succeeds. It never reaches into an in-flight run. It applies to the next run.**

`16` gave the API read, validate and reload with no edit path.
The question was whether reload is refused while runs are in flight, applied live, or deferred.

- **Refusing while busy** makes reload unusable exactly when it is most needed - a broken definition on a busy cell.
- **Applying live** violates `05`, which made the worker contract immutable for the life of the run.

So the definition version is pinned **per run**, not per cell.

That is worth stating as a design move rather than a detail: it **reuses `05`'s immutability instead of inventing a versioning concept**.
The contract already froze the goal, the workspace, the runner, the tool grants, the autonomy level, the budget and the definition-of-done. The definition version is one more thing it froze.

Nothing new to build, and no window in which a run is executing against a definition that changed underneath it.

`06`'s constraint holds and is now load-bearing here: **`cell_id` must be stable across a reload**, so it cannot be derived from definition content, because content is what changes.

### 4. Three verbs, increasing in violence

| Verb | Removes | Keeps | Reversible |
| --- | --- | --- | --- |
| **archive** | the running cell | definition, record | yes |
| **delete** | running cell, definition binding | **record** | yes, via git |
| **purge** | the record slice | nothing | **no** |

An archived cell cannot be called. Its definition and history are intact and it can be brought back.

**Delete keeps the record**, per `02`. The asymmetry is deliberate: a definition is a file in git, so deleting it is reversible; its history is not.

**Purge is the only irreversible verb farseer has**, which is why it is separate, louder, and says what it is destroying.

#### Purge is partial

Purge takes a scope: one cell, one date range, or both.
A purge that can only destroy everything cannot serve a retention policy, and retention is the reason purge exists.

#### Cell #0 can be archived but never deleted

`01` made cell #0 the default address the operator talks to.
A runtime with no cell has **no addressable manager and nothing to receive an instruction**.

Promoting another cell on delete was considered and rejected: it is complexity in the most dangerous verb, for no benefit.
Cell #0 is undeletable, full stop.

### 5. A purge gap and a backpressure gap are different events

`09` flagged this as a bug that only appears after someone purges: a client retrying forever on a hole that is never coming back.

**Two distinct events:**

- **`gap`** - recoverable. The client refetches the range from the record. This is `16`'s backpressure case.
- **`void`** - permanent. The data is gone. Stop asking.

**And purge appends a tombstone.**

At the purge point, purge writes a record entry naming the range destroyed, the scope, and when.
So the hole is **explained rather than merely present**.

This matters because `02` made the record **evidence**.
An unexplained hole and a deliberate deletion must not look identical, or the record cannot be trusted about its own completeness.

Two consequences fall out.

**`02`'s dangling cross-record hints get better.** A pointer from cell A into a purged cell B resolves to "purged on 2026-08-23" rather than "unavailable". `02` made those hints rather than invariants precisely so purge could not break them; the tombstone turns a silent dangle into an answer.

**`09`'s rule stands and is now visible.** Nothing may infer how many events happened from a `seq` delta. After a purge the sequence has holes, and the tombstone is what tells a reader why.

### What this ticket did not need

Worth recording, because the ticket listed it as an open question.

**Definition versioning and rollback stay in plain git.** Farseer owns nothing here.

The definition is a file, `16` gave read, validate and reload with no edit path, and this ticket pinned the version per run. Rollback is `git checkout` followed by reload.
Anything farseer built on top would be a second version-control system shadowing the first, and `01` already ruled that a cell is data in git rather than something farseer manages.

### Tickets this informs

- `12 autonomy and deny list` - **purge is the only irreversible verb farseer owns.** `08` made irreversibility a policy dimension for tools; purge is the same dimension pointed inward at farseer's own record, and it should be gated with at least the force of an irreversible tool.
- `13 harness build kit` - a cell definition must carry a **stable `cell_id` that is not derived from its content**, since content changes on every reload and the record's join key must not.

## Amended by 24

**Purge does not reach UI state.**

Purge is defined over the record, and `24` put UI state outside it.
A blob is removed by writing an empty one, which needs no verb of its own.

The consequence is deliberate: a layout may pin a cell that was purged, and farseer will not notice, because it never parses a blob.
That is the UI's problem to skip - the alternative is farseer owing a schema forever.

## Built, 2026-08-31

The verbs are `POST /v1/cells/{id}/{pause,resume,archive,restore,purge,delete}`, plus `GET /v1/cells/states` for everything that has moved.
Delete is a `POST` rather than `DELETE` on the cell because it removes a binding and not a resource - the definition is still in git, which is exactly the asymmetry section 4 draws.

**Lifecycle lives in the store, in a `cell_state` row that exists only for a cell that has moved.**
A deletion held in the loaded registry alone would be undone by the next reload, which re-reads the directory the file never left.
Absent means active, so a fresh store needs no seeding to be correct, and returning to active removes the row rather than writing the word.

**Pause is checked where runs begin**, in all three: the operator's instruction, worker delegation and cell delegation.
A policy flag nothing reads is a switch that does nothing.

**Purge is scoped by `ts`**, inclusive at both ends, over each table's own timestamp - an event's `ts`, a run's `started_ts`, a memory's `ts` - rather than over `seq`, because the operator asking is asking about dates and `seq` is farseer's cursor rather than a clock.
The tombstone is appended after the delete, so its own `ts` is later than any range that purge could have destroyed. A later purge whose range reaches forward over it does remove it; that is honest rather than special-cased, since a tombstone is a record entry and purge is defined over the record.

**One thing this could not finish.** Section 5 asks that a reader be able to tell a `void` from a `gap`.
The kind exists and the tombstone names it, but **farseer emits no `gap` today** - the stream resumes from a cursor and never announces a hole - so there is nothing yet for `void` to be distinguished *from* on the wire.
The distinction becomes real the day `16`'s backpressure gap is emitted; until then it lives in the record, where the tombstone explains the hole to anyone who scans across it.
