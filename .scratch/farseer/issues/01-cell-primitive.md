# Is the cell the right primitive?

Type: grilling
Status: closed
Assignee: izanorshahril
Blocked by: none

## Question

`ARCHITECTURE.md` proposes that a harness is a **cell**: one manager, its workers, its own record scope, and an address.
Farseer becomes a cell runtime, and the builder-and-management harness is cell #0.

Decide whether this primitive survives, because four other tickets are shaped by the answer.

Points to grill:

- Does the operator actually want a second cell in the first year, or is "farseer builds a social media harness" aspirational enough that v1 should hard-code one cell and generalise later?
- Recursion rules as proposed: depth 2 inside a cell, manager-to-manager across cells, operator attaches at any depth bypassing every manager. Do all three hold?
- Is a cell a *runtime* object (processes, sessions, live state) or a *definition* object (roster, policy, contracts) the runtime instantiates? Very different lifecycles.
- What is the smallest thing that is still a cell? A single worker with no manager?
- Does the cell earn its complexity cost, or is "project plus roster" enough?

Failure mode to name: one abstraction too many, invented for a second product that never ships.

## Resolution

Decided 2026-08-19 with the operator, over three grilling rounds.

**The cell survives, narrowed.**
It earns its place as the unit of **addressing plus policy plus record scope**, and never as a unit of code.
Farseer is an orchestration platform, not a single harness, and the cell is how a harness gets an address.

### 1. Scope of the primitive in v1

Farseer is designed for N cells from v1, and the second cell definition is **hand-written by the operator**, not generated.
Autonomous cell generation by cell #0 is a later milestone, because it requires an interview flow, a dry-run mode and definition versioning, none of which the primitive itself needs.
Designing for N cells costs only an address and a call boundary.

Farseer must also host **foreign harnesses**, not only farseer-native ones.
Anything shaped like a coding agent is a candidate for adoption, which is the operator's stated compatibility goal.

### 2. Cell definition and running cell are two nouns

A **cell definition** is data in a file, held in git: identity, roster, policy, record scope, workspace strategy.
It is diffable, reviewable and rollback-able, and creating one requires no build step.

A **running cell** is the runtime instantiation: one live manager session plus in-flight worker contracts.

The lifecycles genuinely diverge.
A roster is edited while workers are running, and a definition is archived while a run is still draining.
Conflating them is where durable-state corruption lives, which is the failure class `BRIEF.md` catalogues on Windows.
Cost of the split is one extra noun.

### 3. The manager is the invariant

A cell is **one manager plus zero or more workers**.
The manager is mandatory, because the manager **is** the address: an A2A call is delivered to an agent card, and without a manager there is nothing to receive the call, write the contract, or hold record scope.

A lone worker with no manager is not a small cell.
It is a worker inside cell #0.

A cell starts with **zero running workers** and spawns them on demand, up to a configured cap.
The degenerate valid case is a manager with an empty roster doing the work in its own session, which keeps "the runtime must run with exactly one cell" coherent.

### 4. Recursion rules

- **Rule 1, amended.** Inside a cell, delegation depth is 2, and **a worker cannot spawn, full stop**. The `explicit grant` escape hatch in `ARCHITECTURE.md` section 2 is removed, because a grant reintroduces unbounded depth through a config flag and will eventually be set for convenience. A worker needing delegation **returns to its manager with a request**, and the manager decides. Same expressive power, provably loop-free, one fewer knob, and the depth invariant becomes checkable rather than policy-dependent.
- **Rule 2, holds unchanged.** Across cells the interaction is a call, manager to manager only, and the caller never sees the callee's workers. This is both the loop defence and the encapsulation that allows a local cell to be promoted to a remote service as configuration.
- **Rule 4, holds unchanged.** The operator may attach to any worker in any cell, bypassing every manager. Observation is not delegation.

Noted but not adopted: the operator wants the option to revisit rule 1 once Windows spawn behaviour is understood, including spawning workers inside a farseer-owned container.
That is `03 spike: job objects` and `04 spike: workspace teardown` territory, not this ticket.

### 5. Operator addressing

The operator may address **any cell manager directly**, not only the manager of cell #0.
"One interface" means one UI, not one entry agent.
Routing every request through cell #0 makes it a bottleneck, a lossy relay for work whose owner is already known, and burns a manager session on forwarding.
Cell #0 is the **default** recipient, used when the owner of the work is not known, not the only one.
This is the instruction-side mirror of rule 4.

### 6. Foreign harness: runner or cell

The discriminator is **does the thing make its own delegation decisions**.

- **No: it is a worker runner.** Driven over ACP (Zed) by a farseer manager, which writes its contract and owns its record. Claude Code, Codex or Gemini running one session against one task is a runner. This is already what `ARCHITECTURE.md` section 5 says.
- **Yes: it is a peer cell.** It has its own manager and roster, is called over A2A, and farseer never sees inside it.

