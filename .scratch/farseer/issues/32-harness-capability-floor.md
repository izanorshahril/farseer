# 32 harness capability floor

**Status:** open. A measurement, and three things it changed.
**Measured:** 2026-08-27, live, against `pi 0.84.3`, `omp 18.0.4`, `agy 1.1.13`, `codex-cli 0.149.1`.

pi is the benchmark, because the operator picked it as the fleet default and because it is the runner farseer drives most completely. The question is what a coding harness is *expected* to do, and which of those things farseer can **see** - `10 runner inventory`'s rule holds throughout: observed, never advertised. Every row below is a captured line, not a help page.

## What the four can do, and what farseer can see

`yes` means farseer has a mapping backed by a captured payload. `has it` means the harness does it and farseer is blind to it - which is worse than `no`, because an operator will assume it is being recorded.

| capability | pi | omp | agy | codex-app-server |
|---|---|---|---|---|
| steer a live turn | yes | yes | no | has it, unwired |
| tool calls in the record | yes | yes | no | has it, unwired |
| compaction boundary | yes | yes | no | yes |
| skills / commands | has it | has it | has it | has it |
| subagents | none to have | **yes** | none configured | none |
| cost in currency | yes | yes | no | no |
| context denominator | has it | has it | no | yes |
| quota window | none to have | none to have | none to have | yes |

Four separate `has it, unwired` cells. That is the finding, not the ticks.

## The three things this changed on the spot

### 1. A compaction that failed was being recorded as a compaction

Asking pi to compact a short session returns:

```json
{"type":"compaction_end","reason":"manual","aborted":false,"willRetry":false,
 "errorMessage":"Compaction failed: Nothing to compact (session too small)"}
```

`compaction_end` fires whether or not anything was compacted. The adapter mapped it straight to `context_compacted`, so the record would have said a result came out of a summary when it did not. Fixed: only a run that neither aborted nor errored is a compaction, and the `reason` (`manual` / `auto`) is kept, because pi deciding the context is full is a different fact about the run than farseer asking.

This is the sharpest case yet for probing over reading: the happy path had been captured and mapped correctly, and the mapping was still wrong.

### 2. omp really does spawn subagents, and farseer would have cut them off

The operator's open question was whether a runner spawns its own subagents and whether farseer can see it. For omp: yes, and yes.

```
tool_execution_start  tool: task  args: {"context": "...", ...}
tool_execution_start  tool: hub   args: {"op":"wait","ids":["LocateConstant"],"timeoutMs":30000}
agent_end             isTerminal: false
...
agent_end             isTerminal: true   (last message role: custom, customType: "async-result")
```

omp runs a subagent as a **named background job**. The foreground loop calls `task`, then `hub {op: "wait"}`, then **ends with `isTerminal: false` while the subagent is still running**. A second loop starts when the job's result arrives as an `async-result` message, and that one is terminal.

farseer ended a worker at the first `agent_end`. It would have reported a run finished while its own subagent was still working, and taken the foreground loop's half-answer as the result. Fixed: `isTerminal: false` is spending, not an ending. Absent means terminal, because pi sends no such field and has no background jobs to be waiting on.

Same family as the two before it: `29 harness protocol` ended a run at EOF that never came, then at `turn_end` that was not the end. **Three times now, the terminal signal has been finer-grained than farseer assumed.** It is worth stating as a rule rather than fixing a fourth time: *ask the runner whether it is done; never infer it from the shape of the stream.*

### 3. Subagent spend is recorded but does not reach the run report

The terminal `agent_end` carries only its own leg's messages - two, in the probe above, where the first leg had eight. Summing the terminal leg alone under-reports what the run spent, so a non-terminal leg's tokens and cost now land in the record as a `tool_result` event. **The run report still carries only the final leg**, which means `11 analytics questions` will under-count an omp run that used a background job. Recorded as a known gap rather than papered over; the fix is for the report to accumulate across legs, which is a change to `RunReport`'s shape and belongs with the design review's third finding.

## Skills: the largest unwired capability

All four have skills, and farseer surfaces none of them.

`{"type":"get_commands"}` on pi returns **28 commands** on this machine, twenty-odd of them `source: "skill"` - `skill:code-review`, `skill:diagnosing-bugs`, `skill:domain-modeling`, and so on. omp streams the same thing unprompted as `available_commands_update`, and agy expands slash commands in print mode.

Three consequences, in order of how much they cost:

1. **A cell cannot ask for a skill.** A `CellDefinition` names a runner and a prompt; it cannot say "review with `skill:code-review`". The operator's own machine already carries the review playbook and the roster has no way to point at it.
2. **The record does not say a skill was used.** A run that invoked `skill:diagnosing-bugs` and one that did not look identical, which is the same gap `31 manager delegation reach` documents for delegation.
3. **The menu cannot show them.** `13 harness build kit` made the inventory a menu; skills are the part of a harness the operator has actually customised, and they are the part farseer is most blind to.

Not fixed here. It is a ticket-sized decision, because it touches `22 cell addressing` (is a skill a roster entry?) and `12 autonomy and deny list` (a skill is someone else's instructions loading into a run farseer is bounding).

## Where the runners actually differ

Stripped of the shared floor, three real differences:

- **omp is pi plus subagents.** Same protocol, verbatim - the probe drove omp with pi's own frames and got pi's own events back - which is why they share one adapter. They stay two runners because a manager that can delegate internally is a different thing to offer than one that cannot.
- **agy is the honest floor.** One-shot, tokens, no money, no steering, no compaction. It bakes effort into the model id, so `gemini-3.7-flash-low` and `-high` are two entries rather than one model at two settings - which is why `runners.toml` gives it a `model` and deliberately no `effort`.
- **codex-app-server is the only runner that reports quota.** That is `27 quota accounting`'s entire foundation, and it is the reason to keep testing Codex even while it trails on everything else: it is the only evidence farseer has that the quota surface works at all.

## Open

- Skills, per the three consequences above. The largest gap on this page.
- omp's `hub` tool - background jobs are farseer's own concern (`05 run state model` has a whole vocabulary for a run in flight), and a runner having its own parallel one is worth a look before the two collide.
- Whether `agent_end`'s under-counting is fixed in the adapter or in `RunReport`. It is the design review's "RunReport is becoming a bag" finding arriving with a concrete cost attached.

---

## omp had never actually launched, 2026-08-28

`31 manager delegation reach`'s fix went in for both pi and omp, on the strength of this ticket's own finding that they speak one protocol verbatim. Then omp was run through farseer for the first time and died before its first turn:

```
run_finished  "the process exited without ever emitting a terminal result"
```

`omp --exclude-tools` is `unknown flag`. **They share a protocol and not a command line**, and this ticket's "omp is pi plus subagents" said the first half loudly enough to hide the second.

Probed rather than read, since that is this ticket's own rule:

| | pi 0.84.3 | omp 18.0.4 |
|---|---|---|
| deny the tool that waits for a person | `--exclude-tools ask_question` | **no such tool, and no such flag** - its twenty are `read`, `bash`, `task`, `hub` and the rest |
| load a named skill | `--skill <path>`, repeatable | **no way at all**: `--skills` is a glob filter over what it *discovered*, so with discovery denied there is nothing to match |
| load an extension | `-e <path>` | `-e <path>` |
| deny discovery | `--no-skills`, `--no-extensions` | same |
| extension API | 26 methods, no schema builder, `parameters` is JSON Schema | superset: `zod`, `arktype`, `typebox`, `logger`, plus the same methods |

So `build_args` takes the runner name now, and the shared adapter carries two command lines rather than pretending to carry one.

### The skill gap became a refusal

omp cannot be given a declared skill, and the failure mode is the one this ticket exists to prevent: the run works, the answer is worse, and nothing says why. So farseer refuses at the point a person can see it:

```
runner `omp` cannot be given a skill by name, and this cell declares ["farseer-echo", "farseer-record"]
```

Same call `31` made about delegation, and the same call `delegate_to_worker` already made about a skill missing from the repository. **A capability a runner lacks must be named where the run is configured, not absorbed where nobody is looking.**

### omp does delegate, through a mount

With the flags fixed, an omp manager delegated live and returned `RUTABAGA` from a real pi worker. It reached the tool a way pi does not:

```
tool_call_started  write  {"path":"xd://mcp__farseer__delegate_to_worker", ...}   -> No such tool
tool_call_started  read   {"path":"xd://delegate_to_worker"}                      -> the schema
tool_call_started  write  {"path":"xd://delegate_to_worker", "content":"{...}"}   -> ok
```

omp mounts extension tools as **devices addressed by writing JSON to `xd://<tool>`**. It guessed an MCP-shaped name first, read the schema, then called correctly - self-correcting, and farseer's tool descriptions were enough to do it from.

One consequence for `02 record scope`: the event's `tool_name` is `write`, and the verb is inside `args.path`. **An operator scanning omp's events sees a file write where a delegation happened.** The delegation itself is recorded correctly - the child run row exists, under the parent task, with its own spend - so this is a legibility gap in the event stream rather than a false record. Not fixed here.
