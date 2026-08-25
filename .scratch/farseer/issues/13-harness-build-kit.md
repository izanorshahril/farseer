# What does "farseer builds a harness" actually produce?

Type: research
Status: closed
Blocked by: none

## Question

`ARCHITECTURE.md` claims the output is a cell definition plus scaffolding, not generated code.

- Survey how existing systems package a reusable agent team: prime-agent's Continual Harness (durable prompts, memories, skill descriptions, reusable subagent specs), deepseek-harness's everything-is-a-plugin model, OpenClaw's workspace plus agentDir plus sessions decomposition.
- What is the minimum a cell definition must contain to be runnable and reviewable?
- Is a cell definition a file the operator can read and edit in git, or a database row?
- How is a cell versioned and rolled back when its roster or policy changes?
- Does building a cell require an interview with the operator, or can the builder cell infer a roster from a goal?
- How is a new cell dry-run before it is trusted with real work?

## Carried from 08

`08` passed the falsification test **without adding a single field**, so this kit inherits a confirmed field list and, more usefully, a confirmed list of **non**-fields.

A build kit that adds any of these has broken the test `08` just passed:

- **No review mode.** Artifact type is discovered from the file.
- **No scheduling.** Triggers are API clients, not part of a definition.
- **No credential store.** Credentials live in MCP servers; the definition only grants tool access.
- **No git flag.** The workspace strategy is a policy value, and `plain directory` is one of them.

Also inherited: the social media cell is **thinner** than the coding cell - two worker kinds rather than many.
That is a fact about the domain, not a smell, and the kit should not push an author toward padding a roster to look substantial.

## Carried from 20

The kit's own output must pass the four contract tests from `05`: emit activity at least every N seconds, emit the three progress kinds, accept a follow-up instruction at a turn boundary, and surface a distinguishable cancellation.

**The cheapest way to guarantee that is for the kit to emit ACP**, since `20` found ACP maps onto farseer's activity and progress split almost exactly - `agent_message_chunk` and `thought` are activity, `tool_call` and `tool_call_update` and `plan` are progress.

A harness built by this kit that emits its own bespoke JSON has to re-earn all four tests, and `20` found two real tools that failed them.

## Carried from 21

If this kit can emit an A2A Agent Card, it must **generate the card from the cell definition** rather than let one be maintained by hand.

A compliant A2A 1.0 card requires `name`, `description`, `supportedInterfaces`, `version`, `capabilities`, `defaultInputModes`, `defaultOutputModes` and `skills`, plus `securitySchemes`.
`skills` is a **public enumeration of what the agent can do**, and a cell's roster is data in git that changes per definition.

A hand-maintained card therefore drifts from the roster, and **nothing in A2A notices**.
Generating it makes the drift structurally impossible rather than merely discouraged.

## Carried from 14

`14` locked the glossary, and it is authoritative.

The kit's output must be describable in those words.
**A kit that introduces a new noun has reopened `14`**, so if the kit needs a word that does not exist yet, that is a signal the primitive grew rather than a naming gap.

## Carried from 17

A cell definition must carry a **stable `cell_id` that is not derived from its content**.

Content changes on every reload, and `06` requires the id to survive a reload or the record loses its join key and history detaches from the cell that produced it.
So the kit must not compute the id from a hash of the definition, which is the obvious and wrong choice.

## Carried from 12

`12` settled what policy consists of, and only two parts of it belong in a cell definition:

- **tool grants** - the allowlist, which `12` found is the only real isolation v1 has.
- **irreversibility level** on each tool declaration - `reversible`, `undoable`, or `irreversible`.

Everything else `12` decided is runtime behaviour rather than definition content: autonomy ceilings compose at call time, deny lists union down the chain, and gating is a runtime decision.

Note also that **the delivery gate is a tool, not a field**.
`05` put `definition_of_done` on the worker contract; whether satisfying it means `no-mistakes`, `cargo test` or a human watching a video is a tool grant.
A kit that adds a delivery-gate field has broken `08`'s test.

## Resolution

Resolved 2026-08-23 by direct research plus assembly from six closed tickets.

### Verdict

**`ARCHITECTURE.md` was right: the output is a cell definition plus scaffolding, not generated code.**

The survey found three systems converging on the same split from different directions, and one that deliberately went the other way. The disagreement is the interesting part.

### 1. The survey

#### prime-agent's Continual Harness

Formalised as **H = (rho, G, K, M)** - prompt, sub-agents, skills, memory - held as **durable state the agent can create, read, update and delete while it works**.

Three properties worth taking:

