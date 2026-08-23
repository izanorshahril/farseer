# Memory lifecycle: promotion, retraction, and who is allowed to do either

Type: grilling
Status: closed
Blocked by: none

## Question

Graduated from the map's **Memory lifecycle** fog on 2026-08-23, which noted it waits on nothing in particular.

`02` gave memory three tiers and an MCP write path.
`11` gave a way to **measure** whether a lesson helped.
`13` found that prime-agent records refinements as snapshots so a bad lesson can be rolled back.

**Farseer has no mechanism for any of it.**
A lesson is written into a tier and stays there, correct or not, forever.

### 1. Two closed tickets disagree about the default write tier

This is the sharpest part, because it is not fog - it is a contradiction already on the map.

- **`02` section 4** says **cell-local** is "the default".
- **`13`** says "the default write tier should be **run-local**, not global", citing prime-agent's finding that state local to the session stops one confused afternoon poisoning every future run.

Both are closed. Both are cited elsewhere. One of them is wrong.

Note they are answering slightly different questions - `02` was scoping *readability*, `13` was arguing about *blast radius* - which is probably how the contradiction survived. Decide which is the default and say so in one place.

### 2. What promotes a lesson, and is it automatic?

`11`'s fourth question - **which lessons actually reduced failure rate** - is exactly the evidence a promotion should require, and `13` says promotion should be the normal path rather than an exception.

So the mechanism has an obvious input and no defined trigger:

- **Automatic on evidence.** The runtime promotes when `11`'s query clears a threshold. Cheap, and it means the operator wakes up to a global tier they never approved.
- **Agent-requested.** A worker or manager asks. But `02`'s MCP face already withholds raw event append on the grounds that **an agent that can forge events can rewrite its own history** - and an agent that can promote its own lesson to global can poison every cell from inside one run. The parallel is close enough that the same reasoning may apply.
- **Operator-gated.** Safest, and it makes promotion an exception rather than the normal path, which is what `13` argued against.

Whichever it is, note that promotion is the only path by which one cell's claim reaches another cell, since `02` made cross-cell reads beyond the global tier opt-in via the reader's definition.

### 3. How is a lesson that turned out to be wrong retracted?

`02` made the record **append-only**, and memory is agent-written claims inside it.

So a wrong lesson cannot simply be edited away, and three questions fall out:

- **Is retraction an append** - a tombstone claim that supersedes - or a delete?
- **Does `17`'s purge apply?** Purge is the only irreversible verb farseer owns and it writes a tombstone emitting `void`. A retracted lesson is not a privacy problem, so purge is probably the wrong tool, but it is the only removal verb that exists.
- **`13`'s snapshot finding** suggests rollback rather than deletion, which is a third shape neither `02` nor `17` currently has.

### 4. What this must not break

- `02` - **scrub on write, never at read.** A promoted lesson has already been scrubbed once; promotion must not become a second write path that skips it.
- `02` - the MCP face is **query plus memory-write, never raw event append**. Whatever verbs this adds, the forge threat that ruling exists to stop must stay stopped.
- `12` - **purge is operator-only**, because forge and destroy are two halves of one threat. If retraction is a form of removal, the same reasoning applies to it.
- `11` - if promotion is automatic on `11`'s own metric, the metric becomes a control input rather than an observation, and starts influencing what it measures.

## Resolution

Resolved 2026-08-23 by grilling, with the first question settled by researching Hermes Agent at the operator's direction.

### 1. Cell-local is the default. `02` stands, and `13`'s concern is answered by a mechanism neither ticket proposed.

**Hermes Agent has no memory tiers.** It has **one flat store with a hard character cap** - `MEMORY.md` at 2,200 characters for environment facts and lessons, `USER.md` at 1,375 for operator preferences - and when a write would exceed the cap **the tool returns an error rather than silently dropping anything**, forcing the agent to consolidate or evict in the same turn.

Memory does not auto-compact. **Scarcity is the feature**, and at roughly 215,000 GitHub stars as of July 2026 it is the most widely deployed curated-memory design in existence.

