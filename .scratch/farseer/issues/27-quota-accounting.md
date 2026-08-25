# Credit and quota accounting: a subscription allowance is not money

Type: grilling
Status: closed
Blocked by: none

## Question

Graduated from the map's **Credit and quota accounting** fog on 2026-08-23, once `10` closed and supplied the observable facts.

`11` measures **cost per successful run** in dollars.
`23` made a budget a **pool that draws down**.

Neither says anything about a **subscription allowance** being consumed.
A run on Claude Pro costs the operator nothing marginal and still exhausts something finite, so **a dashboard showing only dollars reads `$0.00` for the runner that just ran out**.

The operator has asked for a command center with a utilisation widget, so this is a surface question as well as a model question.

### 1. Quota is probably not a budget dimension, and that is the crux

The tempting move is to add quota alongside `wall_clock` and `cost_usd` in `05`'s budget.

**It does not fit, and the reason is structural.**

`23` made a budget compose **down a call chain**: a callee's spend decrements the caller's pool, and defaults narrow through three layers.
That works because money is **the caller's to give**.

A subscription window is not. It belongs to an **account**, is shared by every cell and every run using that account, and is consumed by things farseer did not start - the operator's own interactive session, another tool entirely.

So quota looks like a **machine-level resource** rather than a per-call allowance, which makes it closer to `05`'s **liveness** - derived, observed, never granted - than to a budget.

Decide which it is. If it is a budget dimension, explain how it composes when two cells share one account. If it is not, say what it *is*, because `05`'s three axes have no slot for it either.

### 2. Farseer gets a status, not a gauge

`10` measured exactly what is available headless:

```json
{"status":"allowed","resetsAt":1787473800,"rateLimitType":"five_hour",
 "overageStatus":"rejected","isUsingOverage":false}
```

**`used_percentage` is not in it.** It exists, but only on the status line, which `10` proved does not fire in `-p` mode.

So "80% consumed, slow down" is **not expressible**. Farseer knows whether it is currently allowed and when the window turns over, and nothing in between.

A utilisation widget therefore cannot be a progress bar sourced from the runner.
Open: whether farseer **derives** a usage estimate from its own accumulated token counts per window, which is possible and is farseer inventing a number the provider never confirmed, or whether the widget honestly shows only **allowed / not allowed** and a countdown.

The first is more useful and can be wrong. The second is always right and much less useful. This is the decision.

### 3. Per runner, or per account?

Two runners can sit on one account, and one runner can be configured with two accounts.

The window belongs to the **account**, so accounting keyed by runner will double-count or mislead the moment the operator adds a second runner on the same login.

But `13`'s roster names runners, `11` reports **by runner**, and the operator thinks in runners.
So the natural key for display and the correct key for accounting may not be the same thing, which is worth stating rather than discovering.

Note `10` found the account identity is available: `apiKeySource` distinguishes login from key, and `overageStatus` describes the account's overflow posture.

### 4. Where does it live?

A `rate_limit_event` is a **runtime-observed fact**, which is `02`'s definition of an **event** - so it appears to belong in the log with a `seq` and an `actor` of `system`.

Two objections to settle:

- It arrives on **every single run**, so the log gains a high-frequency event whose value is almost entirely in the **latest** one. `02` built a log for history, not for current state.
- The same window state is reported identically by every concurrent run on that account, so the log would hold many rows saying one thing.

If it is not an event, then farseer holds mutable current state somewhere, and `24` just established the only non-record store - an **opaque UI blob farseer never parses**, which is plainly the wrong home for something the runtime must reason about.

### 5. What this must not break

- `11` - **cost per successful run** stays in dollars. Quota is a second quantity, never a redenomination of the first.
- `23` - a budget draws down and composes. Whatever quota is, it must not silently acquire those semantics by being stored next to one.
- `02` - **scrub on write**, and the MCP face never accepts raw event append. If quota becomes an event, an agent must not be able to forge one.
- `24` - the UI blob is opaque and farseer never parses it. Runtime state does not go there.

## Resolution

Resolved 2026-08-23 by grilling. The last decision ticket on the map.

### 1. Quota is a runner property, not a budget dimension

**A budget is allocated. A window is observed.** That is the whole distinction, and it is the third time this map has resolved a question by separating two things one word was covering.

`23` made a budget a pool a **caller** grants and a callee draws down, composing by narrowing.
Every part of that fails for a subscription window:

- **No caller owns it.** It belongs to an **account**, so there is no one to grant it and nothing to narrow.
- **It is shared** by every cell using that account, simultaneously, with no allocation between them.
- **It is consumed by things farseer never started** - the operator's own interactive sessions, and any other tool on the same login.

Farseer does not allocate a window. It **observes** one.

**And `26` already built the slot.** A runner adapter reports `available` / `exhausted_until(t)` / `unknown`, which is a property of a **runner**, not of a run. Quota accounting is that signal recorded over time, plus farseer's own consumption attributed to windows.

No new concept. No third budget field. `05`'s contract is untouched.

### 2. The widget shows farseer's own spend, and never claims to be the provider's number

`10` proved `used_percentage` exists only on a status line that **provably does not fire headless**, so "how much of my window is left" is **not answerable** and no honest surface may imply it is.

What the widget shows:

