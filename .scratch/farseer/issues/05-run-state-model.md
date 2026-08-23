# Run state model and control semantics

Type: grilling
Status: closed
Blocked by: none

## Question

Split from the original `05` on 2026-08-22.
The control-channel research half moved to `20 Worker control channel: ACP (Zed) first, or headless JSON per harness?`, because a research ticket and a grilling ticket bundled together cannot resolve in one session.

This ticket fixes farseer's own vocabulary for a running worker, above and independent of whatever transport `20` picks.

Settle:

- The **run state model**. Five states have arrived from three tickets and need reconciling into one coherent shape.
- The **three numbers**: the `stalled` threshold, the `likely-hung` threshold, and the auto-release timeout from `07`.
- Who owns **retry and escalation** when a workspace will not tear down.
- What a **worker contract** actually contains, since `07` defined a run as "one worker contract's execution" without saying what a contract is.

## Carried from earlier tickets

From `07 Attach: to a worker or to a run, and what does intervention do?`:

- A **run** is one worker contract's execution, and exactly one record entry.
- Control states `autonomous`, `observed`, `taken over`, with release returning to `autonomous`.
- Detaching without releasing auto-releases after a timeout, so a dropped connection never freezes a worker waiting on a human.
- **Cancel is a separate verb** from takeover. Taking over never kills a worker.
- Intervention appends `operator_intervened` to the run and sets `operator_touched` on the result. The contract is not voided, and the manager decides whether to re-plan.

From `18 How do comparable tools detect hangs and handle observation and takeover?`:

- Liveness is a **no-progress-event watchdog**, not a stdout watchdog. It tracks the last structured event per worker: tool call start, tool result, status change. This is what makes it work with no PTY.
- **120 seconds** with no progress event marks the run `stalled`. **600 seconds** flags it `likely-hung`.
- **No auto-kill at either threshold.**

So this ticket inherits five run states to reconcile into one model: `autonomous`, `observed`, `taken over`, `stalled`, `likely-hung`.
Note that `stalled` and `likely-hung` are orthogonal to the control states, so they are probably a second axis rather than more values on the first.
Decide that explicitly rather than by accident.

The auto-release timeout from `07` is still unset, and belongs here alongside the other two numbers.

Findings: [hang-detection-and-attach.md](../research/hang-detection-and-attach.md).

## Carried from 03

`03 Spike: Win32 Job Object kill-on-close with a real harness child` confirmed the reap and produced two facts this contract must absorb.

**Process identity is `(pid, creation_time)`, never a pid alone.**
Windows recycles pids aggressively, and the spike terminated an unrelated desktop application by treating a reused pid as a tree member.
Wherever this contract names a process - in run state, in a kill path, in anything written to the record - the identity is the pair, or better, job membership, which the kernel tracks and which cannot go stale.
An inference that goes wrong here does not fail safe.

**Reap latency is sub-millisecond**, roughly 300 to 400 microseconds for a five-deep tree.
There is no teardown grace period to design.
Cancel can be treated as effectively instantaneous, and any reap timeout is a safety net for a pathological case, not a normal-path budget.

So the numbers this ticket must set are still `stalled` at 120s, `likely-hung` at 600s, and the auto-release timeout from `07`.
Teardown is not among them.

## Carried from 04

**Teardown ordering is a hard constraint, not a preference: reap the job, then delete.**
Deleting without reaping does not fail cleanly - it removes files out from under a live process that survives, leaving a running dev server pointed at a half-deleted tree.

Measured: 0 failures in 60 supervised cycles, p50 2.5ms, p95 38.5ms.
The proposed quarantine-by-rename fallback **does not work** and was removed: a directory held open as a process cwd cannot be renamed any more than it can be deleted.
So a workspace that will not delete is a state the operator must be shown, not a state to paper over.
This contract should name that state and say who owns the retry.

## Resolution

Resolved 2026-08-22 by grilling.

### 1. Three axes, not one enum, and the third is never stored

The five states that arrived from `07` and `18` were never one enum. They are three independent axes.

| Axis | Values | Owner |
| --- | --- | --- |
| **Lifecycle** | `queued` -> `running` -> `finished(ok / failed / cancelled)` | the runtime |
| **Control** | `autonomous` / `observed` / `taken over` | whoever is attached |
| **Liveness** | `live` / `stalled` / `likely-hung` | **derived, never written** |

Liveness is computed from `now - last_activity_at`.
Storing it would create two sources of truth that can disagree, and a crash mid-transition would leave a run permanently marked `stalled` when it is not.
As a computed view it is always correct and costs nothing to persist.

A run therefore has one timestamp driving liveness, not a state machine.

