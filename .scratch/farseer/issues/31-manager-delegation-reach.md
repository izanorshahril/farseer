# 31 manager delegation reach

**Status:** closed 2026-08-29. Four transports, every manager runner, none of them told its own token - see the notes at the foot of this ticket.
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

---

## codex-app-server reaches it too, 2026-08-28

Third transport, and the one this ticket predicted on closing: `thread/start`'s free-form config plus `bearer_token_env_var`.

### The nesting was the whole finding

`mcpServers` at the top of `params` is **accepted and silently ignored**. The thread starts, no server is launched, and nothing says so - the same failure mode `36 tool grant enforcement` found in `pi --tools nosuchtool`, from a different vendor on the same afternoon.

Under `config`, in Codex's own snake_case, it starts:

```
"name":"farseerprobe","status":"starting"
"name":"farseerprobe","status":"failed"
"error":"MCP client for `farseerprobe` failed to start: MCP startup failed:
         Environment variable FARSEER_MANAGER_TOKEN for MCP server 'farseerprobe' is not set"
```

A deliberately unreachable URL and a deliberately unset variable, and the error is the server **proving it read the field**. A probe that only asked "did the thread start" would have called the ignored spelling a success.

The test asserts the nesting and asserts `params.mcpServers` is absent, because a test checking only that the field appears *somewhere* in the frame would pass on the shape that does nothing.

### What running it corrected

The first version reused pi's prompt - no credentials, because pi's extension injects them. The manager answered:

```
Unable to delegate: manager authorization token unavailable.
```

It could see the tools and had nothing to present. **An MCP client calls the tool itself**, so it must pass the two credentials the tool authorizes on; an extension route does not. Codex now shares Claude Code's wording because it shares Claude Code's transport, and the split in this ticket's prompts is by transport rather than by runner.

### Proven

`PERSIMMON`, relayed by a `codex-app-server` manager from a real `pi` worker (`01a048ff-3e8a`, finished ok) delegated through farseer's MCP face.

### The asymmetry this leaves, and it is worth closing

This ticket called the env-var route **strictly better** than the MCP shape where a manager carries its own bearer, and Codex has landed on the worse of the two: the token is in its prompt, where the model can quote it.

Half of it is already better - the *transport* bearer is an env var Codex resolves in its own process, so it is not in the frame or the record. What remains is the tool's own `manager_token` argument.

**The fix is to derive manager identity from the bearer the guard already validated**, making both arguments optional. That would remove the credential from Claude Code's prompt as well, which is the older instance of the same problem. Not done here.

---

## ACP has a channel after all, 2026-08-28

### The assumption that hid it

`29 harness protocol` found ACP's `fs/*` and `terminal/*` are served **by the client**, incompatible with a runner-owned worktree, so farseer declines both.
That made it easy to conclude ACP had nothing to offer a manager, and this ticket closed saying "whether it can serve tools instead is unprobed".

**It does not need to serve tools.** `session/new` takes `mcpServers`, which is the opposite arrangement: farseer names an address and the agent dials it. farseer's own comment on that field said it was empty because the MCP face "is reached by the manager's runner-native configuration" - true for Claude Code, and for an ACP runner there is no such configuration, so the field was empty for a reason that did not apply.

### Probed, not read

goose 1.47.0, live:

```
"agentCapabilities":{"mcpCapabilities":{"http":true,"sse":false}}
"_meta":{"extensionResults":[{"name":"farseerprobe","success":false,
  "error":"failed to initialize MCP client: ... error sending request for url (http://127.0.0.1:9/v1/mcp)"}]}
```

Two facts in one exchange: goose says it speaks HTTP MCP, and goose says **whether it reached what it was given**. The second is the more valuable, and farseer now records it as a `status_changed` event before anything else in the run - a manager whose channel never opened did the work itself, and the record has to say so.

Offered only when `initialize` claims the capability, so an agent that never said it speaks HTTP MCP is not handed an address it may quietly ignore.

### The bearer is inline here, and that split the type

ACP's `HttpHeader` is a literal value, so the token is in the frame. Codex takes the **name** of an environment variable it resolves itself.

One `Option<(String, String)>` carrying either would be exactly the ambiguity `14 vocabulary lock` refuses, so `McpReach` is an enum: `BearerFromEnv { url, var }` and `BearerInline { url, bearer }`. Each protocol matches the spelling it can use and neither invents the other.

### What is built, and what stopped

The channel works. goose connected to farseer's MCP face, found `delegate_to_worker`, and called it. It then failed on the credential:

```
manager_answered {'text': 'Delegation failed: manager_token does not authorize this manager run.'}
```

