# Attach: to a worker or to a run, and what does intervention do?

Type: grilling
Status: closed
Assignee: izanorshahril
Blocked by: none

## Question

The operator privilege firstmate could not deliver on Windows.

- Do you attach to a **worker** (a live process) or to a **run** (the durable record)? Runs survive restarts; workers do not. Attaching to a run means dead sessions stay browsable through the same UI.
- Read-only tail by default with an explicit takeover, or immediate interactive control?
- After you intervene, what happens to the manager's plan: told and re-plans, contract void and a fresh report required, or unaware unless the outcome changes?
- May you attach to a worker inside *another* cell, past two managers? The proposal says yes, unconditionally.
- Is a PTY view needed at all, or is a rendered event stream with input injection enough?

## Resolution

Decided 2026-08-19 with the operator, over two grilling rounds.

### 1. Attach targets a run, not a worker

`farseer attach <run>` subscribes to a **run**, and the run may or may not currently have a live worker behind it.
If a worker is alive the subscription continues transparently into live output.
If it is dead the subscription is replay only.

One address, one URL, one UI surface for live and historical work.
Links do not break when the runtime restarts, which they would if attach targeted a process.

This is also forced by `16 What is the local API surface?`: replay-then-live and attach-mid-run are the same question, and must not be given two different answers.

### 2. A run is one worker contract's execution

A **run** is exactly one worker contract's execution, and exactly one record entry.

The operator's whole request is a **task**, which spans many runs and needs its own separate word.
Fixing the final words is `14 Vocabulary and naming lock`, which already flags run, session, task and contract as four things never to be conflated.

### 3. Read-only by default, explicit takeover

Attach is read-only tail by default.
Interactive control requires an explicit takeover.

Three reasons:

- Attach is used mostly to watch, and stray keystrokes into a live agent are destructive.
- Takeover must be a recorded event, carrying who, when and what was sent, which needs an explicit boundary to hang the event on.
- Silent injection makes the manager's model of its own worker wrong, with no marker anywhere.

Cost to the operator is one keystroke.

### 4. Cross-cell attach is scoped to records farseer owns

`ARCHITECTURE.md` rule 4 is amended.

**Attach reaches any worker in any cell whose record farseer owns.**

For native cells this stays unconditional, past any number of managers, which is the operator privilege firstmate could not deliver on Windows.
A **peer cell** is a foreign orchestrator over A2A whose internals farseer cannot see, per `01 Is the cell the right primitive?`.
Its workers are therefore not attachable.
Only its call is observable: request, status, result.

Left unqualified, rule 4 promises something the architecture cannot deliver.

### 5. Rendered event stream, no PTY in the runtime

Attach renders a structured event stream and injects input into it.
The runtime never exposes a raw PTY.

- `ARCHITECTURE.md` section 5 already picked ACP (Zed) for structured control without a PTY, and PTY handling on Windows is the exact failure catalogue in `BRIEF.md`.
- A PTY cannot be meaningfully replayed from the record without storing raw ANSI and reconstructing terminal state, which breaks decision 1.
- A browser prototype UI, which is why `01` chose HTTP plus SSE, would otherwise need a full terminal emulator.

**Terminal-only runners**: some runners have no ACP adapter and are only drivable through a terminal.
For those, **the adapter owns the PTY and translates to events**.
One attach surface either way.

### 6. Intervention tells the manager, and the manager decides

Intervention appends an `operator_intervened` event to the run, carrying what the operator sent.
The manager reads it on its next wake and decides for itself whether the goal changed and whether to re-plan.
The contract is not voided.
The run's result carries an `operator_touched` flag permanently, so neither the manager nor the record mistakes the outcome for autonomous.

Rejected alternatives:

- **Manager unaware unless the outcome changes.** Makes the manager confidently wrong about its own worker.
- **Contract void, fresh report required.** Makes the cheapest and most common intervention, nudging a stuck worker back on track, expensive.
- **Operator declares whether the nudge changed the goal.** Friction, and unnecessary: the manager holds both the contract and the operator's message, so it can judge.

Operator note: this matches how current coding agents and firstmate already behave, where the manager steers or starts another worker in parallel if it is unblocked or needed.

### 7. Takeover is a released mode, and cancel is a separate verb

A run has three control states:

- `autonomous`
- `observed`, meaning a read-only attach is present
- `taken over`

Release returns the run to `autonomous` and the worker carries on.

Two consequences to write into the spec:

- **Detaching without releasing must auto-release after a timeout.** Otherwise a closed terminal or dropped connection freezes a worker waiting on a human forever, which is exactly the class of hang `BRIEF.md` catalogues.
- **Cancel is a separate verb.** Taking over never kills a worker, and killing one is not a form of intervention.

### Open item, ticketed

The operator has not checked how comparable tools detect hangs and handle observation and takeover, naming Traycer as an example.
That does not block anything decided here, but it can refine the timeout and hang-detection design.
Raised as `18 How do comparable tools detect hangs and handle observation and takeover?`.

### Terms settled, for the vocabulary lock

`run`, `task`, `attach`, `observed`, `taken over`, `autonomous`, `operator_intervened`, `operator_touched`.

## Built, 2026-08-31

`POST /v1/runs/{id}/{observe,take-over,release,heartbeat,intervene}` and `GET /v1/runs/{id}/control`, with the axis on every run view beside lifecycle and liveness.

**The lease lapses on read, not on a timer.**
Section 7 requires that detaching without releasing auto-releases, and a swept lease needs a task that a busy or restarted farseer might not run - so control is derived the way `05` derives liveness, and a lease nobody renewed is already autonomous the moment anyone asks.
The lapse is appended to the record like any other release, because the agent getting the wheel back is the fact, whoever caused it.

**`steer` and `intervene` are two verbs on purpose.**
Both put the operator's words into a run, so both mark it `operator_touched` and append `operator_intervened`, per section 6.
They differ in what is on the other end: a manager is a conversation the operator is having and `05` put `steer` on it as the way to talk, while a worker is executing a contract somebody else wrote - and typing into one unannounced is exactly what section 3 refused.
So the Runs widget offers `steer` on a manager, and on a worker offers `take over` first and `intervene` only once the takeover exists.

**Attach works on a finished run**, per section 1: the widget offers `observe` whatever the lifecycle, because the subscription is the event stream and replay is the same address as live.

**Not built.** Cross-cell attach needs no code - the record is farseer's and the stream already filters by run - but a peer cell's workers are not attachable and there is no A2A endpoint yet to make that distinction observable.
