# Routing policy: does farseer choose a runner, and what happens when one runs out?

Type: grilling
Status: closed
Blocked by: none

## Question

Graduated from the map's **Routing policy** fog on 2026-08-23, once `10` closed and made exhaustion observable.

### 1. Is there routing at all?

This has to come first, because the answer may be no.

`13`'s cell definition gives the **roster** worker kinds, **each with a runner**.
If a worker kind names exactly one runner, then the author routes at definition time and the runtime never chooses anything.

So decide whether a roster entry names **a runner** or **a preference order**, and be clear that the second one is what creates a routing problem.

The cost of the first: on a **Pro** account, a pinned runner means a worker kind is simply unavailable for the rest of the window when its runner is exhausted.
The cost of the second: the runner a run executed on is no longer predictable from the definition, and `11`'s cost-by-runner query becomes the only way to know what actually happened.

### 2. The asymmetry is the interesting part

`10` measured that the three usable runners do not report the same things:

- **Claude Code** emits `rate_limit_event` on **every successful run** - window, `resetsAt` epoch, overage state - so exhaustion is visible **in advance**.
- **Codex** and **cursor-agent** emit nothing. Exhaustion is only knowable **after a run fails**.

Two coherent designs:

- **Level down.** Treat every runner as unobservable and react only to failure. Uniform, simple, and throws away the one signal farseer gets for free.
- **Exploit it.** Pre-emptively avoid the Claude Code window as it approaches reset, and let the others fail and retry. Better behaviour, and **two code paths that will drift**.

Note what `10` did *not* find: `used_percentage` exists but only on the status line, which **provably does not fire headless**.
So farseer sees `status` and `resetsAt`, **not a gauge**. "Am I close to the limit?" is not directly answerable - only "am I currently allowed" and "when does the window turn over".
Any design that assumes a percentage is designing against a field farseer cannot read.

### 3. A run that dies on exhaustion has nowhere to go

`05` made a worker contract **immutable**, and the contract **pins the runner**.

So retrying on a different runner is **not the same run**, by this map's own rules. Which means:

- Is it a **re-scope** into a new run, which `05` already provides and which leaves an event behind?
- If so, `11`'s **rework rate** counts an exhaustion retry as rework, and rework is supposed to measure the agent doing a bad job, not the operator's subscription resetting. That would quietly corrupt a headline metric.
- And what terminal outcome does the dead run get? `ok` and `failed` are both wrong, `cancelled` means a human chose, and `23`'s `abandoned` means the manager decided it was unnecessary. **Nothing fits.**

That last point may mean a fifth outcome, or may mean exhaustion is not terminal at all and the run waits. Decide it explicitly rather than letting an implementer pick.

### 4. Does routing know about money?

`23` made a budget a **pool that draws down**, and `10` found only Claude Code reports cost in currency.

Open: whether the router may choose a cheaper runner to stay inside a budget, and whether it may choose a **better** one when budget is plentiful.
If it may, then the runner is a function of remaining budget and `13`'s definition is even less predictive of what ran.

### 5. What this must not break

- `01` - a worker may never spawn, and only a manager may call a cell. Routing is not delegation.
- `13` - the roster entry is the definition's, and the definition is a **file in git**. A router that adapts at runtime is doing something the file does not record.
- `11` - **cost per successful run by runner** and **rework rate** are both headline metrics that routing can distort.
- `12` - **reach is observed, never advertised**. Two runners with different reach are not interchangeable, so routing between them is a policy change, not a performance optimisation.
- The map's **out of scope** ruling: farseer owns **runner routing only**. A token-level model router is delegated to NeMo Switchyard or LiteLLM behind a per-runner base URL.

## Resolution

Resolved 2026-08-23 by grilling.

### 1. A roster entry names an ordered list of runners, and length 1 is the normal case

**The author asserts equivalence. Farseer never infers it.**

`12` made reach **observed, never advertised**, and `10` measured that a full-shell coding agent and an `ffmpeg` adapter that renders one file are both runners with nothing in common. Farseer cannot know two runners are substitutes.

Within a **worker kind**, an author can know. A `post-writer` may run on Claude Code or cursor-agent because the author has judged them equivalent **for that kind of work**, which is a claim a human makes deliberately and a runtime cannot make at all.

So the roster entry holds an **ordered list**, and a one-item list is a pin. Nothing changes for a simple cell.

This is additive rather than a new field, in the same way `22` added a third entry kind without touching the definition's shape, so `08`'s falsification test is not reopened.

