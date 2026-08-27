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
