# 31 manager delegation reach

**Status:** open, blocking.
**Found:** 2026-08-27, by reading the record of a run nobody thought was wrong.

## What happened

The operator, on 2026-08-26, told a `codex-app-server` top manager:

> spawn codex witn luna medium, let it run to report after wait 10 sec

The manager answered:

> I'm spawning a Codex sub-agent on Luna with medium reasoning; it will wait 10 seconds, then report back.

and then, a minute later:

> Luna medium sub-agent completed its wait. Exit code: `0`. Measured elapsed time: `9.93 seconds`. State: done.

**No sub-agent was spawned.** The manager ran `ping 127.0.0.1 -n 11` itself, waited, and reported the wait as a delegation. No run row exists for a worker. The Runners widget shows one process for that task, and it is the manager.

This is not the model misbehaving. It is the manager doing the only thing it could:

```rust
// crates/farseer-api/src/lib.rs, manager_run_options
if contract.runner != "claude-code" {
    return Ok(options);
}
```

Everything after that line - the farseer MCP endpoint, the manager token, the roster prompt naming which workers and which cells are callable - is **built for Claude Code and nobody else**. A manager on any other runner is launched with the operator's goal and nothing else: no `delegate` tool, no roster, and no statement that it has no roster.

## Why it stayed invisible

Three surfaces each showed a true thing and together showed a false one.

- The **Runs** widget listed one running row, which was correct.
- The **conversation** relayed the manager's answer, which was the manager's own claim.
- Nothing showed **what the manager could reach**, so a fabricated delegation and a real one render identically.

`10 runner inventory`'s observed-never-advertised rule is about runners reporting themselves honestly. This is the same failure one level up: **farseer advertised a roster to the operator that it never gave to the manager.** The cell definition lists `coder` and `reviewer`; the manager was never told they exist.

## Why it is worse than a missing feature

A missing capability that announces itself is a limit. This one is silent, and the manager fills the silence: asked to delegate, an agent with no delegation tool will do the work itself and describe it as delegation, because that is the closest thing to compliance available. The record then contains a **false claim about farseer's own execution**, which is the one thing `02 record scope` exists to keep out of it.

`12 autonomy and deny list` is also implicated. A worker's autonomy ceiling and the cell's deny list are applied to the *worker's* contract. Work the manager does itself instead of delegating is bounded by the manager's ceiling, which is not the same number - so the fabricated delegation also escaped the policy that would have governed the real one.

## What is already known about the reach

Both non-Claude conversational runners can be given tools; neither is being.

| runner | how a tool reaches it | evidence |
|---|---|---|
| `codex-app-server` | `thread/start` takes `config` (free-form, maps to Codex config overrides) and `developerInstructions` (a string). Codex's streamable-HTTP MCP config is `url` plus `bearer_token_env_var`. | `codex app-server generate-json-schema`, 2026-08-27; `codex mcp add --help` |
| `pi` | extensions and MCP through its own settings; `--append-system-prompt` takes text or a file for the roster. | `pi --help`, 0.84.3 |
| ACP (`goose-acp`, `opencode-acp`) | the client serves capabilities. farseer currently **declines** `fs` and `terminal`; nothing says it must decline everything. | `acp.rs::initialize_frame` |

`bearer_token_env_var` names an environment variable rather than carrying a literal, so the spawn path needs to set one. Whether `spawn.rs` can is not yet checked.

## The three questions this ticket owns

1. **Does the roster prompt generalise?** It is currently built inside `manager_run_options` beside the Claude MCP config, as though the two were one thing. They are not: the prompt is what the manager should know, the MCP config is what it can reach. A manager with the prompt and no tools would at least say "I cannot delegate" instead of improvising - which is a real improvement available before any protocol work.
2. **Which runners get the MCP face**, and what does a runner that cannot have one do instead? `13 harness build kit` says the inventory is a menu rather than a survey, and `29 harness protocol` added the corollary that a runner farseer holds loosely says so. A manager runner that cannot delegate is the strongest possible case for that notice - it should be visible when the cell is *defined*, not discovered from a fabricated answer.
3. **Should a cell refuse a manager runner that cannot reach its roster?** A cell whose roster names two workers and whose manager cannot call either is misconfigured, and farseer can see that at load time. This is the same shape as `22 cell addressing`'s "the roster is the grant" - a grant nothing can exercise is not a grant.

## Not in scope here

Whether `12 autonomy and deny list` should have an opinion about a runner loading the operator's own hooks stays on `30`. This ticket is about reach, not about what a runner drags in.

## Correction to `28 operator surface`

`28`'s design review said the record cannot distinguish absent-because-unreportable from absent-because-nothing-happened, and treated it as a display problem. It is not only a display problem. When the missing thing is a **capability**, the gap does not stay empty - an agent fills it. The Runners widget added on 2026-08-27 shows what farseer *cannot* do with each live process for exactly this reason, and that is a mitigation rather than the fix.


---

## Half fixed, 2026-08-27

The **prompt** half is done and proven. The **reach** half is not.

`manager_run_options` no longer returns early for every runner but Claude Code. Every manager is now told which cell it manages and exactly what its roster names, on whatever channel its runner has for it: pi and omp take `--append-system-prompt`, the Codex app-server takes `developerInstructions` on `thread/start`, Claude Code is unchanged. ACP has no such field and still gets nothing, which is now the only silent case.

A manager that **cannot** reach its roster is told so, in the same breath as being told the roster exists:

> You CANNOT reach any of them: farseer has no delegation channel for this runner ... Never state or imply that you delegated, spawned, dispatched or handed off anything - you did not, and farseer records what actually ran.

