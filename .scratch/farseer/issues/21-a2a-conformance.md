# A2A conformance: task model, Agent Card, discovery

Type: research
Status: closed
Blocked by: none

## Question

Split out of `06 Cell-to-cell transport: in-process envelope, A2A endpoint, or both?` on 2026-08-23.
`06` settled the design; this ticket settles the spec facts that design now depends on.

It is **not** to be resolved until the operator asks for a `/research` subagent to be spawned, per the map's standing preferences.

- **Task model fit.** Does A2A's message and task model handle a callee that runs for an hour? How do partial results flow, and is there a progress notion farseer can map `05`'s activity and progress signals onto?
- **Agent Card.** What fields are required, what are optional, and what does publishing one force farseer to state publicly? Every field is a commitment that is hard to walk back, and `06` made turning the endpoint on the moment that commitment is made.
- **Discovery and expiry.** Dead agents linger until their card expires. What does the spec say about card lifetime, and what does health checking actually look like in practice?
- **Streaming.** How does A2A carry a stream of partial results, and does it survive a caller reconnecting mid-task? Note `16` made replay and attach the same cursored mechanism locally, so a gap here is a real mismatch rather than a cosmetic one.
- **Auth.** What does the spec expect, and does a per-peer bearer token fit its model or fight it?

## What this ticket must satisfy

`06` fixed the design, and this research must map onto it rather than reopen it.

- The internal envelope is **not** A2A-shaped. A2A is a mapping at the boundary. The question is whether that mapping is lossless, not whether to do it.
- The envelope to map: `{ call_id, from_cell, to_cell, goal, autonomy_ceiling, budget, definition_of_done, deadline }`.
- A cell call is **fire-and-forget**, returning a `call_id`, with results on an event stream. If A2A's task model is request-response at its core, say so plainly, because that is a genuine mismatch and not a detail.
- The endpoint is **off by default** and authenticated with a **bearer token per peer**.
- Farseer is a **fleet**, and `06` decided it must advertise that rather than present as a single agent. Whether A2A's Agent Card can express "I am an orchestrator" is a real question with a real consequence.

Deliver a mapping table from the envelope to A2A, a list of Agent Card fields farseer would have to commit to, and an explicit list of anything that does not map.

## Resolution

Resolved 2026-08-23 by direct research.

**A2A v1.0 is stable as of 2026 and governed under the Linux Foundation.**
Worth noting alongside `09`'s governance lesson: this is the opposite of the Kuzu situation, and a reasonable dependency to take at a boundary.

### Verdict

**The mapping is viable, and it is lossy in exactly one direction.**

A2A's **task lifecycle** maps onto farseer's almost perfectly.
A2A's **contract envelope** does not map at all, and four of eight fields must ride in metadata.
A2A's **stream** cannot express `16`'s cursored replay.
A2A **cannot say "I am an orchestrator"**, which `06` needed.

None of these is fatal. All of them are worth knowing before the endpoint is switched on.

### 1. The task lifecycle maps, and better than expected

A2A defines seven task states:

| A2A | farseer, per `05` |
| --- | --- |
| `TASK_STATE_SUBMITTED` | lifecycle `queued` |
| `TASK_STATE_WORKING` | lifecycle `running` |
| `TASK_STATE_COMPLETED` | `finished(ok)` |
| `TASK_STATE_FAILED` | `finished(failed)` |
| `TASK_STATE_CANCELED` | `finished(cancelled)` |
| `TASK_STATE_INPUT_REQUIRED` | **a gated action awaiting approval** |
| `TASK_STATE_AUTH_REQUIRED` | no farseer equivalent |

Two things fall out of this that are better than the ticket assumed.

**`CANCELED` is a distinct terminal state from `FAILED`.**
That satisfies `05`'s "`cancelled` is never `failed`" and `20`'s fourth contract test natively, with no convention needed.
Notably this is *stronger* than what the native headless harnesses offer, where `20` rated cancellation "weak" for both Claude Code and Codex.