That reframes this ticket's question. The map assumed **tiering** was what bounds a bad lesson's blast radius. Hermes bounds it with a **budget** instead: a wrong lesson cannot accumulate, because it competes for a small fixed space with better ones and must justify itself against them on every subsequent write.

So:

**The default write tier is `cell-local`, and every tier carries a size cap that errors rather than drops.**

`02` was right about the default and did not say why it was safe. This is why.

`13`'s argument was real - one confused afternoon should not poison every future run - but its proposed fix, a **run-local** default, carries two costs that only became visible when written down:

- **Run-local is `02`'s word for scratch that dies with the run.** A run-local default means memory does **nothing at all** until promotion is built, which converts promotion from a feature into a prerequisite.
- **It creates a chicken-and-egg with `11`.** `11`'s fourth question - which lessons reduced failure rate - needs a lesson to have been *used* to generate evidence. A lesson that dies with its run has n=1 forever, so statistics can never promote anything.

The cap dissolves both. Nothing has to be promoted for memory to work, and **`11`'s evidence governs eviction rather than promotion**, which is the only direction the statistics actually support.

**Do not copy Hermes' number.** 2,200 characters is a single-agent assistant's budget. Farseer has a cap **per tier per cell**, and the value is an implementation choice. The principle is what transfers: a cap that **errors**, never one that silently drops.

### 2. Promotion is tiered by blast radius

**The manager decides for `cell-local`. The operator gates `global`.**

Global is the only tier that crosses cells, so it is the only one that needs a human. Friction is proportional to reach, and `02`'s isolation reasoning survives intact.

Rejected, and why:

- **Automatic on `11`'s evidence** - `11`'s metric would stop being an observation and become a control input influencing what it measures.
- **Agent-requested at any tier** - closest to `02`'s forge threat. An agent that can promote its own lesson to global poisons every cell from inside a single run, which is the same shape as the reasoning that withheld raw event append.

Hermes converges on the gate but not the tiering: it offers a single `write_approval` flag, default **off**, with `/memory pending` to review staged writes and approve or reject them individually. Farseer's version is that flag made **automatic for exactly one tier** rather than global and manual.

### 3. Retraction is an append, never a removal

**A retraction is a superseding tombstone claim. The read path resolves latest-wins.**

This fits `02`'s append-only record exactly and keeps the history of what was believed and when, which is the thing `11`'s fourth question needs in order to ask whether a lesson helped.

**Farseer explicitly diverges from Hermes here, and the divergence is forced.**
Hermes corrects with a destructive `replace` and `remove`, and `hermes journey delete` removes permanently. Hermes is not an append-only record; `02` made farseer one.

The consequence is that **consolidation under the cap is also an append**: merging several entries into a denser one writes a new claim that supersedes them, rather than editing in place. Hermes' consolidation behaviour transfers, its storage semantics do not.

`17`'s **purge** stays what it is - operator-only, irreversible, for content that must not exist. A merely wrong lesson is not a privacy problem and does not warrant it.

### Two independent convergences worth recording

- **Hermes splits memory by kind, not by scope** - environment facts and lessons in one store, operator preferences in another. That is `02` section 4's ruling arrived at separately, and it is the third time an outside system has independently confirmed a farseer decision, after prime-agent on tiers and A2A on artifacts.
- **Hermes scans on write**, blocking prompt-injection and credential-exfiltration patterns and invisible Unicode. That is `02`'s **scrub on write, never at read**, with a threat model attached. The invisible-Unicode case is one farseer had not considered and should adopt.

### Tickets this informs

- `02 record scope` - **cell-local confirmed as the default write tier**, and every tier gains a **size cap that errors rather than drops**. Scrub-on-write should additionally reject invisible Unicode, per Hermes' threat model.
- `11 analytics questions` - the fourth question becomes an **eviction** criterion, not a promotion one. Promotion is a judgement; demotion can be evidence-driven.
- `16 local API surface` - the operator needs a way to approve or reject a **global** promotion. Per `23`, this is the same gated-action surface as everything else and needs no new verb.
- `13 harness build kit` - a cell definition's **record scope** field now also implies a per-tier cap.
