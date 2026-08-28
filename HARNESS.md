# The harness contract

What farseer has learned about coding harnesses by driving eight of them, written as requirements for one that does not exist yet.

Every claim here is a captured line from a live run, not a help page - `10 runner inventory`'s rule, **observed, never advertised**.
Sources are the decision tickets in `.scratch/farseer/issues/`; the ones that carry the evidence are cited inline.

This is not a plan to build that harness now.
It is the thing to read before building it, and the thing to check a candidate against.

## Why this document exists

`32 harness capability floor` measured four harnesses against what a coding harness is expected to do, and found **four cells reading `has it, unwired`** - the harness does the thing and farseer cannot see it.

That is worse than `no`.
A missing capability is a gap somebody notices; an invisible one is a gap an operator assumes is covered.
Three separate refusals now exist in farseer for exactly that failure - `31` refuses to let a manager imply a delegation it cannot make, `delegate_to_worker` refuses a skill absent from the repository, and instruct refuses a skill the runner cannot load - and the rule they share is the spine of this document:

> **Silence around a missing capability is the bug; the missing capability rarely is.**

An orchestrator's real cost is not writing adapters.
It is that every adapter is a guess about a surface the harness never promised to keep, and the guess fails quietly.

## 1. The launch is not the protocol

`32` established that omp speaks pi's RPC protocol **verbatim** - the probe drove omp with pi's own frames and got pi's own events back - and farseer shared one adapter on that evidence.
Then omp was launched for the first time and died before its first turn:

```
Error: unknown flag: --exclude-tools
```

The evidence was about the wire.
The launch is not on the wire.

| | pi 0.84.3 | omp 18.0.4 |
|---|---|---|
| deny the tool that waits for a person | `--exclude-tools ask_question` | no such tool, no such flag |
| load a named skill | `--skill <path>`, repeatable | **no way at all** - `--skills` filters what it *discovered* |
| allowlist tools | `--tools`, `-t` | `--tools` |
| extension API | 26 methods, plain JSON Schema | superset, with `zod`, `logger`, `arktype` |

**Requirement.** The launch surface is part of the protocol and versioned with it.
One command line, one capability query, and a harness that is asked for something it cannot do says so **at launch**, not by ignoring it.

## 2. A capability query, answered by the binary

Every fact in `32`'s matrix was obtained by running the harness and reading what came back.
That is the only method that works today, and it is a method, not a design.

The costs are concrete and were all paid:

- A **happy path captured and mapped correctly** was still wrong: `compaction_end` fires whether or not anything was compacted, so a failed compaction (`"Compaction failed: Nothing to compact"`) was recorded as a compaction. Only probing the failure case found it.
- A **skill silently dropped**. omp cannot be handed a named skill; with discovery denied there is no argv that loads one. The run works, the answer is worse, nothing says why.
- **Four `has it, unwired` cells** that only a re-probe would ever close.

**Requirement.** `harness --capabilities` returns machine-readable truth: which tools exist, whether skills load by path, whether a turn can be steered, whether cost is reported and in what unit, whether a context denominator exists, whether subagents exist.
An orchestrator reads it once at launch and refuses what it cannot honour, in front of a person.

## 3. Tools are an allowlist, and the names are stable

`12 autonomy and deny list` resolved that without a sandbox, **grant lists beat deny lists** - `deny read .env` is defeated by `cat .env` - and that **if shell is granted, everything is granted**.

Both pi and omp take `--tools` as an allowlist, which is the right shape.
The problem is one level up, and `36 tool grant enforcement` is open on it: a cell grants `shell` and `cargo-test`; a runner knows `read` and `bash`.
`cargo-test` is a command, not a tool, and has no runner-side name at all.

**Requirement.**

1. Tool names are **stable across versions** and enumerable from the capability query.
2. `--tools` is an allowlist; `--no-tools` means none. Both already exist in the field and both are load-bearing.
3. A **capability layer above tool names** - the harness accepts a coarse grant (`read-only`, `edit`, `shell`) and maps it itself, so an orchestrator does not own a translation table it will be wrong about.
4. Granting shell says out loud that everything is granted. The harness states it; the orchestrator does not have to infer it.

## 4. The terminal event must be unambiguous