**`INPUT_REQUIRED` is an interrupted, not terminal, state**, and the client resumes by sending another message with the same task id.
That is exactly the shape of `01`'s gated actions, so approvals cross a cell boundary without invention.

### 2. Long-running work is fine. A2A is async-first.

The ticket asked whether A2A handles a callee that runs for an hour.

**Yes, and the design assumption matches `06`'s.**
Operations return immediately with a Task object, and processing continues asynchronously.
`SendMessageConfiguration` carries a `returnImmediately` flag: `true` returns as soon as the task is created.

`06` decided a cell call is **fire-and-forget, returning a `call_id`**.
That is `returnImmediately: true`, and the `call_id` is the A2A Task id.

Also aligned with `08`: A2A returns results as **Artifacts** associated with a Task, and states that Messages SHOULD NOT be used to deliver task outputs.
`08` independently made **artifact** farseer's general unit of reviewable change. Convergence, not coincidence.

### 3. The stream gap, which is the real mismatch

The ticket flagged this as needing an honest answer, and there is one.

A2A offers three delivery modes: **streaming** (`SendStreamingMessage`, `SubscribeToTask`), **push notifications** to a client webhook, and **polling** via `GetTask`.

`SubscribeToTask` MUST return a Task object as its first event, representing current state at subscription time - explicitly to avoid the race between `GetTask` and subscribing.
Multiple concurrent streams per task are supported, and events MUST be broadcast to all of them in order.

**But that is a state snapshot followed by live events. It is not a backlog replay from a cursor.**

`16` made replay and attach the *same* call with a different cursor: `?since=<event_id>` replays from the record then transitions to live with no gap and no duplicate.
**A2A has no equivalent.** Reconnecting to an A2A task gets current state plus whatever happens next; the events that occurred while disconnected are gone.

Consequence, stated plainly: **farseer's own record remains the source of truth, and a peer cell's history is not recoverable through A2A.**
This is consistent with `07`, which already ruled peer cells **observable but not attachable**, and with `06`'s decision that the callee's record is separate and not merged. The gap does not break anything already decided; it explains *why* those decisions were right.

Also relevant to farseer specifically: **push notifications require the client to be reachable over HTTP**, which for a single-machine local-first tool means the streaming or polling modes are the realistic ones.

### 4. The Agent Card, and what publishing one commits farseer to

A compliant A2A 1.0 Agent Card requires at minimum:

`name`, `description`, `supportedInterfaces`, `version`, `capabilities`, `defaultInputModes`, `defaultOutputModes`, `skills`

Plus `securitySchemes` for authentication, an optional provider block, and an optional agent signature.

**`skills` is the expensive one.**
It is a public enumeration of what this agent can do, and `06` already established that turning the endpoint on is the moment the card becomes a commitment hard to walk back.
For a cell whose roster is data in git and changes per definition, **the card and the definition can drift**, and nothing in A2A notices.

Recommendation for whoever implements this: **generate the Agent Card from the cell definition rather than maintaining it separately**, so drift is structurally impossible rather than merely discouraged.

### 5. Discovery and expiry: there is no health check

The ticket asked what health checking looks like, given dead agents linger until their card expires.

**The honest answer is that A2A has no health check, and no liveness concept at all.**

Discovery is a well-known URI: the client GETs `/.well-known/agent-card.json`.
Freshness is **plain HTTP caching** - a `Cache-Control` header with `max-age`, and an `ETag` derived from the card's version or a content hash so clients can make conditional requests.

So a "dead agent" is just a URL that stops answering. There is no expiry in the protocol, only cache lifetime on a static document.

For farseer this is fine and requires nothing: the endpoint is off by default per `06`, and a peer that stops answering is a failed call the calling manager already owns, per `06`'s "a cell call is a run in the calling cell".

### 6. Auth fits natively

`06` chose a **bearer token per peer**.

A2A declares authentication through `securitySchemes` in the Agent Card, supporting `APIKeySecurityScheme`, `HTTPAuthSecurityScheme`, `OAuth2SecurityScheme`, `OpenIdConnectSecurityScheme` and `MutualTlsSecurityScheme`.

