# Where does UI state persist?

Type: grilling
Status: closed
Blocked by: none

## Question

Raised by the operator on 2026-08-23, against a paragraph on the map that got it wrong.

The operator wants a command center: threads, a canvas, and widgets for utilisation, cost and a kanban board.
**On restart it must come back the way it was left, not vanilla.**

The map initially said UI state "has no home in the record, and should not get one", which conflated two separate claims.
The first is right: a canvas layout is not a thing that happened, so `02`'s append-only log is the wrong home.
The second does not follow. Not being in the record is no reason to be ephemeral.

So: where does it live, and who owns it?

### What makes this non-trivial

- `01` made the runtime **headless**, and a runtime that parses layouts is not headless any more.
- `16` put `/v1/` under an **additive-only** promise, so whatever surface this gets is permanent.
- `02` gave the record three categories with different rules, and this is none of them.
- `17` made **purge** the only irreversible verb farseer owns, and it is defined over the record.

## Resolution

Resolved 2026-08-23 by grilling.

### 1. Farseer stores it, as an opaque blob, in a table separate from the event log

Two verbs on one key: `GET` and `PUT /v1/ui-state/{key}`.
Value is bytes. **Farseer never parses it.**

Mutable, last-write-wins, no `seq`, no history, no event emitted on write.
A write is not a thing that happened, and emitting one would flood the log with cursor drags.

#### Why farseer rather than the UI

The obvious answer is `localStorage`, and it fails on three counts the operator will actually hit:

- **Per-origin, per-browser.** A Tauri window and a browser tab would not share a layout, and `01` expects UI variations to be swapped one at a time.
- **Clearing site data destroys the dashboard**, with no warning that it was the store.
- **It is not backed up.** Farseer's data directory is. Whatever protects the record protects the layout for free.

#### Why SQLite rather than files

`09` already put `rusqlite` with `bundled` in the binary, so a table adds **zero install surface** and a file tree adds path escaping, torn writes and a directory to sweep.

Farseer is the single writer per `02`, so the write path is already solved.

### 2. Opaque means opaque

**Farseer never validates the contents, and never follows a reference inside them.**

A layout pinning a cell that was later deleted or purged is **the UI's problem to skip**.

That is the price, and it is the right one.
The moment farseer knows a blob contains a `cell_id` it must keep that knowledge working forever, and an opaque blob has become a schema under `16`'s additive-only promise.

Consequences, all of them deliberate:

- **`17`'s purge does not touch it.** Purge is defined over the record, and this is not the record. A blob is removed by writing an empty one.
- **`02`'s scrub does not apply.** Scrub is on write, for events and memory. This is the **second unscrubbed thing** after attachments, and for the same reason: farseer cannot scrub what it will not read. A UI must not put secrets in it.
- **`11` excludes it.** It is not evidence and must never reach an analytics query.

### 3. Two invariants, because opacity without limits is a hole

- **A size cap per key**, 1 MiB, rejected with `413`. Without one, a UI eventually stores a base64 screenshot and the operator's data directory grows without explanation.
- **A length cap on the key**, and the key is a **TEXT PRIMARY KEY treated as an opaque string** - never a path, never split on a separator by farseer. Namespacing is the UI's convention, not farseer's parsing.

### 4. No concurrency control, and that is a citation not a shortcut

Last-write-wins with no ETag and no conditional write.

`01` ruled **concurrent clients on one live session** out of scope: the operator swaps UI variations one at a time rather than running them side by side.
A lost update requires two writers, and this map already decided there is one.

If that is ever revisited, a conditional write is additive and `16`'s promise permits it.

### What this does not change

**Everything about runs, cells and cost stays reconstructible from a query plus a cursor.**

This blob holds view preference only: layout, arrangement, collapsed state, which widgets the operator wants.
The moment real data lands in it, replay breaks and a UI becomes the owner of something the record should hold.

That line is the whole reason this ticket has a size cap rather than a schema.

### Tickets this informs

- `16 local API surface` - **two new operations** under the additive-only promise, `GET` and `PUT /v1/ui-state/{key}`. No parsing, no validation, `413` over the cap.
- `02 record scope` - **UI state is a fourth category and explicitly not the record**: mutable, unscrubbed, no `seq`, no scrub, no event on write. Named here so nobody later mistakes it for a memory tier.
- `11 analytics questions` - excluded from every query.
- `17 cell lifecycle` - **purge does not reach it**, because purge is defined over the record.