farseer ended a worker at pi's `turn_end` until it learned a turn is one round-trip and a tool call starts another.
Then omp taught the same lesson one level out: omp runs a subagent as a **named background job**, so the foreground loop ends with `"isTerminal": false` while the subagent is still running, and a *second* loop ends terminally later.

Ending at the first `agent_end` would have reported a run finished while its own subagent was working, and taken the half-answer as the result.
Absent means terminal, because pi has no background jobs to be waiting on - a default that is correct today and is a guess.

**Requirement.** Exactly one event means "this run is over", it is named, it is always present, and it is never emitted by a leg that is not the end.
Every non-terminal leg is explicitly marked as such.

## 5. Spend is reported per leg, and totals are the orchestrator's job

The corollary of §4, and it cost a real defect: omp's non-terminal leg was recorded in farseer's log and **never reached the run report**, so every omp run that used a background job under-reported what it spent.
Fixed by a dedicated signal rather than a re-read of the log, because **money that only exists in a payload string is money the report cannot add up**.

There is a second rule here, from `10`:

> A runner that reports no spend must not be made to look like it reported zero.

**Requirement.** Every leg reports its own spend, in a stated unit, with a stated basis - list price is not money billed, which is why farseer records pi's figure with a `CostBasis` label.
Absent stays absent. Zero means zero.

## 6. Skills, extensions and discovery are explicit

`32` found **every harness discovering skills from the operator's home directory**, so a run reached whatever happened to be installed on the machine.
That makes a run unreproducible for a reason nobody can see, and farseer denies discovery **on every runner that offers a flag for it** and passes only what a cell names.

That qualifier was added on 2026-08-29 and is the honest version: `37 inherited tool environment` found two runners with no such flag at all, so "farseer denies discovery" was true of four runners and stated as though it were true of six.

**Requirement.** Discovery is off by default.
Skills, extensions and rules load **by path**, repeatably, from arguments only.
A harness that cannot load one by path says so in the capability query, so the refusal happens before the run rather than in the answer.

## 6a. Who can actually take a skill, and who can take a tool list

Probed 2026-08-29, by reading each binary's own flags and by launching it. Two capabilities that sound like one:

| runner | load a named skill | deny skill discovery | tool allowlist | deny plugin/MCP discovery |
| --- | --- | --- | --- | --- |
| **pi** 0.84.3 | `--skill <path>`, repeatable | `--no-skills` | `--tools` | `--no-extensions` |
| **omp** 18.0.4 | **no** - `--skills` is a glob *filter* over what it discovered | `--no-skills` | `--tools` | `--no-extensions` |
| **claude-code** | not probed - the operator drives it interactively | - | - | `--strict-mcp-config` |
| **codex-app-server** | no | no | no | **no** - `config.mcp_servers` merges |
| **goose-acp** 1.47.0 | no | no | no | **no** - `--with-builtin` only adds |
| **opencode-acp** 1.18.22 | no | no | no | `--pure` |
| **agy** 1.1.13 | no | `--disable-slash-commands` | no | no |

Read the columns rather than the rows, because they do not agree:

- **Only pi can be handed a skill directory.** Every other runner discovers skills or does without. omp is the instructive case: it *has* skills, streams them as `available_commands_update`, and offers no argv that loads a specific one - so a filter over discovery is not a loader, and with discovery denied there is nothing left to filter.
- **Denial and loading are separate capabilities and separate flags.** agy can deny (`--disable-slash-commands`) and cannot load; opencode can deny plugins (`--pure`) and cannot deny skills. A design that treats "controls its skills" as one bit gets four of these seven wrong.
- **Two runners cannot be isolated at all.** goose and `codex app-server` load the operator's own extensions and MCP servers before the client speaks, and keep them when it adds more. A `goose acp` session handed an empty server list still loaded `developer`, `skills`, `scheduler`, `summon` and `Extension Manager`.

**Requirement.** Loading by path and denying discovery are two capabilities, declared separately in the capability query, and a harness that offers one without the other says which. The orchestrator refuses in front of a person rather than silently dropping a declaration - which is what farseer does today, per `32 harness capability floor` and `36 tool grant enforcement`.

## 7. Nothing waits for a person unless asked

