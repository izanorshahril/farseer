# Codex app-server: farseer has been driving the cut-down face of a runner it already depends on

Type: research
Status: closed
Blocked by: none

## Question

`29 harness protocol` decided the rule **use the richest face a harness offers**, and kept Codex on its native adapter because `codex exec --json` reports things ACP does not.

That was the right rule applied to a face nobody had looked at.
`codex exec` is the **cut-down** face. `codex app-server` is the real one, and this ticket asks what farseer is leaving on the table.

Answered 2026-08-26 from two primary sources, neither of them documentation:

- **`codex app-server generate-json-schema --out <dir>`** - Codex generates its own protocol schema, so the method list and every payload shape is exact rather than transcribed.
- **A live probe** of `codex app-server` 0.149.1 on this machine: `initialize` -> `thread/start` -> `turn/start`, twenty-seven captured lines.

## 1. The surface

**95 client methods** and **75 server notifications**.
For comparison, farseer's Codex adapter reads one stream and recognises four line types.

The methods that matter here:

| | |
|---|---|
| `thread/start`, `thread/resume`, `thread/fork`, `thread/rollback` | a thread is durable and branchable |
| `turn/start` | takes **`model`**, **`effort`**, `approvalPolicy`, `sandboxPolicy`, `cwd`, `personality`, `serviceTier` |
| `turn/steer` | takes **`expectedTurnId`** - a steer *into a running turn*, guarded by optimistic concurrency |
| `turn/interrupt` | cancel that the runner acknowledges |
| `thread/compact/start` | compaction farseer can **ask for** |
| `account/rateLimits/read`, `account/usage/read` | quota farseer can **ask for** |
| `model/list`, `config/read` | what this installation can actually do |

## 2. Three notifications that correct closed tickets

All three arrived **unprompted, headless, in a six-second turn**.

### `thread/tokenUsage/updated` - the denominator, and both scopes

```json
{"tokenUsage":{
  "total":{"totalTokens":22287,"inputTokens":22281,"cachedInputTokens":0,
           "cacheWriteInputTokens":0,"outputTokens":6,"reasoningOutputTokens":0},
  "last":{"totalTokens":22287,"...":"..."},
  "modelContextWindow":258400}}
```

`28 operator surface` asked for context info and got a token count with no denominator.
`29 harness protocol` then argued that ACP's `used`/`size` was the answer and that the per-turn breakdown was the wrong thing to build.

**Codex sends both, at the two scopes the argument was about**: `last` is the turn, `total` is the session, `modelContextWindow` is the denominator.
So the `28`-vs-`29` disagreement was never a disagreement about what to report - it was two adapters each able to answer half.

### `account/rateLimits/updated` - and it carries a percentage

```json
{"rateLimits":{"limitId":"codex","planType":"plus",
  "primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":1787710593},
  "secondary":{"usedPercent":0,"windowDurationMins":10080,"resetsAt":1788273509},
  "credits":{"hasCredits":false,"unlimited":false,"balance":"0"}}}
```

This corrects **three** tickets at once.

- **`10 runner inventory`** measured that "Codex and cursor-agent emit nothing" about quota, and that exhaustion is knowable only after a run fails. True of `codex exec`. **False of `codex app-server`**, which pushes a snapshot after every turn.
- **`26 routing policy`** built its whole design on that asymmetry - *"two coherent designs: level down, or exploit it"* - with Claude Code as the only observable runner. The asymmetry is smaller than `26` believed, and `26` is unwired, so this lands before anything was built on it.
- **`27 quota accounting`** is the sharp one. It says farseer sees no gauge: *"`used_percentage` exists but only on the status line, which provably does not fire headless"*, and **`GET /v1/quota` never reports a percentage, with tests asserting its absence**.

`usedPercent` fires headless here, twice, for two windows.

**`27`'s rule survives, and its reasoning is worth re-reading rather than deleting.** The percentage `27` refused was one farseer would have *computed from its own spend*, which is a lower bound on a window that other sessions drain - most wrong exactly near exhaustion. This percentage is **the provider's own**, which is a different number arrived at a different way, and `10 runner inventory`'s observed-never-advertised rule admits it. The tests asserting absence should keep asserting absence of a **derived** percentage.

Also present and absent from every other runner: two windows at once (5-hour and weekly), a plan type, and a credit balance.