The analogy is employee versus subsidiary.
This is what makes the compatibility claim work: a social media cell's workers are runners driven the same way coding runners are, which is exactly the section 3 load-bearing test.

**v1 builds the runner path only.**
The peer-cell path is specified but ships no external A2A endpoint.
Internal cell-to-cell keeps A2A-shaped envelopes on the in-process bus, so the message shape is proven without the network.

### 7. Cell is data, plugins live at the tool and adapter boundary

A cell definition is data the runtime interprets, never code the runtime loads.
The genuine extension points are **MCP servers** for tools and **ACP adapters** for runners, both already out-of-process plugins that exist today.

**No farseer plugin ABI in v1.**
It is an extension mechanism designed before there is anything to extend, and a dynamic-loading ABI on Windows is a support burden that cannot be walked back.

### 8. Headless from v1

The runtime is a library plus a local daemon exposing **one API**.
The CLI is a client of that API, and any future UI is another client of the same API.
Zero rendering code in the runtime, and no privileged CLI path.

Justified by two requirements, confirmed against prior art in [headless-ui-boundary.md](../research/headless-ui-boundary.md):

- **Durability.** The runtime outlives any UI restart.
- **Browser reachability.** An HTML prototype UI must be able to reach the runtime, which is the operator's cheap-prototyping and UI-swap requirement.

**Transport: localhost TCP HTTP on `127.0.0.1`, bound port, local API token. Streaming: Server-Sent Events.**
Chosen over a Windows named pipe specifically because a browser cannot open a named pipe, which would foreclose the cheapest possible prototype UI.
Every surveyed tool with a multi-client requirement converged here: OpenCode's `/event` SSE design, Jupyter's kernel-to-many-frontends pub/sub, Docker, Syncthing, Home Assistant, Temporal.
That convergence is driven by browser reachability rather than technical merit, and is recorded as such.

Two costs carried forward:

- Docker Desktop **CVE-2025-9074**, patched August 2025, proves "it is only local" is not a security boundary. The API token is therefore not optional.
- Docker Desktop's Windows named-pipe ACL default of Administrators-only causes recurring failures, which is independent evidence against the pipe.

**Friction budget**, an explicit operator constraint:

- The CLI **auto-spawns** the runtime if it is not running. The operator never starts a server by hand.
- The API token is generated on first run into the config directory and read automatically by the CLI. It surfaces only as a URL token when a browser is pointed at the runtime, the way Jupyter does.
- One binary ships both runtime and CLI, so they cannot version-skew. Only an external UI can, and the API carries a version field for it.

**Concurrent clients are not a v1 requirement.**
The operator wants to swap between an old and a new UI, or between UI variations, **one at a time**, not side by side.
SSE gives concurrent fan-out nearly free, so it is not designed against, but nothing in v1 depends on it.

### 9. Worker versus tool

The discriminator is **supervision**, not whether an LLM is involved.

- A **worker** needs a contract, a budget and its own record entry, and can be cancelled, retried, escalated, or attached to mid-flight.
- A **tool** is a call that returns or errors, and needs none of that.

So a six-minute video render that costs money, can be cancelled, and produces an artifact for review is a **worker**, despite not being an agent.
A text-to-image call returning in three seconds is a **tool**.
A scheduler that posts at a time is a **tool**, or arguably not in the cell at all.

The roster field therefore holds **supervised units of work**, not agents, which is the more general and more defensible primitive.

### 10. Falsification test, to be written into the v1 spec

**The v1 cell definition must fit on one page, and the coding cell and the social media cell must differ only in roster, tools and policy values, never in which fields exist.**

The moment a field is added that only coding needs, or only social needs, the primitive has leaked and farseer is maintaining two products behind one word.
This test is runnable the day the second cell definition is hand-written, which is why v1 hand-writes it rather than deferring it.

`08 What does a non-coding cell need that a coding cell does not?` is the ticket that runs this test.

### Terms settled, for the vocabulary lock

`cell definition`, `running cell`, `manager`, `worker`, `tool`, `runner`, `peer cell`, `roster`, `record scope`.
Fixing the final words is `14 Vocabulary and naming lock`.

## Amended by 06 and 14

Two later tickets overtook wording in section 6 of this resolution.

**`06` rejected the A2A-shaped in-process bus.**
Section 6 says internal cell-to-cell "keeps A2A-shaped envelopes on the in-process bus, so the message shape is proven without the network", inheriting `ARCHITECTURE.md`'s proposal.
`06` decided against it on the same reasoning `16` used against ACP-as-substrate: **an external protocol is spoken at a boundary, never shaped into internals.**
The native path is a **typed internal payload, never serialized**, and A2A is a mapping applied at the boundary.

The surrounding decision is untouched - v1 still builds the runner path only, and still ships no external A2A endpoint.

**`14` retired `envelope` as a noun**, so the payload named here is a **cell call**.