pi ships `ask_question`, and an unattended run holding on it is a hang farseer must detect and kill rather than a question anybody will answer.
farseer denies it by name - which is a per-harness workaround for a general problem.

**Requirement.** An `--unattended` mode where every path that would block on a human either fails or takes a declared default, and the harness guarantees it rather than the caller denying tools one at a time.

## 8. Silence is the only honest hang signal

`18 hang detection prior art` keyed a watchdog on progress events.
`05 run state model` overruled it: a high-end model reasoning for twenty minutes emits no progress events and would have been flagged `likely-hung` while working perfectly.
The watchdog now keys on **any bytes from the adapter**, and the two thresholds - 120s `stalled`, 600s `likely-hung` - survived the change of signal unchanged.

That correction is what made `35 notification plane` possible: only a mechanical silence signal is trustworthy enough to wake somebody at 3am.

**Requirement.** A heartbeat on the stream during long model turns, so silence means silence.
No auto-kill by the harness; cancel stays the caller's verb.

## 9. Delegation is farseer's word, and the harness must not shadow it

An omp manager can delegate two ways that do not know about each other: farseer's own extension, which produces a recorded child run with a roster check, a worker cap and a budget draw - and omp's `task`/`hub`, which produces work that is invisible as a run.
`36` folds this into the grant question: `task` and `hub` are tools, so "may a manager spawn its own subagents" becomes a decision somebody makes, rather than a capability nobody chose.

There is a second, smaller version of the same problem.
omp addresses extension tools through an `xd://` device mount, so a delegation is recorded with `tool_name: "write"` and the verb buried in `args.path`.
The child run row is correct; **an operator scanning the event stream sees a file write where a delegation happened.**

**Requirement.** A tool call is reported under the name the caller registered it with.
If the harness has its own concurrency, its spawned units are addressable and reportable as first-class runs, not as tool calls.

## 10. What no harness provides, and farseer had to build

Listed because a harness built for this should consider them, not because they belong inside one.

| farseer has | no surveyed harness has |
|---|---|
| Win32 Job Object per process tree, `KILL_ON_JOB_CLOSE` | **zero** surveyed Windows agent tool uses Job Objects at all (`18`) |
| supervised worktree teardown, reap-then-delete | the common failure: `04` measured 10 of 10 stuck without reaping |
| one append-only record with cell-scoped visibility | per-session transcripts |
| liveness derived, never stored | watchdogs that watch their own host process, not the agent (`18`) |
| an immutable contract per run | a config that drifts |

`18`'s sharpest finding is worth restating: **Orca and Traycer both have real hang-watchdog code that only watches their own Electron host process, not the agent worker.** Watching the worker is a differentiator, not table stakes.

## 11. Steal ACP where it is right

`29 harness protocol` found ACP already correct on two things farseer had hand-rolled or missed:

- **`usage_update` carries `{used, size}`.** Neither Claude Code nor Codex reports a context window **size**, so "how full is the context" was unanswerable until farseer spoke a protocol whose authors had already decided it was the number that matters.
- **`session/cancel`, `session/load`** map cleanly onto farseer's own `cancel` and attach semantics.

And where it is not:

- **`fs/*` and `terminal/*` are served by the client.** Incompatible with a runner-owned git worktree, and farseer declines the capability.
- **`stopReason` does not map onto farseer's outcomes.** `refusal` and `max_turn_requests` are both `failed` today, which loses information.

**Requirement.** Speak ACP's vocabulary where it fits, extend rather than redefine, and never require the client to serve the filesystem.

## The short version

A harness farseer would not need to guess about:

1. **Says what it can do**, machine-readably, before the run.
2. **One command line**, versioned with the protocol.
3. **Allowlisted tools** with stable names and a coarse capability layer above them.
4. **One unambiguous terminal event**, and every other leg marked non-terminal.
5. **Per-leg spend** with a unit and a basis; absent stays absent.
6. **No discovery** - skills and extensions by path only.
7. **Unattended mode** that never waits for a person.
8. **A heartbeat**, so silence means silence.
9. **Its own concurrency as first-class runs**, reported under real tool names.
10. **Job Objects and supervised teardown on Windows**, because nothing else does it.