### `thread/compacted` - a boundary, from a second runner

`10 runner inventory` scored "does this harness say when it compacted" as its own column and found **only Claude Code** did.
`29 harness protocol` found ACP still has no portable compaction boundary at all.

Codex has `thread/compacted` as a notification and `thread/compact/start` as a method - it says when, and it lets farseer decide when.

## 3. What else the probe showed, unasked

- **`effort: "low"` was accepted on `turn/start`.** The "thinking level" `28 operator surface` reported as unreportable is **settable per turn**, and `29` was right that it was unrequested rather than unavailable.
- **`item/agentMessage/delta`** streams fragments, and `item/completed` carries the assembled `text`. Same shape as ACP, and farseer already learned this lesson once: `RunnerSignal::OutputChunk` exists, and the terminal item is the honest answer.
- **The operator's own hooks ran, and two failed.** `hook/started` / `hook/completed` reported three `sessionStart` hooks from `~/.codex/hooks.json`, of which `session-start:1` and `:2` came back `failed`.
  `10 runner inventory` already warned that a runner inherits the operator's configuration unless the adapter prevents it - a one-word prompt once cost $0.32 loading plugins nobody granted. This is that, visible for the first time **as events farseer could record**.
- **Three MCP servers started** (`node_repl`, `codex_apps`) from the operator's own config, before farseer said anything.
- `22281` input tokens for "Say hello in one short sentence." That is the operator's environment, not the goal.

## 4. Decision

**Codex moves to `app-server`, as a distinct runner rather than a replacement.**

`29 harness protocol` set the precedent and it applies unchanged: `goose` and `goose-acp` are two runners because they report different things, and `codex` and `codex-app-server` are two runners for the same reason and by a wider margin.

The order to build it in, cheapest first:

1. **The read half.** `thread/tokenUsage/updated` -> `usage_updated` with a real `modelContextWindow`; `account/rateLimits/updated` -> `27`'s `WindowObservation`, which needs a second window and a percentage field it does not have; `thread/compacted` -> `context_compacted`; `item/*` -> chunks and the assembled answer.
2. **The write half.** `initialize` -> `thread/start` -> `turn/start`, which is the same shape as `acp_drive`'s handshake and should reuse its `Channel::Acp` machinery rather than grow a parallel one.
3. **`effort` on the contract.** This is the one that needs a decision elsewhere: reasoning effort is a property of *how a run is executed*, and `05 run state model` made the worker contract immutable. It is a contract field or it is a runner default, and guessing is how a field ends up meaning neither.
4. **`turn/steer` with `expectedTurnId`.** `20 worker control channel` concluded steering is **turn-boundary granular** after measuring the runners of the day. Codex now offers mid-turn steering with optimistic concurrency, which is the first evidence against that and should be recorded as a **correction to `20`** rather than quietly used.

### Not decided here

- ~~Whether `codex exec` stays at all once the app-server runner exists.~~ **Answered 2026-08-29: it stays.** See below.
- What to do about the **inherited environment**. The hooks and MCP servers are the operator's, and farseer now has events for them - `12 autonomy and deny list` should probably have an opinion about a runner farseer spawns loading hooks farseer did not grant.
- Whether the app-server's long-lived process changes `19 rust toolchain`'s one-process-per-run assumption. It is a *server*: `thread/start` is cheap, and the same process could hold many threads. That is a different lifecycle from anything on this map.

## Sources

Both generated on this machine, 2026-08-26, against `codex-cli 0.149.1`:

- `codex app-server generate-json-schema --out <dir>` - `ClientRequest.json`, `ServerNotification.json`, and the payload definitions quoted above.
- A live `codex app-server` probe: `initialize`, `thread/start` with `sandbox: read-only`, `turn/start` with `effort: low`. Twenty-seven lines, quoted verbatim above.


## Implementation note, 2026-08-26: the read half, wired and proven

`codex-app-server` is a runner. A live run through `run_worker` finished with `usage_updated`, `manager_answered` and `session_started` in the record:

```json
{"model":null,"provider":null,"runner":"codex-app-server","session_id":"01a03e29-1a68-76c3-b6c5-fdeb24d5d478"}
```

Every mapping is backed by a line from the probe. What landed, and what deliberately did not:

