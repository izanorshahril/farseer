# What does a non-coding cell need that a coding cell does not?

Type: grilling
Status: closed
Blocked by: none

## Question

The load-bearing test for the cell abstraction, using the operator's own example: a social media cell with post-writer, media-generator and video-editor workers.

- What is a non-coding cell's unit of reviewable change? A coding cell has a diff. A post has no diff, and a rendered video certainly does not.
- Does a non-coding cell need git at all? If not, what does workspace isolation mean for it?
- Credentials: posting needs real account access, a category of authority farseer has so far granted only over local files.
- Approval: does every outward-facing artifact stop at the operator, always?
- Long-running and scheduled work: a post scheduled for Tuesday is not a task that completes now.
- Tools: media generation and video editing are external services or heavy local binaries, not CLI harnesses.

If the answers force a second runtime, the cell abstraction has failed and `01` must be revisited.

## Carried from 01

`01 Is the cell the right primitive?` handed this ticket the explicit falsification test for the whole design.

**The v1 cell definition must fit on one page, and the coding cell and the social media cell must differ only in roster, tools and policy values, never in which fields exist.**

If answering the questions above adds a field that only coding needs, or only social needs, the primitive has leaked and `01` must be reopened.

Two decisions from `01` reshape the roster question before it is asked:

- **Worker versus tool is decided by supervision, not by whether an LLM is involved.** A worker needs a contract, a budget and its own record entry, and can be cancelled, retried, escalated or attached to mid-flight. A tool is a call that returns or errors. So a six-minute video render is a **worker** despite not being an agent, a three-second text-to-image call is a **tool**, and a scheduler that posts at a time is a **tool**. The roster holds supervised units of work, not agents. This removes the objection that a non-coding roster is mostly tools wearing worker costumes, but it raises a sharper one: check whether the social media roster still has enough genuinely supervised units to be a cell at all.
- **Foreign harnesses are runners, driven over ACP (Zed), unless they make their own delegation decisions.** So "media generation and video editing are external services or heavy local binaries, not CLI harnesses" is a question about what a runner may be, not only about tools.

## Resolution

Resolved 2026-08-23 by grilling.

### Verdict on the falsification test

**The primitive survives. `01` is not reopened.**

The test from `01` was: the v1 cell definition must fit on one page, and the coding cell and the social media cell must differ only in **roster, tools and policy values**, never in which fields exist.

Working through every question on this ticket added **zero fields**.
Two things did change, and neither is a field:

- A **word** is wrong. "Runner" implies "an agent sits here", and it must not. Handed to `14`.
- A **policy dimension** is missing. Irreversibility. Handed to `12`.

That is the primitive bending, which is what it is for, rather than leaking.

### 1. Reviewable change is over artifacts, and artifact type is discovered

A run produces **artifacts** in a workspace, and review is over artifacts.

A diff is not a separate concept, it is how a **text** artifact's change is presented against a base.
A post is a text artifact with no base, so the diff is the whole thing.
A video is a binary artifact, so review is watching it.

**Artifact type is discovered from the file, never declared in the definition.**
So there is no `review_mode` field, and the test survives.

`05` already put `definition_of_done` on the **contract** rather than the definition, so "what counts as finished" has a home that is not the primitive.

### 2. A non-coding cell does not need git, and this costs nothing

`04` chose `worktree` as the **default** isolation strategy and left `snapshot` unevaluated in the same slot.
A third value, **`plain directory`**, drops into that slot with no new field.

Social media work wants isolation and history but not git: versioning generated video in git is bloat with no upside, because binaries do not diff and do not merge.

**The workspace is a directory. git is one way to isolate one.**

`04`'s teardown findings apply unchanged either way, since they were about `SHARING_VIOLATION` on a directory rather than about git.

### 3. The leak: "runner" carries a connotation it must not

`01` said foreign agents are **runners** over ACP.
`01` also said a six-minute video render is a **worker**, not a tool, because it is supervised.
But `05`'s contract names a `runner`, and an `ffmpeg` render is not an ACP agent.

So what fills the `runner` field for a video-editor worker?

**Redefinition: a runner is anything that satisfies the worker control channel contract.**

