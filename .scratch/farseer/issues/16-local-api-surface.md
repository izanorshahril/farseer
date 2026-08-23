# What is the local API surface?

Type: grilling
Status: closed
Blocked by: none

## Question

`01 Is the cell the right primitive?` decided the runtime is headless from v1: one local API on `127.0.0.1` over HTTP with a token, live output over Server-Sent Events, CLI and any UI as equal clients.
That decided the boundary exists and how it is reached.
It did not decide what crosses it.

- What is the minimum set of operations? Candidates: list cells, start a cell, send an instruction to a manager, list running workers, attach to a worker, send input to an attached worker, cancel a worker, approve or reject a gated action, read the record.
- Which of these are commands and which are subscriptions? Attach is clearly a subscription. Is an instruction to a manager a fire-and-forget command with results arriving on the event stream, or a long-lived request?
- One event stream or several? A single firehose the client filters, or per-worker streams. Fan-out cost versus client complexity.
- Replay: when a client connects mid-run, does it get history then live, or live only with history fetched separately? Note that this is the same question as attach-mid-run, and must not get two different answers.
- Backpressure: what happens when a worker emits faster than a client reads, and what happens when no client is connected at all. The record is the durable answer, so the stream may be lossy, but that must be stated.
- Does the API expose the cell definition for editing, or is editing a definition a plain file operation outside the API?
- Versioning: the API carries a version field for external UIs. What is the compatibility promise?

Constraint carried from `01`: friction budget. The CLI auto-spawns the runtime, the token is handled automatically, and one binary ships both runtime and CLI.
Nothing decided here may require the operator to start a server by hand.

Reference: [headless-ui-boundary.md](../research/headless-ui-boundary.md).

## Prior art: berd (Block, 2026)