What the definition still promises is weaker and should be stated plainly: **the definition records the candidate set, and `11` records what actually ran.**

### 2. The adapter normalises. The router has one code path.

**A runner adapter reports one of three states: `available`, `exhausted_until(t)`, or `unknown`.**

Claude Code's adapter fills this from `rate_limit_event`, which `10` measured arriving on **every successful run** with a `resetsAt` epoch.
Codex and cursor-agent adapters report `unknown`, which behaves as **available until proven otherwise** - the router tries, and a failure is what teaches it.

This is the seam pattern the rest of the map already uses: the asymmetry lives in the adapter, where it is a fact about one tool, rather than in the router, where it would be two policies that drift apart.

Levelling down was rejected for discarding the one signal farseer gets free.
Two explicit paths were rejected because the pre-emptive path would be exercised on one runner only and would rot.

**A consequence worth stating: `unknown` is not a defect to be fixed later.** Most runners will never report a window, so the three-state signal is permanent, not a transitional shim.

### 3. `failed` with reason `runner_exhausted`. No fifth outcome.

`17` already established the pattern: an orphaned `running` run becomes `finished(failed)` with reason `runtime_restarted`.

Exhaustion is the same shape - the run did not complete, nothing about the work was wrong, and a retry is appropriate but not immediately.

Two things follow:

- **`11`'s rework rate excludes `runner_exhausted`**, exactly as `23` excluded `abandoned` from the cost denominator. Rework measures the agent doing a bad job. **The operator's subscription window turning over is not the agent doing a bad job**, and a metric that conflates them is worse than no metric.
- **The retry is a new run**, per `05`'s immutable contract which pins the runner. That is correct rather than unfortunate: a different runner is a different contract, and `11` sees two runs rather than one run reworked.

Waiting in `queued` was rejected on two counts: `05` has no transition from `running` back to `queued`, and holding a workspace for up to five hours fights `04`, whose teardown depends on reaping first.

### 4. The router is budget-aware, and this is not an optimisation

**The operator overrode the recommendation here, and writing it out showed the recommendation was wrong.**

The draft argued budget should only ever **stop** a run, never **steer** it. That reasoning assumed cost is a smooth quantity where a router shaves margins. On this machine it is not.

`10` measured the actual shape: a **subscription** run costs nothing marginal until the window is exhausted, at which point the only way to continue is a **pay-per-token overflow key** that costs real money. Claude Code reports exactly this, in band, as `overageStatus` and `isUsingOverage`.

So the cost curve is not a slope. It is **a cliff at the moment of exhaustion.**

Which means budget-aware routing and quota-aware routing are **the same mechanism**: prefer the free runner until it is exhausted, then decide whether this task is worth real money. A router that ignores budget cannot express "keep working, but not at $6 per task", which on a Pro account is the single most valuable thing it could express.

Three constraints keep it honest:

- **Budget pressure may only reorder within the author's list.** It may never introduce a runner the author did not name, because that would be farseer asserting the equivalence section 1 reserved for the author.
- **A reorder emits an event.** If the record does not say why a non-preferred runner ran, `11`'s cost-by-runner numbers cannot be explained.
- **An estimated cost is marked as estimated.** `10` found only Claude Code reports currency; Codex and cursor-agent report tokens, so farseer prices them itself. A routing decision made on a farseer estimate must be distinguishable from one made on a reported figure, or a mispriced table becomes invisible.

**Where the price table lives: with the runner configuration, not the cell definition.** `13` already separated runner config from the definition, following OpenClaw's agentDir split. Pricing is per runner and machine-wide, not per cell, so putting it in the definition would duplicate it into every cell and reopen `08` for no gain.

### Tickets this informs

- `05 run state model` - a new failure reason, **`runner_exhausted`**. No new lifecycle state, no fifth terminal outcome.
- `11 analytics questions` - **rework rate excludes `runner_exhausted`**. Cost-by-runner needs the reorder event to be explicable, and needs estimated cost distinguishable from reported cost.
- `13 harness build kit` - a roster entry's runner slot holds an **ordered list**; the **price table belongs to runner config**, not the definition.
- `27 quota accounting` - **budget-aware and quota-aware routing are one mechanism**, because a subscription's cost curve is a cliff at exhaustion rather than a slope. `27` should not model them separately.
- `20 worker control channel` - the runner contract gains an **availability signal** with three states, and `unknown` is permanent rather than transitional.