- **The immutable base system prompt is never rewritten.** The Continual Harness sits alongside it and only ever supplements.
- **Refinements are recorded as snapshots**, so a bad lesson can be rolled back.
- **State is local to the session by default** rather than shared globally, so, in their framing, one confused afternoon does not poison every future run.

That third point is independent confirmation of `02`'s memory tiers, and it sharpens one thing `02` left implicit: **the default write tier should be run-local, not global.** A lesson earns promotion to cell-local or global; it does not start there. `11`'s fourth question - which lessons actually reduced failure rate - is exactly the evidence a promotion should require.

The first point maps cleanly onto farseer's existing split: the **cell definition** is the immutable base (human-edited, in git), and **memory** is the mutable supplement (agent-written claims, per `02`). Same architecture, different words, arrived at separately.

#### OpenClaw

Decomposes an agent into **three** things: a **workspace** (files and persona rules), an **agentDir** (auth profiles, model registry, per-agent config), and a **session store** (chat history and routing state).

The mapping onto farseer is close but not identical:

| OpenClaw | farseer |
| --- | --- |
| workspace | **workspace** (`04`) plus the persona half of the **cell definition** |
| agentDir | the rest of the **cell definition**, plus runner config |
| session store | **session** - and `14` narrowed that word to exactly this, harness-owned |

One concrete warning worth carrying: **"never reuse agentDir across agents - it causes auth and session state collisions."**
Farseer gets that for free by making the cell definition per-cell, but it is a real failure mode observed in a shipping system rather than a hypothetical.

#### deepseek-harness, the one that disagrees

**Everything is a plugin**, including the model adapter, the tool registry, the session log and the agent loop itself. **"No privileged core to patch."** Everything replaceable from configuration. Roughly 95,000 GitHub stars in two days.

`01` explicitly **ruled out a farseer plugin ABI**. So this is a genuine counter-datapoint and should be recorded as one rather than waved past.

The honest reading: **dsh and farseer agree on the goal and differ on the mechanism.**

Both want every capability replaceable from configuration.
dsh achieves it with an in-process plugin ABI. Farseer achieves it with **out-of-process protocols** - MCP servers for tools, ACP adapters for runners, API clients for triggers and UIs.

`01`'s reasoning survives contact with the counter-example: a dynamic-loading ABI on Windows is a support burden that cannot be walked back, and an ABI is a stability promise farseer would have to keep forever. The out-of-process route gives the same swappability with a versioned wire protocol instead of a binary interface.

Worth noting where dsh's append-only session log sits relative to farseer's: dsh records **everything the model sees** - system prompts, reasoning, tool calls, context injections. That is a **transcript**. `02` deliberately separated the transcript from the record, and `20`'s activity/progress split is why. Different artefacts serving different questions.

### 2. The minimum cell definition

Assembled from what the closed tickets require. Nothing here is new.

| Field | Source | Note |
| --- | --- | --- |
| `cell_id` | `06`, `17` | **Stable across reload. Never derived from content.** |
| `name`, `description` | `21` | Feeds the A2A Agent Card if the endpoint is ever enabled. |
| `version` | `17` | Plain git. See below. |
| **manager** | `01` | Mandatory. Its runner and its prompt. |
| **roster** | `01` | Worker kinds available, each with a runner. Zero at init is legal. |
| **tools** | `12` | The allowlist, each declaring `reversible` / `undoable` / `irreversible`. |
| **workspace strategy** | `04`, `08` | `worktree`, `plain directory`, later `snapshot`. |
| **policy values** | `12` | Default autonomy, deny list, worker cap. |
| **record scope** | `01`, `02` | Which memory tiers this cell may read across. |

**That fits on one page**, which is the falsification test `01` set and `08` passed.

### 3. A file, not a database row

`01` made a cell definition **data in git**, and `16` gave the API read, validate and reload with **no edit path**.

So it is a file the operator reads and edits, and the store from `09` holds the record rather than the definition.

The argument that settles it: the operator already has a text editor and a diff tool, and `02` made the record evidence rather than configuration. Putting definitions in the database would mean farseer owning merge conflicts, concurrent edits and version history that git already does better.

### 4. Versioning and rollback are plain git

Answered by `17` and repeated here because this ticket asked directly.

Rollback is `git checkout` followed by `reload`.
`17` pinned the definition version **per run**, so a rollback never reaches into work already executing.

Farseer owns nothing here. Anything it built would be a second version-control system shadowing the first.

### 5. Dry run is a ceiling clamp, not a mode

The ticket asked how a new cell is dry-run before it is trusted with real work.

**A dry run is a run with `autonomy_ceiling` clamped to `reversible`.**