| Notification | Mapped to | |
|---|---|---|
| `thread/tokenUsage/updated` | `usage_updated` with a real `modelContextWindow` | the denominator, natively |
| `thread/compacted` | `context_compacted` | second runner able to say so |
| `item/completed` (agentMessage) | `Output` | the **assembled** answer |
| `item/agentMessage/delta` | activity only | `05 run state model`'s rule |
| `turn/completed` | `Finished`, `interrupted` -> `Cancelled` | only a human choosing not to proceed |
| `account/rateLimits/updated` | **nothing yet** | read into [`RateLimits`], not signalled |

The last row is the deliberate one. `27 quota accounting`'s `WindowObservation` holds **one** window and this reports **two**, each with a percentage that shape has no field for. Forcing it through now would report a number farseer cannot stand behind, so the parser reads it into its own type with tests, and a test asserts `parse_line` stays silent about it.

### The deltas taught nothing new, which is the point

Codex streams `item/agentMessage/delta` **and** sends `item/completed` carrying the assembled `text`. ACP has no assembled form, which is why `RunnerSignal::OutputChunk` had to exist. Here the runner does the assembling, so the deltas are activity and the item is the answer - the same rule reaching two different conclusions because the runners differ, rather than one rule bent to fit.

### One shared request loop, extracted on the second use

`jsonrpc.rs` holds the write-a-request-and-read-past-everything-else loop that ACP and the app-server both need, extracted when the **second** protocol appeared rather than in anticipation of it - `08 generalization test`'s standard, applied to farseer's own code.

The handshake differs in a way worth naming: the app-server does nothing until an `initialized` **notification** arrives, so a client that skips it waits forever on a `thread/start` the server has not begun listening for. That is the third time this crate has met a live process producing nothing because of a handshake detail, and it is spelled out in the code rather than left in a trace.

### An `#[ignore]`d test does not run itself

Running every live test at once found the ACP one **failing** - it asserted `RunnerSignal::Output`, which was true when it was written and became false hours later when the fragment bug was fixed and ACP started emitting `OutputChunk`.

Nothing was broken except the test, but nothing said so either: `#[ignore]` keeps it out of `cargo test`, and it had not been run since the change. The whole point of these tests is that they are the only ones touching a real runner, and they are exactly the ones that rot silently.

Worth a habit rather than a fix: **run the ignored sweep whenever a signal's shape changes**, not only when the runner does.

### Not built, and each one is a decision rather than work

- **Quota**, above: `27` needs two windows and a percentage field.
- **`effort` and `model` on `turn/start`.** Both are wired to `None`. `30` section 4 says why: reasoning effort is a property of how a run executes, `05 run state model` sealed the contract, and it is a contract field or a runner default. Guessing is how a field ends up meaning neither.
- **`turn/steer`.** Needs the `expectedTurnId` farseer does not track, and using it at all is the **correction to `20 worker control channel`** recorded on that ticket - the first evidence against turn-boundary granularity. A decision, not wiring.


## Implementation note, 2026-08-26: quota widened, and the refusal kept

`27 quota accounting` now holds what `codex app-server` reports. Live, on a real run:

```
report.windows == 2, both with the provider's own used_percent, both with a blank account
```

Three changes, and the third is the one that mattered most to get right.

### A window is identified by its account **and** its limit

`latest_observation` was keyed by account alone. With Codex reporting a five-hour and a weekly in the same notification, each would have differed from the one before it forever, and **append-on-change would have silently become append-everything** - the exact repetition `27` section 4 built this to avoid. `WindowObservation::window_key` is now `(account, rate_limit_type)`, and `windows()` groups by both.

Claude Code's single `five_hour` window is untouched by this.

### `used_percent` is reported, never computed

The field is `Option`, skipped entirely on the wire when absent, and set only from what a provider states. Nothing anywhere turns spend into a percentage.

**The test that encoded `27`'s refusal still passes unchanged**, because its fixture is a runner that states nothing - which is every runner but `codex-app-server`. A second test beside it asserts the provider's number is reported as stated, with unrelated spend on the same account, so the two claims sit next to each other and neither can quietly become the other.

That was the point of doing this rather than deleting the old test: **`27` refused a percentage farseer would derive, not a percentage that exists.** Deleting the test would have thrown away the reasoning along with the assertion.

### A window filling up is now a transition