[block/berd](https://github.com/block/berd) is a live example of exactly the boundary `01` chose, shipped by a company with resources.

- **Rust core, separate frontend.** Tauri 2 shell, React 19 UI. The UI is a client, not the product.
- **The agent backend is a sidecar process, not a library.** Tauri bundles `goose` as an external binary and launches it with `goose serve`. Version-pinned via `goose-backend.lock.json`, overridable with a `GOOSE_BIN` environment variable.
- **The wire protocol between UI and backend is ACP, over a WebSocket.**

Two things to weigh here.

First, it is independent confirmation that a Rust agent runtime with a swappable UI over a documented protocol is a real shape, not a theory.
The `GOOSE_BIN` override is precisely the "swap the backend, keep the UI" affordance in reverse, and farseer wants the mirror of it.

Second, and more interesting: berd uses **ACP as its own UI transport**, not just as its foreign-agent adapter protocol.
`01` scoped ACP to runners and left the local API surface undefined.
The question this ticket must answer explicitly: is farseer's local API a bespoke HTTP plus SSE surface, or is it ACP over the local transport, with the UI treated as just another ACP client?
Reusing ACP would collapse two protocol surfaces into one and give every ACP-speaking editor a free window onto farseer.
Against it: ACP models a **single agent conversation**, and farseer's surface is a **fleet** - rosters, cell lifecycle, run control, attach and takeover. Most of what farseer must expose has no ACP verb.
Do not assume either answer.

Note also that berd is not the fleet shape the operator wants. It is a single-agent desktop app with a canvas. It is prior art for the **boundary**, not for the **model**.

## Carried from 05

`05 Run state model and control semantics` fixed three axes this surface must expose:

- **Lifecycle** - `queued` / `running` / `finished(ok, failed, cancelled)`.
- **Control** - `autonomous` / `observed` / `taken over`.
- **Liveness** - `live` / `stalled` / `likely-hung`.

**Liveness is derived, never stored.** It is computed from `now - last_activity_at` at read time.
So the API must not expose a write path for it, and any client that caches it must recompute rather than trust a snapshot.
This is the one place where a naive CRUD shape would be actively wrong.

Also inherited: the manager has four verbs, not one - **steer** (same run), **re-scope** (new run, same task), **cancel**, **re-run**.
Whether all four are operator-callable through this API, or only some, is this ticket to decide.
Cancel is a separate verb from takeover, per `07`.

## Resolution

Resolved 2026-08-23 by grilling.

### 1. The API is bespoke HTTP plus SSE. ACP is an adapter on top of it.

Both, and the ordering is the decision.

**The substrate is a bespoke HTTP plus SSE API on `127.0.0.1`.**
**An ACP server adapter sits on top of it as a first-class feature.**

If ACP were the substrate, every fleet operation it has no verb for - cell rosters, cell lifecycle, run control across three axes, attach, takeover, record queries - would deform into custom extensions, and farseer would own a fork of someone else's protocol with Zed's release cadence coupled to its UI surface.
Roughly a fifth of farseer's surface maps to an ACP verb.
A protocol that covers a fifth of the surface is not the transport.

**The berd precedent does not transfer, and that is the crux.**
berd is a single-agent desktop app, so ACP's single-conversation model is a complete fit for it.
Farseer is a fleet. Same protocol, different shape of problem.

Two directions must not be conflated:

- Farseer is an ACP **client** to runners. Decided in `01`, unaffected by this ticket.
- Farseer is an ACP **server** to editors. Decided here, and it is a feature, not a transport.

**Consequence for `06 cell-to-cell transport`:** speaking ACP as a server makes farseer ACP-transparent, so it can present itself as a **runner** to another orchestrator.
That is a second inbound path `01` did not anticipate when it routed foreign orchestrators to A2A as peer cells.
`06` should decide explicitly whether that path is allowed rather than discovering it later.

### 2. The ACP server exposes one thing

**One ACP session maps to one cell's manager conversation. Nothing else.**
The editor picks the cell at session start, defaulting to cell #0.
Gated actions map onto ACP's existing permission-request flow, which is a clean fit rather than a stretch.

Explicitly **not** exposed over ACP: roster management, cell lifecycle, the fleet view, and **attach and takeover**.

Attach and takeover are excluded on purpose and not merely for lack of a verb.
Takeover is an operator-safety surface, and it should not be reachable from a protocol whose client list farseer does not control.
**An ACP editor talks to a manager. It does not drive the fleet.**

### 3. One stream endpoint, scoped server-side

`GET /events?cell=...&run=...`, with the scope chosen by the client at connect time and applied by the server.

Not a firehose the client filters: that ships bytes nobody wants and forces every client to reimplement filtering.
Not fixed per-worker streams: that is a connection per worker for a fleet view.

`01` already ruled concurrent clients out of v1, so fan-out cost is near zero and the only real cost is client complexity.
Push it to the server.

### 4. Replay and attach-mid-run are the same mechanism

**Every stream connection takes a cursor.**

- `?since=<event_id>` replays from the record, then transitions to live with no gap and no duplicate.
- Omit it and the connection is live only.

So "attach to a running worker" and "replay a dead session" are the same call with a different cursor.
That is exactly what `07` asked for when it said live tail and dead-session replay share one surface, and it means the two can never drift into two different answers.

SSE carries `Last-Event-ID` in the protocol itself, so reconnect-with-cursor is free rather than something farseer invents.

### 5. The stream is explicitly lossy, and a slow client never slows a worker

**This is the rule that matters most in this ticket.**
A UI that can stall a worker is the kind of coupling that only surfaces under load, in production, on someone else's machine.

- Bounded per-connection buffer.
- A client that falls behind receives a **`gap` event carrying the last contiguous event id**, and refetches the middle from the record.
- With no client connected, **nothing is buffered at all**. The record is already the durable answer.

Losing stream events is therefore a real and accepted outcome, stated in-band rather than hidden.
The record is the source of truth; the stream is a tail on it.

### 6. Cell definitions are not editable through the API

Definitions are files in git, per `01`, and editing them is a plain file operation.

An edit API would make farseer responsible for merge conflicts, concurrent edits, and skew against the operator's own editor, in exchange for nothing the operator cannot already do in that editor.

The API does expose **read**, **validate** and **reload**.
Without those there is no way to tell a broken definition from a working one short of a restart, and that friction lands directly on the budget `01` set.

### 7. Instructions are commands, not long-lived requests

An instruction to a manager is **fire and forget**: it returns a task id immediately, and results arrive on the event stream.

An instruction can run for hours.
A long-lived request ties completion to a socket staying open, and `05` already established that connections drop and that this must never affect the work.
Same principle, same answer: **never make a worker's fate depend on a client's connection.**

### 8. All four manager verbs are operator-callable, and two of them leave a mark

The operator may call **steer**, **re-scope**, **cancel** and **re-run** directly.

Operator-initiated **re-scope** and **re-run** append an event so the manager knows its plan was overridden.

Blocking the operator would be strange in a tool built for one operator who owns the machine.
But re-scope and re-run are normally manager decisions, and a manager that silently discovers its plan changed underneath it will re-plan badly.
This is the `operator_touched` principle from `05` again: **do not restrict the human, do record that it was the human.**

### 9. Versioning

`/v1/` in the path. **Additive only within a major.**

- New fields may appear.
- Existing fields never change meaning and never vanish.
- Clients must ignore unknown fields.
- A breaking change means `/v2/` served alongside `/v1/` for one release.

Note who this promise is for.
`01` ships runtime and CLI in one binary, so the CLI can never skew.
The compatibility promise exists purely for third-party UIs, which is the entire reason the boundary was drawn.

### 10. Token handling, and the thing the token does not protect against

- Generated at runtime start.
- Written under the user's local app data with an **ACL restricting it to the current user**.
- The CLI reads it automatically, so the operator never sees it.
- A browser UI receives it in a URL **fragment**, never a query string. A fragment is not sent to the server and not written to server logs; a query string lands in both history and logs.

Two hard requirements alongside it:

- **Bind `127.0.0.1` only.**
- **Validate the `Origin` header on every request.**

The second is not optional hardening.
Per [headless-ui-boundary.md](../research/headless-ui-boundary.md), Docker Desktop's CVE-2025-9074 was exactly this shape: a loopback-bound API reachable by any web page in the operator's browser through DNS rebinding.
**A token alone does not save you**, because the browser attaches it for the attacker.

### Tickets this informs

- `06 cell-to-cell transport` - farseer as an ACP server means it can present itself as a **runner** to another orchestrator, a second inbound path alongside A2A peer cells. Decide whether that is allowed.
- `02 record scope` - the record is the source of truth behind a lossy stream, and must support cursor-based reads from an arbitrary `event_id`. That is a query requirement, not just a storage one.
- `17 cell lifecycle` - reload and validate are API operations, so definition version pinning has a live surface to act on.
- `20 worker control channel` - unaffected. That ticket decides farseer's channel **to a worker**; this one decided farseer's channel **to a client**. Two different uses of ACP, as flagged there.

## Amended by 23

**No third verb. `approve` and `reject` are sufficient.**

The prototype in `15` offered the operator approve, **edit** and reject. `23` found **`edit` is takeover** - `07`'s definition word for word - producing the same two events, `operator_intervened` and `operator_touched`.

So "edit" decomposes into what already exists: take over, modify, release, approve.

The consequence for this surface: **the gated-action prompt and the attach surface are the same surface.**
One place where a human touches a run, one set of events.

The constraint that follows is on **presentation, not storage**: if the interface implies a distinction between editing and taking over, the record cannot support it, and "was this edited or taken over" will have no answer.

## Amended by 24

**Two new operations under the additive-only promise: `GET` and `PUT /v1/ui-state/{key}`.**

Farseer stores an opaque blob and **never parses it**, so there is no validation, no schema and no reference-following.
A `413` above 1 MiB per key, and the key is an opaque string farseer never splits on a separator.

No concurrency control, and that is `01`'s out-of-scope ruling on concurrent clients rather than a shortcut.
A conditional write stays additive if that is ever revisited.

This is deliberately the **only** part of the API that returns something farseer does not understand.
Everything about runs, cells and cost stays reconstructible from a query plus a cursor.