The run id resolved to a live manager; the token did not match. The token is a 64-character hex string that this transport requires the **model** to copy out of its prompt and into a tool argument - and `gpt-5.6-luna` did not copy it exactly. Codex did, on the same prompt, an hour earlier.

**This is the asymmetry noted above arriving as a live failure rather than a principle.** This ticket argued a credential in a prompt is one the model can quote, spend or leak; it can also simply mistype it, and then the delegation fails in a way that reads like an authorization bug.

**The fix is unchanged and now has evidence: derive manager identity from the bearer the guard already validated, and make both arguments optional.** The guard scans `AppState::managers` for the presented token already, so the identity is resolved a few frames earlier and thrown away. That removes the credential from every MCP prompt - Claude Code's included - and makes this transport work without asking a model to be a courier for a secret.

Not built here. It is one change that touches all three MCP transports and deserves its own pass.

---

## The credential left the prompt, 2026-08-28

`CARDAMOM`, relayed by a `goose-acp` manager from a real `pi` worker - `01a04917-35d3`, ok, 1535 micros, 7641 tokens. Same instruction that failed an hour earlier on a mistyped token.

### The fix this ticket kept deferring

Identity now comes from **the bearer the request already carried**, and `manager_run_id` / `manager_token` are optional arguments kept for a client that prefers to state them.

The guard was already resolving this and throwing it away: it scans `AppState::managers` for the presented token to let a manager-scoped request through at all. `manager_by_token` returns the context instead of a bool, and rmcp hands a tool body the HTTP `Parts` through `ctx.extensions`, which is how the bearer reaches `authorize_manager`.

**No manager on any transport is told its own token any more** - Claude Code and Codex included, which were carrying one for no reason other than that Claude Code's route was built first. A test asserts it across all three MCP reaches, so the next transport inherits the property instead of the habit.

### Three ways a prompt-borne credential fails, and the third was the surprise

This ticket argued a credential in a prompt is one a model can **quote** or **leak**. goose found the third: it can **mistype** it. A 64-character hex string copied from a prompt into a tool argument, one character out, failing as `manager_token does not authorize this manager run` - which reads like an authorization bug and is a transcription error.

Codex copied the same string correctly an hour earlier. **It had been working by luck of the model**, and nothing in the design said so.

### A test whose premise the fix expired

`delegating_to_a_worker_not_in_the_roster_is_refused` asserted that an active manager UUID with a wrong `manager_token` is refused. Its client presents the manager's real bearer, so it is now authorized and the wrong argument is ignored - which is the intended behaviour, not a regression.

It asserts the new rule instead: **the transport proved this manager, and a mistyped argument beside it must not undo that.** A caller with no bearer never reaches a tool body at all; the guard tests already cover that.

That is the second time this map has found a test that would have passed on a premise that had quietly expired, after `31`'s own `a_manager_that_cannot_delegate...` asserted against pi once pi could delegate.

---

## opencode closes the set, 2026-08-29

`SAFFRON`, relayed by an `opencode-acp` manager from a real `pi` worker - `01a0494e-ac12`, ok, 1535 micros, 7638 tokens.

No new code. `opencode-acp` is an ACP runner, so it inherited the channel goose's work built, and this was a probe rather than a build.

**Every runner farseer drives as a manager can now delegate**, over four transports:

| transport | runners | bearer |
| --- | --- | --- |
| generated MCP config file | claude-code | from the request |
| `thread/start` `config.mcp_servers` | codex-app-server | env var name in the frame |
| `session/new` `mcpServers` | goose-acp, opencode-acp | literal header in the frame |
| farseer's own extension | pi, omp | environment, never in a frame |

None of the four tells a manager its own token.

### What opencode does not say

goose reports `_meta.extensionResults` on `session/new` - whether each server it was handed actually started. **opencode reports nothing at all**, while advertising both `mcpCapabilities.http` and `sse`, and while in fact connecting.

So on this runner an empty failure list means "nothing was said", not "nothing failed", and the two are indistinguishable.

That is a limitation of the evidence and not of the channel, and it is why `failed_mcp_servers` returns what the agent said rather than a verdict farseer computed. The rule holds either way: **farseer records what an agent said and never a conclusion it did not.**

### Also worth noting for `29 harness protocol`

opencode advertises `sse: true` where goose advertises `sse: false`, and opencode's `sessionCapabilities` include `fork` and `resume` where goose's include `delete`. Two ACP agents, one protocol version, different surfaces - the same lesson `32 harness capability floor` learned about pi and omp, arriving in a protocol designed to prevent it.