`differs_from` compares `used_percent`, so a window going 12% -> 85% is recorded **while the status is still `allowed`**. That is the first advance warning farseer has ever been able to see, and `26 routing policy` was designed believing no runner offered one - which is why its correction note says to re-read it before wiring rather than wire it as written.

### Two things read but deliberately not mapped

- **Exhaustion is read from `rateLimitReachedType`, never from the percentage.** 100% used and refused are different claims, and a test pins that a window at 100% with no `rateLimitReachedType` is still `Allowed`. Farseer deciding a limit had been hit before the provider did would be exactly the inference `12 autonomy and deny list` forbids.
- **Codex's `credits` and `spendControlReached` are not `is_using_overage`.** Claude Code's overage means something specific; mapping a different provider's different concept onto that word would make the field mean two things. Left absent.

### And the account is still declared

The adapter fills every field **except** `account` and `runner`, and the API - the layer that reads runner config - fills those in. `27` is explicit that the account is declared by the operator and never inferred, so the adapter cannot supply it even though it knows `limitId: "codex"`. A test asserts the adapter leaves it blank.


## Decided 2026-08-26: effort is read, never written - and the default is the hint

Section 4 step 3 asked whether reasoning effort is a **contract field** or a **runner default**. The operator's answer: keep control where farseer has it, leave every other runner exactly as it is, and **research the default so it can be shown as a hint**.

That turned out to be answerable rather than a judgement call, because the default is observable.

### It was noted three times and never researched

`28 operator surface` recorded the thinking level as unreportable. `29 harness protocol` corrected that to **unrequested**. `30` confirmed `effort` is accepted per turn. None of the three asked the question that matters: **what is it set to now?**

`config/read` answers it, with provenance:

```
config.model_reasoning_effort = "xhigh"
config.model                  = "gpt-5.6-sol"
origins.model_reasoning_effort.name.file = C:\Users\...\.codex\config.toml
```

And `model/list` names each model's `supportedReasoningEfforts` - `low`, `medium`, `high`, `xhigh` - so farseer never has to hold that list itself.

### Why farseer must not set it

This machine is configured to `xhigh`. The first probe in this ticket passed `effort: "low"` and **silently overrode that**, which is what a farseer sending its own value would do to every run - downgrading work the operator had deliberately configured up, with nothing in the record saying so.

So `turn_start_frame` carries no `effort` and no `model` in production, and a test pins that. The default is **surfaced as a hint instead**, on `session_started`:

```json
{"configured_effort":"xhigh",
 "configured_from":"C:\\Users\\...\\.codex\\config.toml",
 "configured_model":"gpt-5.6-sol",
 "model":null,"provider":null,"runner":"codex-app-server"}
```

Verified live.

### A hint is a different kind of fact, and lives in a different field

`SessionInfo.model` stays **observed** and stays `null` here, because the app-server names no model for the turn. The configured model sits in `Configured` beside it and is named `configured_*` on the wire and **"configured effort"** in the meta strip.

They agree today only because farseer sends no override. Merging them would make that coincidence look like a measurement, and would quietly become a lie the day farseer starts setting one.

`10 runner inventory`'s rule extended, in one line: **observed, never advertised - and a default is neither.**

### What this decides about the contract

**Nothing yet, deliberately.** `05 run state model`'s contract gains no `effort` field, because farseer sets no effort. The question stays open behind a real precondition: it becomes a contract question the day farseer has a reason to override a default it can now see and name.

Other runners are untouched. Claude Code, Codex `exec`, cursor-agent, goose and the ACP agents behave exactly as before - none of them exposes a default farseer can read, so none of them grows a hint, and none of them loses anything.


## Decided 2026-08-26: nothing is hidden, and what farseer cannot do is said out loud

The operator's rule, in their words: **everyone is welcome, though with notice or warning if we can't control it.**

So the settings menu still offers every runner this build can launch, and each one carries what farseer **cannot** do with it, worded as a consequence rather than as a missing field:

| runner | said out loud |
|---|---|
| `claude-code` | reports no context window, so there is no denominator |
| `codex-app-server` | cannot be steered once a run starts |
| `goose-acp`, `opencode-acp` | reports no quota; never says when it compacted |
| `codex`, `cursor-agent`, `goose` | all four |

`control_of` holds the table, next to the dispatch that proves it. Every field is a capability farseer has **driven against the real binary and left a test behind for** - not a claim read off documentation - and an unknown runner is assumed to do nothing, which under-promises rather than offering a verb that stalls.

