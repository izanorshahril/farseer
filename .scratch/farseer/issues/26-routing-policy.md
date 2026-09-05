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


## Corrected by 30 (2026-08-26)

**Section 2's asymmetry is smaller than it measured, and this ticket is unwired, so the correction arrives before anything was built on it.**

Section 2 rests on `10 runner inventory`'s measurement that **Claude Code alone** reports quota in band, and that Codex and cursor-agent are knowable only after a run fails - then offers two designs, "level down" or "exploit it", with the second costing two code paths that will drift.

That is true of `codex exec`. It is **false of `codex app-server`**, which pushes `account/rateLimits/updated` after every turn with `usedPercent`, `resetsAt` and `windowDurationMins` for two windows.

So the choice this ticket framed - one observable runner against two blind ones - is now two observable runners against one blind one, and the observable ones **do not report the same shape**. "Level down" gets more expensive as more runners can see; "exploit it" was priced against one exception and now has two.

Neither design is invalidated. The number they were being weighed against changed, and this ticket should be re-read before it is wired rather than wired as written.

---

## Built 2026-08-28, all four sections

Sections 1 to 3 went in first and section 4 followed in the same day; the note below was written between the two and its "not built" section is superseded at the foot of this page.

### The roster entry holds the list, and one item is still a pin

`runner = "pi"` and `runners = ["pi", "omp"]` are the same field, deserialised through a string-or-sequence helper.
That is not a legacy alias to be removed later: this ticket found a one-item list is the normal case, and making every cell write brackets to say the ordinary thing would tax the common path for the benefit of the rare one.

`cells/zero.toml`'s `reviewer` now names `["pi", "omp"]`, which is the author asserting equivalence **for that kind of work** - the claim `10 runner inventory` says a runtime cannot make.

### The router, and `unknown` doing the work

`first_available_runner` takes the author's order and returns the first candidate whose account has no spent window.
Exhaustion is read from the record - `27 quota accounting`'s on-change window log - and expanded through `runners_on`, so **an account being spent takes every runner on it out of the running at once**, which is the whole reason `27` keyed by account rather than by runner.

A runner farseer has never seen a window for is not in the exhausted set, so it stays eligible.
That is section 2's `unknown` behaving as available until proven otherwise, and it is the common case rather than a gap: most runners will never report a window.

### `runner_exhausted`, and the reorder that explains itself

Every candidate spent is `runner_exhausted` on the delegation, not a fifth outcome.

When a non-preferred runner is chosen, farseer appends a `status_changed` event with `actor: system` naming the preferred and the chosen.
Section 4 asked for that in the budget case; it is needed in this case for the same reason, and the reason is the load-bearing part - **if the record does not say why a non-preferred runner ran, `11 analytics questions`'s cost-by-runner numbers cannot be explained.**

### Not built: section 4, the budget-aware reorder

The price table this ticket put "with the runner configuration" **does not exist yet**.
`crates/farseer-core/src/runners.rs` says so in a comment on `RunnerEntry::account`: absent rather than stubbed, per `13 harness build kit`.

So farseer routes on availability today and not on money.
The cliff this ticket identified - a subscription costs nothing marginal until the window turns over, then only a pay-per-token key continues - is real and unaddressed.
Whoever builds it inherits section 4's three constraints unchanged, and the first is the one to hold on to: **budget pressure may only reorder within the author's list**, never introduce a runner the author did not name.

### Also still true

`11 analytics questions` must exclude `runner_exhausted` from rework. Nothing enforces that yet, and rework is not computed from this reason today.


---

## Section 4, built the same day

### The trigger is observed, not estimated

The draft of this build was going to price a run in advance and reorder when the estimate pressed on the remaining budget.
That needs the one number nobody has: what a run will cost before it runs.

Section 4 already contained the better answer and it took a second reading to see it.
The cost curve **is not a slope**, it is a cliff: a subscription run costs nothing marginal until its window turns over, and past that the only way to continue is a pay-per-token key that costs real money.
`is_using_overage` is the provider stating which side of that cliff a runner is on - `10 runner inventory` measured Claude Code reporting it in band, and it has been a field on `WindowObservation` the whole time.

So routing prefers a candidate not on overage, and no estimate enters the decision at all.

**Preference, never exclusion.** A runner on overage still runs when it is the only one left, because this ticket is explicit that budget pressure may **reorder within the author's list** and never add or remove a runner the author named.

### The price table exists, and only for the record

`RunnerEntry::usd_micros_per_mtok`, absent for every runner until an operator writes one down.

One blended rate rather than input and output separately, because that is the granularity farseer can observe: pi and omp report a total token count and no split.

A run that reported tokens and no currency is costed from it and carries **`cost_estimated: true`**, which is this ticket's own constraint - a routing decision made on a farseer estimate must be distinguishable from one made on a reported figure, or a mispriced table becomes invisible.
A runner that stated its own figure is never overwritten by the table, and with no price at all the field stays **absent rather than zero**, per `10 runner inventory`.

### The reorder event says less than it could

It records `preferred_runner_unavailable` with the preferred and the chosen, and does not say which of the two pressures moved it.
Exhaustion and overage are read in one pass and the distinction is not reconstructed.
`11 analytics questions` needs to know a reorder happened and what ran; a confident wrong reason would be worse than an honest vague one.

### The rework exclusion, and a departure from section 3

Section 3 asked for `failed` with reason `runner_exhausted`, on `17 cell lifecycle`'s precedent of an orphaned run becoming `finished(failed)` with reason `runtime_restarted`.
**That is not what was built, and the difference is the point.**

`17`'s run had started: a process existed and its outcome was genuinely unknown, so a row saying so is the honest record of something that happened.
An exhausted delegation starts nothing - no contract sealed, no workspace, no process.
Writing a run row would put a run into `11 analytics questions`'s denominators that never ran, in order to record an event that is not about a run at all.

So it is a `status_changed` event on the manager's own run, naming the worker and every candidate.
"How often did exhaustion block work" is a scan of those, rather than an outcome filter over phantom rows.

And the consequence section 3 wanted arrives for free.
With no run and no rescope edge, `runner_exhausted` **cannot** reach `rework_depth`, whose chain walks `rescoped_from`.
`11`'s exclusion is true by construction rather than by a rule somebody has to keep remembering - which is the only kind that survives.

`26` is closed.