`20`'s contract tests already describe this without naming it - emit activity, emit the three progress kinds, accept cancellation, surface a distinguishable cancellation.
An ACP agent satisfies that.
A process adapter around `ffmpeg` satisfies it too: stderr gives progress, bytes give activity.

No field changes, so the falsification test holds.

But **the word carries "an agent sits here", and that connotation will mislead every future reader** into thinking a non-agent worker needs some other slot.
Either the definition is nailed down loudly or the word changes. That is `14`'s call and it is a real one.

### 4. Credentials dissolve. Irreversibility does not.

**Farseer never holds account credentials.**
A "post to X" capability is an MCP server that holds its own credential, and the contract's existing `tool grants` field says whether a worker may call it.
No new field, and no new category of authority for farseer to hold.

What does not dissolve is the **risk asymmetry**.
Every authority farseer has granted so far is over local files, which is reversible.
**A published post is not.**

That is a policy value rather than a field, but it is a **dimension `12` does not currently have**.

So: tools declare whether they are irreversible, autonomy policy gates on that, default gated, overridable per cell.
The gating machinery already exists - `01` has gated actions and `16` mapped them onto ACP's permission flow - so this is a new input to existing machinery, not new machinery.

### 5. Scheduling: not a subsystem, but in the box

The recommendation was "no scheduler in v1". The operator asked how comparable tools handle it, and the evidence partly cut against that.

**Both comparables ship scheduling as core, not as a plugin:**

- **hermes-agent** has a built-in cron scheduler in a dedicated `cron/` directory, for unattended recurring work.
- **OpenClaw** has **Automations**, a built-in scheduler that persists jobs and wakes the agent, **and separately** event-driven **hooks** fired by agent lifecycle events.

OpenClaw's split confirms the distinction worth keeping: **hooks are event-triggered, cron is time-triggered.** They are two trigger kinds, not one feature.

So omitting scheduling entirely would leave farseer missing what both comparables have.
But putting a durable-timer subsystem inside the runtime buys catch-up-after-downtime, missed-fire policy and timezone handling, none of which the cell primitive requires.

**Resolution: triggers are API clients, not subsystems.**

- A **cron trigger** holds its own durable timers, fires, and calls farseer's local API to start a run.
- A **hook trigger** subscribes to the SSE event stream from `16` and calls back in on a matching event.

Both are just clients, the same shape as a UI.
This satisfies the operator's "as plugin" instinct **without a plugin ABI**, which `01` ruled out - the extension points stay MCP servers and API clients, both already out of process.

They ship in the box as first-party trigger clients.
The runtime gains nothing and farseer loses no capability.

Note the pleasing consequence: `16` drew the client boundary for UIs, and it turns out to carry triggers for free.
That is evidence the boundary was drawn in the right place.

### 6. The sharper objection from `01`, answered

`01` asked whether the social roster still has enough genuinely supervised units to be a cell at all, once worker-versus-tool is decided by supervision.

Working it through with the operator's own example:

| Unit | Verdict | Why |
| --- | --- | --- |
| post writer | **worker** | LLM, long, cancellable, needs its own record entry |
| media generator | **tool** | a three-second call that returns or errors |
| video editor | **worker** | six-minute render, supervised, cancellable, emits progress |
| scheduler | **tool** | a call that returns "scheduled" |

Two worker kinds plus tools plus a manager.

**That is a cell.**
`01` made the manager the invariant and allowed zero workers at init, so a cell with two worker kinds is comfortably within the definition.

The social cell is **thinner** than the coding cell.
That is a fact about the domain, not a failure of the abstraction, and it is worth recording so nobody later mistakes thinness for a smell.

### Tickets this informs

- `13 harness build kit` - now unblocked. It inherits the confirmed field list and, more usefully, the confirmed **non**-fields: no review mode, no scheduling, no credential store, no git flag. A build kit that adds any of those has broken the test this ticket just passed.
- `14 vocabulary lock` - **"runner" is the one word known to be wrong.** A runner is anything satisfying the worker control channel contract, not an agent. Decide whether to redefine loudly or rename.
- `12 autonomy and deny list` - inherits **irreversibility** as a policy dimension it does not currently have. Tools declare it, policy gates on it, default gated.
- `10 runner inventory` - the inventory is wider than "agents". A process adapter around a local binary is a runner if it satisfies the contract, so the inventory question includes non-agent runners.