A per-peer bearer token is `HTTPAuthSecurityScheme` with scheme `bearer`. **It fits the model rather than fighting it.**

One useful bonus: the caller's identity comes from which token authenticated the request, which means `06`'s `from_cell` envelope field does not need a wire home - it is derived from auth rather than asserted by the caller, which is also the safer construction.

### 7. What does not map

The envelope `06` fixed, field by field.

| Envelope field | A2A home |
| --- | --- |
| `call_id` | Task id |
| `from_cell` | **derived from auth**, not carried |
| `to_cell` | the endpoint URL the card advertises |
| `goal` | the initiating Message content |
| `autonomy_ceiling` | **no native field** |
| `budget` | **no native field** |
| `definition_of_done` | **no native field** |
| `deadline` | **no native field** |

**Four of eight fields have no native A2A home** and must ride as structured data in a message part or in metadata.

That is not a blocker - it is a JSON payload either way - but it has a real consequence: **a non-farseer A2A agent will silently ignore all four.**
It will not honour the autonomy ceiling, will not respect the budget, will not know the definition of done and will not observe the deadline.

So the rule for whoever builds this: **a foreign A2A callee is unbounded by construction.**
Any budget or deadline enforcement must happen on farseer's side, in the calling cell's run, because the callee cannot be trusted to have even read those fields.
That is a policy input for `12`, not just an implementation note.

### 8. A2A cannot say "I am an orchestrator"

`06` decided farseer must advertise its true nature so a caller can refuse, rather than presenting as a single agent.

**The A2A specification has no field expressing that an agent delegates to sub-agents or acts as an orchestrator.**
The model covers agent-to-agent communication and task management, with no representation of internal delegation.

The available options are all weak:

- Put it in `description`. Human readable, not machine checkable.
- Declare it as a `skill`. Structured, but skills describe capabilities rather than architecture, so it is a misuse that no caller will be looking for.
- A custom extension. Nobody implements it.

**Recommendation: state it in `description`, and accept that over A2A the declaration is advisory rather than enforceable.**

This is a genuine asymmetry with `06`'s ACP decision and should be recorded as one.
Over **ACP**, farseer declares itself an orchestrator during `initialize` capability negotiation, and a caller can programmatically refuse.
Over **A2A**, farseer can only say so in prose.

The asymmetry is tolerable because the risk differs: an ACP caller expects a single agent and its supervision assumptions break, whereas an A2A caller already expects an autonomous peer that manages its own work. **A2A's model is closer to what farseer actually is, which is why it lacks the field.**

### Sources

- [A2A Protocol specification](https://a2a-protocol.org/latest/specification/)
- [A2A agent discovery](https://a2a-protocol.org/latest/topics/agent-discovery/)
- [A2A specification on GitHub](https://github.com/a2aproject/A2A/blob/main/docs/specification.md)
- [Agent Cards best practices](https://aigrowthagent.co/articles/agent-cards-best-practices/)
- [A2A protocol architecture guide](https://tyk.io/learning-center/a2a-protocol-architecture-and-technical-specification/)

### Tickets this informs

- `12 autonomy and deny list` - **a foreign A2A callee is unbounded by construction.** It ignores `autonomy_ceiling`, `budget`, `definition_of_done` and `deadline`, because A2A has no field for any of them. Enforcement must live in the calling cell's run. That is a policy question, not an implementation detail.
- `13 harness build kit` - if the kit can emit an A2A Agent Card, it should **generate it from the cell definition**, since a hand-maintained card drifts from the roster and nothing in A2A notices.

## Amended by 14

This ticket closed before `14 Vocabulary and naming lock`, so its resolution uses a word `14` retired.

**`envelope` is not a farseer noun.**
Everywhere this ticket says "the envelope" - the eight-field payload mapped to A2A, and `from_cell` as an "envelope field" - the term is **cell call**.

`14` retired the word because `05` used it for the worker contract and `06` for the cell-call payload: overlapping fields, one token, two targets.
Nothing about this ticket's findings changes. The mapping table, the four fields with no native home, and the unbounded-foreign-callee conclusion all stand.