### 2. The liveness clock pauses whenever control is not `autonomous`

A watchdog exists to catch a worker that stopped making progress **on its own**.
A human thinking for three minutes while taken over is not that, and flagging it would be farseer blaming the operator for the operator's own pause.

`observed` deliberately does **not** pause the clock.
Watching is passive, the agent is still driving, and that is precisely when the watchdog should be live.

### 3. Activity and progress are two different signals

This is the correction that came out of the grilling, and it invalidates how `18` defined the watchdog input.

`18` defined progress as tool call start, tool result, status change.
A high-end model reasoning for twenty minutes straight emits none of those, so under that definition it would be flagged `likely-hung` at 600 seconds while working perfectly.

The fix is to split one concept into two:

| | fires on | drives |
| --- | --- | --- |
| **activity** | any bytes from the adapter - token stream, adapter heartbeat | the **liveness watchdog** |
| **progress** | tool call start, tool result, status change | the **record**, the fleet view, analytics |

**The watchdog keys on activity, not progress.**
Mechanical silence is a hang. Thinking hard is not.

Two consequences.

**The "spinning productively-looking forever" case is not the watchdog's job.**
A model streaming tokens in a loop for an hour is `live`, and correctly so.
That is what the contract's **budget** catches - wall clock, tokens, cost.
The watchdog catches silence, the budget catches waste. Different instruments, and conflating them is how you get a watchdog that either kills good runs or ignores bad ones.

**Activity is inferred from bytes, with an optional adapter heartbeat as the escape hatch.**
Any byte from the adapter is activity.
Most harnesses stream, so most adapters do nothing at all.
An adapter whose harness goes silent while working may emit an explicit heartbeat.
This keeps the rule simple and puts the cost only on the adapters that actually need it, rather than taxing every adapter for one harness's behaviour.

### 4. The three numbers

- **`stalled` at 120 seconds** with no *activity*.
- **`likely-hung` at 600 seconds** with no *activity*.
- **Auto-release is not a timeout at all. It is a heartbeat**: 30 second client heartbeat, release after two missed beats, so 60 seconds.

A wall-clock auto-release timeout has to choose between releasing an operator who is still there and freezing a worker for a client that is already gone.
A heartbeat distinguishes those two cases, which is the entire point of the mechanism.

All three are configurable.
**None of them kill anything.** Cancel remains a separate, explicitly operator-driven verb, per `07`.

### 5. The worker contract is an envelope, and it is immutable

`07` defined a run as "one worker contract's execution" without ever saying what a contract contains.

A contract is: **goal, workspace, runner, tool grants, autonomy level, budget, definition-of-done.**

It is immutable for the life of the run.
Immutability is what makes the record answerable after the fact: "what was this worker allowed to do" has one answer, not a timeline of answers.
`07` already established that intervention does not void the contract, which only holds if the contract cannot drift.

**Steering is not a mutation of the contract.**
The manager may steer mid-run, and should be able to, because every current harness accepts follow-up turns in a session.
A steer appends a `manager_steered` event to the run and changes nothing about the envelope, exactly parallel to how `07` treats `operator_intervened`.

That line holds because it matches what harnesses actually permit: they all take follow-up instructions, and none of them let you retroactively widen tool permissions mid-turn.
Steering moves **within** the envelope. Changing the envelope is a new run.

So the manager has four verbs, not one:

- **steer** - same run, same contract, new instruction.
- **re-scope** - new run against the same task, because a contract field changed.
- **cancel** - end it.
- **re-run** - new run, fresh workspace.

### 6. `cancelled` is never `failed`

They mean opposite things to the manager.
`failed` invites a retry. `cancelled` says a human already decided not to.
Conflating them produces an auto-retry loop that fights the operator.

Budget exhaustion is **`failed`**, not `cancelled`, because nobody chose it.

### 7. Intervention does not change autonomy

After takeover and release, the worker returns to `autonomous` with its autonomy level unchanged.
The result carries `operator_touched` permanently, per `07`.

Silently tightening autonomy because a human helped would punish intervention and teach the operator not to intervene.
`operator_touched` is a fact about **provenance**, not a reason to restrict what happens next.

### 8. A stuck workspace belongs to the cell, not the run

From `04`: the stuck case is real but rare, 0 in 60 cycles, and the quarantine fallback cannot work.

A run whose work is done must not stay open because of a directory.
So the run finishes normally, and the workspace gets its own small state: **`live` / `orphaned`**.

- The runtime **sweeps orphans at startup**, when nothing holds them.
- Anything surviving a sweep is **surfaced to the operator**.
- Retry belongs to the sweep, never to the run.

