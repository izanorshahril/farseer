# Cell-to-cell transport: in-process envelope, A2A endpoint, or both?

Type: grilling
Status: closed
Blocked by: none

## Question

`ARCHITECTURE.md` proposes A2A-shaped envelopes on an in-process bus for local cells, and a real A2A endpoint plus Agent Card only for external harnesses.

- Is A2A's message and task model a good fit for a manager-to-manager contract call, or is it built for a different granularity?
- What must an Agent Card carry for a farseer cell, and what does it force us to state publicly?
- Registry and discovery: dead agents linger until their card expires, so what does health checking look like?
- Is the in-process shortcut real, or does shaping everything as A2A envelopes cost more than a plain internal call?
- Streaming and long-running work: does A2A handle a worker that runs for an hour, and how do partial results flow?
- Auth between cells, and whether a cell gets a scoped identity. Buzz's per-agent cryptographic identity with scoped permissions is the reference.

## Carried from 16

`16 What is the local API surface?` decided farseer speaks **ACP as a server**, exposing one cell manager conversation as one ACP session.

That makes farseer **ACP-transparent**: it is an ACP client to runners and an ACP server to editors.
The consequence for this ticket is a path `01` did not anticipate.
`01` routed foreign orchestrators to A2A as **peer cells**, on the reasoning that they make their own delegation decisions.
But an ACP server surface means **another orchestrator can consume farseer as a runner**, which is a second inbound path with different semantics: as a runner, farseer would be treated as a single agent making no delegation decisions, which is precisely what it is not.

Decide explicitly whether that is allowed, rather than discovering it in the field.
Three options worth weighing: forbid it, allow it and accept that the caller sees a flattened view, or allow it and have farseer advertise its true nature during the ACP handshake so the caller can refuse.

## Resolution

Resolved 2026-08-23 by grilling.
The A2A spec specifics were split out to `21 A2A conformance: task model, Agent Card, discovery` rather than guessed at here.

### 1. Both paths ship, and the ordering is the decision

- **Native path: a typed internal envelope, in-process, never serialized.** This is what local cells use, always.
- **Alternative path: a real A2A endpoint with an Agent Card**, reached by mapping the internal envelope to A2A at the boundary.

The envelope is **not** A2A-shaped internally.
`ARCHITECTURE.md` proposed A2A-shaped envelopes on an in-process bus; that is rejected.

The value of an envelope is the **seam**, not the wire format.
A2A-shaping an in-process call pays serialization and spec-conformance cost for zero benefit, and welds farseer's internal model to an external spec that will keep moving.

This is the same reasoning `16` used to reject ACP-as-substrate, applied consistently:
**an external protocol is something you speak at a boundary, never something you shape your internals around.**

The acknowledged cost is that the A2A adapter is a mapping layer rather than a passthrough, so it is real work rather than free.
That is the cheaper of the two mistakes, because the alternative is a rewrite of the internals every time the spec moves.

### 2. The A2A endpoint is off by default

Local cells never traverse it - they take the in-process path - so the endpoint serves foreign callers exclusively.

`01` ruled that farseer requires no external services.
Shipping it **listening** on the network by default is that same instinct pointed the wrong way.

Turning it on is a deliberate act, because that is the moment the **Agent Card becomes a public commitment** that is hard to walk back.

### 3. Inbound: another orchestrator may consume farseer as a runner, but farseer says what it is

This is the path `16` created by making farseer an ACP server.

**Allowed, and advertised during the ACP `initialize` handshake.**

Forbidding it is unenforceable, since anything can speak ACP at the endpoint, and hostile to the compatibility story farseer exists to tell.
Allowing it **silently flattened** is worse than either option: the caller believes it is driving a single agent when it is driving a fleet, so its own timeout, cancellation and progress assumptions are quietly wrong.

So farseer declares at capability negotiation that it is an **orchestrator, not a single agent**, and the caller decides whether to proceed.
That fails at connect time rather than in the middle of an hour-long task.

Farseer therefore has **three inbound and outbound protocol roles**, which must not be conflated:

| Role | Protocol | Peer |
| --- | --- | --- |
| client | ACP | runners (foreign agents) |
| server | ACP | editors, and orchestrators consuming farseer as a runner |
| both | A2A | peer cells (foreign orchestrators), endpoint off by default |

### 4. The envelope is `05`'s contract minus what the callee owns

`{ call_id, from_cell, to_cell, goal, autonomy_ceiling, budget, definition_of_done, deadline }`

The sharp distinction, and the reason a cell call is not a worker call:

**A manager-to-worker contract names the workspace, the runner and the tool grants. A manager-to-cell contract must not.**
The callee cell owns its own roster, its own workspace policy and its own tool grants.
That ownership is what makes it a cell rather than a worker.

**The caller states what it wants and what it will pay. The callee decides how.**

`autonomy_ceiling` rather than `autonomy` for the same reason: a caller may **cap** the callee, never raise it above the callee's own policy.
A ceiling composes safely under nesting; an absolute value does not.

Constraint from shipping the A2A path: every field must survive a JSON boundary cleanly, even though the local path never serializes it.
No Rust-only types in the envelope.

### 5. A cell call is fire-and-forget

Returns a `call_id` immediately. Results arrive on the event stream.

Identical to `16`'s answer for operator instructions, for identical reasons.
A cell call can run for hours, and a synchronous call blocks the calling manager's own loop and welds two cells' lifetimes together, so a callee that hangs takes its caller down with it.

This also keeps `05`'s watchdog working unchanged: the call is a **run in the caller** with its own `last_activity_at`, so a silent callee is flagged `stalled` like anything else.

### 6. Failure ownership dissolved rather than needing a mechanism

This sat in the map's fog as "waits on the transport decision".

**A cell call is a run in the *calling* cell, so `05` already answered it.**

The calling manager owns retry, timeout and escalation exactly as it does for any worker - same four verbs, same three axes, same budget, same watchdog.
The callee's internal failures are its own record and its own business.

**A cell is a worker whose implementation happens to be another manager.**

No new failure-ownership machinery exists. The fog entry clears without becoming a ticket.

### 7. Identity and auth

**Local cells: a stable `cell_id`, and no cryptography.**
Local cells share one trust domain - one operator, one machine, one process.
Crypto between them defends against an attacker who already has code execution as the operator, which is not a threat model worth paying for.

The id lands now regardless, because the record needs it and retrofitting identity through a record format is genuinely painful.

**Foreign callers on the A2A endpoint: a bearer token per peer, bound to a cell id.**
"No auth" is not available once this is a network surface.
A token **per peer** rather than one global key, so revocation is per peer.

Buzz-style per-agent cryptographic identity with scoped permissions stays deferred.
It is the right answer for a multi-party world, and farseer has one operator.

Same discipline as `16`: **bind loopback by default, validate `Origin`, and make opening it to the network an explicit operator act** rather than a config default nobody read.

### Tickets this informs

- `21 A2A conformance` - inherits the envelope it must map, and the fact that the endpoint is optional, off by default, and token-authenticated per peer.
- `12 autonomy and deny list` - inherits `autonomy_ceiling` and the rule that a caller may cap a callee but never raise it. Nesting composition is now that ticket's problem.
- `02 record scope` - a cell call is a run in the caller, so it is one record entry there. The callee's own record is separate and is not merged. Whether the caller's entry links to the callee's is that ticket to decide.
- `17 cell lifecycle` - `cell_id` must be stable across a definition reload, or the record loses its join key.

## Renamed by 14

`14 Vocabulary and naming lock` retired **envelope** as a noun, because `05` used it for the worker contract and this ticket uses it for the cell-call payload.

The prose below is left as written. Read it with the final words substituted:

- **cell call** - the thing this ticket calls "the internal envelope".
- **worker contract** - what `05` calls "an envelope".

Note that where this resolution says "A2A-shaped envelopes on an in-process bus", it is quoting `ARCHITECTURE.md`'s rejected proposal, and that wording stands as a quotation.

Also renamed: **seat** is now **runner**, applied throughout.