No new concept, no new field, no separate code path.

`12` defined three irreversibility levels and made policy gate on them.
`06` established that ceilings compose by **minimum** and only ever narrow.

So clamping the ceiling at the top of a dry run:

- allows every file write inside the workspace, which `04` proved is fully reversible
- blocks every `undoable` and `irreversible` tool, so nothing escapes the workspace
- **propagates automatically into any cell the cell calls**, because ceilings narrow down the chain and can never be raised

That last property is the one that would have been hard to get any other way. A dry-run "mode" would have needed explicit plumbing through every cell call to stay honest; a ceiling gets it for free from a rule that already exists.

The dry run is therefore reviewable in exactly the normal way: it produces artifacts in a workspace, and `08` made review artifact-shaped.

### 6. Building a cell requires the operator, and that is already settled

The ticket asked whether building a cell requires an interview or whether a builder cell can infer a roster from a goal.

**Out of scope, per `01`.** Autonomous cell generation by cell #0 is deferred as a later milestone, and **v1 hand-writes the second cell definition.**

What remains in scope, and what this ticket answered, is what a definition must contain - which is now the table in section 2.

Recording it here so a later reader does not mistake the omission for an oversight.

### 7. What the kit must not produce

The inherited non-fields, collected. **A kit emitting any of these has broken `08`'s falsification test:**

- **No review mode.** Artifact type is discovered from the file.
- **No scheduling.** Triggers are API clients.
- **No credential store.** Credentials live in MCP servers; the definition grants tool access only.
- **No git flag.** Workspace strategy is a policy value.
- **No delivery-gate field.** `05` put `definition_of_done` on the worker contract; the gate is a tool grant.
- **No `cell_kind` or per-domain strictness.** `12` attached policy to the tool, not the cell.

And two positive constraints:

- **The kit's output must pass the four contract tests from `05`.** Per `20`, the cheapest way to guarantee that is to **emit ACP**, since ACP maps onto the activity and progress split almost exactly. A harness emitting bespoke JSON has to re-earn all four tests, and `20` found two real tools that failed them.
- **If an A2A Agent Card is emitted, generate it from the definition.** Per `21`, a hand-maintained card drifts from the roster and **nothing in A2A notices**.

Finally, per `14`: **a kit that introduces a new noun has reopened `14`.** If the kit needs a word the glossary does not have, that is a signal the primitive grew, not a naming gap.

### 8. One thing the survey suggests farseer should copy

**prime-agent records refinements as snapshots so a bad lesson can be rolled back.**

Farseer's memory has no such mechanism. `02` gave memory three tiers and an MCP write path; `11` gave a way to *measure* whether a lesson helped. Neither gave a way to **undo** one.

This is not a definition-format question, so it does not belong in this ticket's answer. It goes to the fog: **memory lifecycle - promotion between tiers, and retracting a lesson that turned out to be wrong.**

### Sources

- [prime-agent](https://github.com/PrimeIntellect-ai/prime-agent)
- [Prime Agent: self-improving RLM harness](https://www.developersdigest.tech/blog/prime-agent-rlm-harness)
- [OpenClaw agent workspace](https://docs.openclaw.ai/concepts/agent-workspace)
- [OpenClaw multi-agent routing](https://docs.openclaw.ai/concepts/multi-agent)
- [deepseek-harness](https://github.com/deepseek-ai/deepseek-harness)
- [DeepSeek open sources an agent harness where everything is a plugin](https://thenewstack.io/deepseek-harness-open-source-plugins/)

### Tickets this informs

- `10 runner inventory` - the definition names a runner for the manager and for each roster entry, so the inventory is what an author picks from. It is a menu, not a survey.

## Carried from 22

A cell definition's **roster must hold three entry kinds**: workers, tools and **callable cells**.

A callable-cell entry carries a maximum `autonomy_ceiling` the caller may grant it, and per `21` a **foreign peer cell entry is pinned at `irreversible`** and cannot be lowered.

Note this does **not** add a top-level field - `roster` already existed - which is why `08`'s falsification test survived.
A cell whose roster holds no cell entries is coherent.

## Corrected by 23

`23 prototype loose ends` added one top-level policy value that the minimum field table above predates: the owning cell's task-root `budget`.
It also added an optional `max_budget` cap to each worker roster entry, parallel to the callable-cell entry's maximum autonomy ceiling.
Both are quantities already present in the worker contract rather than new domain concepts, and both are required for `23`'s three narrowing layers: owning definition, roster entry, and caller remaining.
The section 2 table is historical rather than current on this point; a build kit must emit the task-root budget and may emit a per-worker cap.