**Cancellation is deliberately not in the table.** It is farseer's, not the runner's: `03 spike job objects`'s kill works on anything with a process id, and no runner has to agree to it.

### Why a warning rather than a shorter menu

`13 harness build kit` found the inventory is a **menu rather than a survey**. A menu that silently drops entries teaches the operator less than one that says why an entry is dimmer than its neighbour - and the missing capability is usually a property of the *face*, not the tool: `codex` and `codex-app-server` are one binary with four caveats and one.

That is the same lesson `29 harness protocol` and `30 codex app server` each arrived at from their own direction, now visible in the surface rather than only in the tickets.


## MVP, 2026-08-26: a manager, a runner and a quota widget, all on one subscription

Cell zero's manager **and** its `coder` worker now run on `codex-app-server`, and the canvas shows the account's real windows while the manager is still alive:

```
allowed | codex-app-server | secondary | 1% used | resets in 121h 55m
allowed | codex-app-server | primary   | 1% used | resets in 4h 4m
```

```
you          Reply with exactly: farseer online.
top manager  farseer online.

CELL zero  RUNNER codex-app-server  CONFIGURED EFFORT xhigh
SESSION 01a0433d  CONTEXT 27,436 / 258,400 (11%)  TOKENS 34,213  LAST RUN ok
```

### The gap this found, and it was structural

The first attempt showed `{"windows":[]}` with a healthy run in flight. `observe_window` was only called **when a run finished**, out of `report.windows` - and a manager does not finish. It stays open for as long as the operator is talking to it, which is exactly the period they are spending.

So a quota surface built on end-of-run reporting is empty during the only time it matters.

Window observations are now appended **as they arrive**, in the read loop, the same correction `28 operator surface` already made for the context window: *a reading that only survives to the end of a run cannot show a window filling up while it fills.*

Two things that made this safe rather than a duplicate path:

- **The store's on-change guard makes it idempotent.** The end-of-run path still runs and is a no-op when nothing changed, so nothing had to be removed to add this.
- **The account still comes from the operator.** `RunOptions` grew an `account`, filled by the API from runner config, because `27 quota accounting` declares accounts and never infers them - and `farseer-manager` reads no config. A window observed without one is **dropped rather than filed under a guess**, which is the same refusal the adapter makes when it leaves the field blank.

### Still true and worth seeing in the surface

`account` reads `codex-app-server` because nothing is declared in `runners.toml`. That is `27`'s deliberate default - an undeclared runner is its own account - and it is also the moment to declare one, since this machine's goose delegates through the same ChatGPT login: `goose-acp`, `codex` and `codex-app-server` are one subscription being counted as three.

---

## `codex exec` stays, 2026-08-29

This ticket left "whether `codex exec` stays at all" open, and later summaries carried it as though the app-server's arrival put pressure on it. It does not, and the framing was wrong in a way worth correcting: **there is no dependency argument here.** Both faces are the same `codex` binary already resolved on this machine, so keeping `exec` costs no download, no toolchain, and nothing at install time.

The real cost this ticket named is narrower and stands: **two adapters for one binary.** `codex.rs` is 184 lines against `codex_app_server.rs`'s 729 - two parsers, two dispatch arms, and two sets of fixtures to keep honest when Codex changes its output.

### Why it stays anyway

`29 harness protocol`'s rule is **use the richest face a harness offers**, and that argument is about **managers**: steering, a live session, quota, a context denominator. A one-shot worker needs none of it - goal in, answer out, process exits - and that is exactly `exec`'s shape.

The app-server is a **server**, and this ticket's own open item says so: its long-lived process does not fit `19 rust toolchain`'s one-process-per-run assumption, and that is still unresolved. Making it the only Codex face would force that question before anything needs it answered.

So the split already in the code is the right one, and it is the same precedent `29` set for goose: **app-server for managers, `exec` as the cheap one-shot face for workers.** `goose` and `goose-acp` coexist for exactly this reason.

`exec` also still carries policy weight: it is one of four runners `ensure_runner_authority` refuses when a cell grants no shell-capable tool, per `12 autonomy and deny list`.

**Do not re-open this as "should we delete exec".** Re-open it only if `19`'s one-process-per-run question is settled in the app-server's favour, or if the two adapters actually start drifting.