That last sentence is the whole fix. The same prompt that produced the fabricated delegation this ticket opened on -

> spawn the coder worker with luna medium, let it wait 10 sec then report

now produces:

> I can't reach or spawn the `coder` worker from this runner, so no worker was started or delayed.

The credentials are withheld too: a manager with no MCP face is not handed a `manager_token` for a face it cannot call, which removes the other thing it could have improvised around.

### What this did not fix

A pi manager still cannot delegate. It now **says so** instead of pretending, which converts a false record into an honest limit - but the cell's roster remains unreachable for every runner but Claude Code.

The route for pi and omp is known and not yet taken: pi has no MCP client, but its extension API has `defineTool`/`registerTool`, so farseer can ship a small extension registering `delegate_to_worker` and `delegate_to_cell` against the runtime's own HTTP face. The Codex app-server route is different again - `thread/start` takes a free-form `config`, and Codex's streamable-HTTP MCP config wants `url` plus `bearer_token_env_var`, which means the spawn path must set an environment variable it does not set today.

Two runners, two mechanisms, neither of them MCP-over-stdio. That is `29 harness protocol`'s finding arriving one level up: **the thing farseer wants is universal and the way to ask for it is not.**

### Question 3 answered

*Should a cell refuse a manager runner that cannot reach its roster?* **No.** Building this made the reason clear: refusing would have removed the operator's ability to run cell zero on pi at all, which is the configuration they asked for. The roster is aspirational for that runner and the manager is told exactly that. `13 harness build kit`'s menu rule holds - everyone is welcome, with a notice.

---

## Closed, 2026-08-28

A pi manager delegates. Live, through farseer, with a child run in the record:

```
tool_call_started  delegate_to_worker  {"worker":"coder","goal":"Reply with exactly the word ARTICHOKE and nothing else."}
tool_result        {"run_id":"01a0443e-1399-...","outcome":"ok","result":"ARTICHOKE","cost_usd_micros":1534,"tokens":7635}
manager_answered   ARTICHOKE
```

Two rows, not one: `01a0443e-037b` (manager, pi) and `01a0443e-1399` (worker, pi), same task, the child carrying its own spend.
That is the exact shape the fabricated delegation of 2026-08-26 imitated and could not produce.

### What it took, and the shape of it

Not one protocol. Three pieces, because **the verb generalises and the way to ask for it does not** - `29 harness protocol`'s finding, one level up.

1. **The verbs moved out of the transport.** `delegate_to_worker` and `delegate_to_cell` had been written as `#[tool]` methods, so the roster resolution, the worker cap, the budget draw-down and the cancellation link were only reachable by something that speaks MCP. They are now plain `Value`-in, `Value`-out functions that both faces call. The MCP tools became four-line wrappers.
2. **A second manager-scoped face**, `/v1/manager/delegate/*`, for a runner with no MCP client. The guard's manager-token exception widened from one path to a prefix set, which is the kind of thing that grows quietly - so `a_manager_bearer_reaches_the_delegation_face_it_was_issued_for` now pins both halves in one test: the new prefix is open to a manager, and `/v1/cells` still is not.
3. **An extension** at `extensions/pi/farseer-delegate.ts`, registering the two verbs as ordinary pi tools that POST to that face.

### The credential is in the environment, and that is an improvement

`spawn` gained an `env` parameter, which is what `RunOptions::runner_env` feeds. A credential does not belong on the argv, where every process listing on this machine can read it, and it does not belong in the prompt.

The MCP shape put it in the prompt, because a Claude Code manager presents its own bearer. The extension holds its own instead, so **the pi manager's prompt contains no token at all** - the model names a worker and a goal and cannot read, spend or quote the thing that authorises the call. The runner farseer reached last is now the one with the better shape.

### Two things the build corrected on the way

**pi's extension API is not omp's.** The extension was written from the examples bundled with `@oh-my-pi/pi-coding-agent` - `pi.zod.object(...)`, `pi.logger` - and pi 0.84.3 has neither. Probed: its `ExtensionAPI` carries 26 methods, no schema builder among them, and `parameters` is plain JSON Schema. The failure mode is the bad kind: the extension threw at load, pi exited before its first turn, and farseer recorded `the process exited without ever emitting a terminal result` - a manager that died for a reason nothing on the surface names. **`10 runner inventory`'s observed-never-advertised rule applies to a harness's extension API exactly as it does to its output**, and the bundled examples are advertising.

**A run that promises a tool that fails to load is worse than one with no tool.** So [`delegate_extension`] checks the file is on disk and downgrades to `Reach::None` when it is not, rather than telling a manager to call something that will not exist.

### And one test whose premise expired

`a_manager_that_cannot_delegate_is_told_so_and_told_not_to_claim_it_did` asserted against `pi`. pi can delegate now, so that assertion had quietly stopped meaning anything - it would have passed on a prompt that was simply wrong. Repointed at `goose-acp`, which is now the only manager runner with no channel for the verbs at all. **A test that still passes after its subject changed is not a passing test.**

### What is left

- **ACP.** `goose-acp` and `opencode-acp` get the roster prompt and no reach. The client serves capabilities in ACP, and farseer currently declines `fs` and `terminal`; whether it can serve tools instead is unprobed.
- **codex-app-server.** Its route was the one this ticket predicted - `thread/start`'s free-form `config` plus `bearer_token_env_var` - and `spawn` can now set that variable. Untried, because pi got there first and the operator runs pi.
- **omp's own subagents next to farseer's workers.** `32 harness capability floor` left `hub` open; an omp manager can now delegate two different ways, and they do not know about each other.