Teardown ordering from `04` stands as a hard constraint: **reap the job, then delete.**
Process identity from `03` stands wherever this contract names a process: **`(pid, creation_time)`, or job membership, never a pid alone.**

### 9. Contract tests handed to `20`

Two of these are disqualifiers, not quality concerns.

1. **The channel must emit activity at least every N seconds, or expose an explicit "still working" signal.** A channel returning only a final blob makes thinking and hanging indistinguishable, so farseer cannot supervise it at all. **Hard disqualifier.**
2. **The channel must emit the three progress event kinds** - tool call start, tool result, status change - with enough fidelity to drive the record. **Hard disqualifier.**
3. **The channel should accept a mid-run instruction without tearing down the session.** A harness that cannot is usable but degraded: the manager falls back to cancel-and-re-run, losing the session's context. Quality concern, not a disqualifier.
4. The channel must surface a distinguishable cancellation, so `cancelled` and `failed` do not collapse into one outcome.

### Tickets this informs

- `20 worker control channel` - inherits the four contract tests above.
- `16 local API surface` - inherits three axes to expose, and the fact that liveness is derived rather than stored, so it is computed at read time and not a field the API writes.
- `02 record scope` - inherits the activity / progress split. Progress events go to the record; activity is liveness bookkeeping and is not a record entry.
- `14 vocabulary lock` - inherits `activity`, `progress`, `contract`, `envelope`, `steer`, `re-scope`, `orphaned`.

## Corrected by 20

Section 5 above says the manager may **steer** mid-run, reasoning that every current harness accepts follow-up turns in a session.
The reasoning was right and the word was too strong.

`20 Worker control channel` surveyed the actual channels and found **no harness supports interrupting a turn already in flight**:

- **ACP is strictly sequential.** The client must receive a `stopReason` before it may send another `session/prompt`. There is no provision for concurrent turns.
- **`codex exec resume`** replays persisted events into a **new process**. That is resumption, not steering.
- **Claude Code `-p`** is one-shot, continued by `session_id`.

So the corrected definition:

- **Steer** = queue an instruction delivered at the **next turn boundary**. Same run, same contract, session context preserved. Universally available.
- **Interrupting a turn in flight is not available anywhere**, and farseer must not promise it. The nearest thing is cancel-then-prompt, which loses the in-flight turn.

The decision stands. The promise is narrowed, and a steer's latency is bounded by the current turn rather than being immediate.

## Amended 2026-08-23: compaction is a known quiet window

Findings: [context-compaction.md](../research/context-compaction.md).

The watchdog above keys on **activity**. A **context compaction is silence.**
During a server-side compaction the client waits on a remote call that streams no tokens and makes no tool calls, which is exactly the shape `stalled` at 120s is built to catch.

So a long compaction on a large conversation can trip a **false `stalled`**, and a pathological one a false `likely-hung` - on precisely the longest and most expensive runs, which are the ones an operator least wants misreported.

**Resolution: pause the liveness clock during a known compaction**, reusing the mechanism this ticket already defines for pausing it while control is not `autonomous`.
The alternative - emitting an activity event at compaction start - depends on a harness signalling the start, which Claude Code's `compact_boundary` may not do.
Reusing existing machinery beats depending on someone else emitting something extra.

**Farseer never compacts.** It does not own the conversation, the runner does.

## Renamed by 14

`14 Vocabulary and naming lock` retired **envelope** as a noun.

This resolution calls the worker contract "an envelope", and `06` calls the cell-call payload "the envelope" - two different things, overlapping fields, one word.
That is exactly the ambiguity `14` exists to prevent.

The prose below is left as written, because rewriting a decided resolution risks changing its meaning.
Read it with the final words substituted:

- **worker contract** - what a manager gives a worker. Named "envelope" below.
- **cell call** - what a manager sends another cell's manager. Named "envelope" in `06`.

Also renamed: **seat** is now **runner**, applied throughout.

## Amended by 23

The lifecycle axis gains a **fourth terminal outcome**.

`queued` -> `running` -> `finished(ok / failed / cancelled / **abandoned**)`

**`abandoned`** means the **manager** decided the run was unnecessary before it started.
`cancelled` was wrong because this ticket was explicit that it means a **human** chose; `failed` was wrong because nothing broke and it invites a retry that should not happen.

One enum value, and it keeps a planning decision visible rather than deleting the run - which would also have fought `02`'s append-only record.

Also from `23`: **budgets are drawn down, not compared.**
A ceiling is a level and is checked once; a budget is a quantity, so a callee's spend **decrements the caller's remaining pool** rather than merely being compared against its original figure.
That distinction is what stops three sequential $2 calls spending $6 under a $2 parent.