- **`allowed` or `exhausted`**, from `26`'s signal.
- **A countdown to `resetsAt`**, which `10` measured as unix epoch seconds.
- **What farseer itself consumed since the window opened** - tokens, and dollars where `10` found them reported.

The third is the useful one, and it is worth being clear about why it is not a percentage.

Farseer's consumption is a **lower bound** on window usage, because the same window is drained by sessions farseer cannot see. Presenting a lower bound as a percentage would be wrong in a way the operator could not detect, and would be **most wrong exactly when it matters** - near exhaustion, after a heavy interactive day.

**But "how much of my window is left" was never the fleet question.** "What has the fleet spent, and which cell is spending it" is, and that is exactly answerable, always true, and decomposable by cell and runner through `11`.

### 3. Account for accounting, runner for display

The window belongs to an **account**. Two runners on one login share one window, so a runner-keyed count misleads the moment the operator adds a second runner on the same account - which `10` shows is likely, since Claude Code and an ACP adapter wrapping it are two runners on one login.

**Runner config names an `account` string. Runners sharing the string share a window.**

Declared by the operator, never inferred. `12`'s rule holds: farseer does not deduce a fact about reach or identity that it cannot observe, and nothing in `10` makes account-sharing reliably detectable.

Display stays keyed by **runner**, because `11` reports by runner and the operator thinks in runners. **The correct key for accounting and the natural key for display are different, and that is fine as long as it is deliberate.**

The price table from `26` lives in the same place, so runner config now holds pricing and account identity - both machine-wide facts, neither belonging in a cell definition.

### 4. Append on change, derive current

`10` measured `rate_limit_event` arriving on **every** successful run, and every concurrent run on one account reports the same window identically.

**Farseer observes every run and appends only when the state differs** - a status flip, a new window, or a changed `resetsAt`.

Two properties follow, and both are improvements rather than compromises:

- **The log records window transitions**, which is what analytics actually wants. "How often did this account exhaust, and for how long" is a scan of a handful of rows rather than an aggregation over every run ever executed.
- **Current state derives from the latest event**, which is the trick `05` already used for liveness: derived from a timestamp, never stored. No mutable runtime state, so `24`'s ruling that the opaque blob is the only non-record store stays true.

The event is `02`-shaped with `actor: system`, and per `02` the MCP face never accepts raw event append, so an agent cannot forge a window state to escape routing.

### What this cost the primitive

**Nothing.** No new budget field, no new store, no new lifecycle state, and no new concept - the runner property came from `26`, the derive-don't-store pattern from `05`, and the event shape from `02`.

That is the last question on the map, and like `22` before it, it added no fields.

### Tickets this informs

- `26 routing policy` - the availability signal is now also the **accounting** primitive, and its history is the quota record. One mechanism, as `26` predicted.
- `02 record scope` - a **`rate_limit_event`** kind with `actor: system`, appended **on change only**. Current window state is derived from the latest, never stored.
- `11 analytics questions` - fleet spend per window is decomposable by cell and runner. **Window usage percentage is not available and must never be presented**, since farseer's own consumption is only a lower bound.
- `13 harness build kit` - runner config holds the **account string** alongside `26`'s price table. Neither belongs in a cell definition.
- `16 local API surface` - the utilisation surface reads `allowed`/`exhausted`, a `resetsAt` countdown, and farseer's own spend. It must not expose a percentage of the provider's window.

## Implementation note, 2026-08-25

Built, and the shape this ticket chose survived contact unchanged: no new budget field, no new lifecycle state, no mutable current-state table.

- `farseer-core`'s `quota.rs` holds `Availability` - `26 routing policy`'s `allowed` / `exhausted_until(t)` / `unknown` - and `WindowObservation`, which knows how to say whether it is a **transition** rather than a repeat.
- `farseer-core`'s `runners.rs` holds runner config, and `farseer serve --runners runners.toml` loads it. An absent file is an empty config, because declaring accounts sharpens accounting and was never a precondition for running anything.
- `farseer-store`'s `quota.rs` appends on change and derives current from the latest event, exactly as `05 run state model` derives liveness.
- `GET /v1/quota` is the surface: `allowed` / `exhausted_until` / `unknown`, a `resets_at` to count down to, farseer's own spend since the window opened, and the runners on the account. Tests assert the **absence** of a percentage in the payload, in the store row and in the observation, because that is the rule most easily lost to a later well-meaning edit.

### An undeclared runner is its own account

This ticket said the account is declared and never inferred, and left open what happens before the operator declares one.

An undeclared runner is keyed by **its own name**.
That is not an inference: it declines to merge two runners rather than guessing they share a login.
The failure mode is two windows displayed where there is really one, which the operator can see and fix with one line of config.
The opposite guess would silently merge two accounts and misreport both, and it would be invisible.

### Anything that is not `allowed` is treated as exhausted

`10 runner inventory` captured `allowed` and no other status string.
Guessing which other values are benign would be inventing reach farseer has not observed, so the mapping fails to `exhausted_until`, which is the direction `26 routing policy` routes **away** from rather than into.

### A cancelled run still reports the window it saw

The observation is attached after the stream ends rather than at the terminal result, because `10 runner inventory` observed `rate_limit_event` arriving around the terminal result rather than before it.
A cancelled run keeps it: the window it saw was real, and cancelling the run does not unsee it.

### Not yet wired

`26 routing policy` itself. The availability signal exists and is recorded, but nothing routes on it yet.
