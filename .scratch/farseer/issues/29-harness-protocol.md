# Harness protocol: does farseer keep parsing four dialects, or speak one protocol?

Type: research
Status: closed
Blocked by: none

## Question

Farseer drives harnesses by **hand-parsing each one's private JSONL dialect**: `claude_code.rs`, `codex.rs`, `cursor_agent.rs`, `goose.rs`.
Four adapters, four sets of field names, four sets of assumptions, and `drive.rs` fanning out to them by runner name.

`10 runner inventory` built that inventory by installing things and reading their output.
It never asked the prior-art question: **what do the other hosts of these same harnesses do?**

Zed, JetBrains, Neovim, Emacs, marimo, Cursor and OpenAI's own surfaces all drive the same binaries farseer drives.
If they converged on something, farseer is paying a maintenance cost for nothing.
If they did not, farseer's four adapters are the honest answer and this ticket closes with a no.

Researched 2026-08-26.

## 1. The inventory, widened

`10` scored five harnesses.
This is the same question asked of every harness with a documented machine-readable mode, which is the population farseer could ever supervise.

**A** = ACP, the Agent Client Protocol: JSON-RPC 2.0 over stdio, client is the editor, agent is the harness.
**Native** = a private line-delimited dialect, which is what farseer parses today.
**RPC** = a private JSON-RPC surface that is not ACP.

| Harness | Machine-readable mode | Shape | ACP | Says its model | Says usage | Says it compacted | Steerable mid-run | In farseer |
|---|---|---|---|---|---|---|---|---|
| **Claude Code** | `--print --output-format stream-json` | Native + control protocol | via `claude-agent-acp` (Zed, Apache-2.0) | yes, `system/init` | yes, `result` | **yes**, `system/compact_boundary` | yes, `--input-format stream-json` | **native adapter** |
| **Codex** | `codex exec --json`; `codex app-server` | Native + **RPC** | via `codex-acp` (Zed, **Rust**) | no, names a thread | yes, four-way split | `thread/compact` on app-server | `turn/steer` on app-server | **native adapter**, `exec` only |
| **cursor-agent** | `--print --output-format stream-json` | Native | `cursor-agent-acp`; also native `agent acp` | yes | not observed | no | not observed | **native adapter** |
| **Goose** | `goose run --output-format stream-json` | Native | **native ACP agent** | per-provider | discards ACP usage today, open issue | no | no | **native adapter** |
| **Gemini CLI** | `--acp`, was `--experimental-acp` | **A** | first-party | via ACP | via ACP | no | via ACP | **ruled out by `10`** |
| **opencode** | `opencode serve` | HTTP + ACP sessions | yes | yes | yes | no | yes | **ruled out by `10`** |
| **Amp** | headless CLI | Native | `amp-acp`, third-party | yes | yes | no | streams thinking and tool calls | no |
| **Crush** | `crush run --format json` | Native | requested, not shipped | **no** - model absent from events, open issue | partial | no | no | no |
| **Droid** (Factory) | `droid exec --output-format stream-json`, also `stream-jsonrpc` | Native + **RPC** | via `acpx` | `--model` accepted | yes | no | RPC mode | no |
| **Copilot CLI** | JSONL; `@github/copilot-sdk` over JSON-RPC | **RPC** | via `acpx` | yes | yes | no | yes | no |
| **Qwen Code** | headless; `acp-qwen-code` | **A** via bridge | third-party bridge | ignores local model config over ACP, open issue | via ACP | no | via ACP | no |
| **OpenHands** | `--json --headless` | Native JSONL | no | yes | yes | no | no | no |
| **Continue CLI** | `-p --format json` | Native | no | yes | yes | no | no | no |
| **Aider** | - | - | `aider-acp`, third-party | - | - | no | no | no |
| **pi** | `pi --mode rpc` | **RPC**, JSON-RPC over stdio | `pi-acp` spawns `--mode rpc` and translates | yes | yes | no | yes | no |
| **OpenClaw** | session tools, `sessions_spawn` | **RPC** | ACP agent **and** ACP client | yes | yes | no | yes | no |

Two facts fall straight out of the table.

**Nobody parses four dialects on purpose.**
Every host that drives more than one harness either speaks ACP or wraps each harness in an adapter process that speaks ACP.
Zed maintains two of those adapters itself and open-sourced both.

**The private JSON-RPC surfaces are the richer ones.**
`codex exec --json` is the cut-down face; `codex app-server` is the real one, and it carries `thread/fork`, `thread/rollback`, `thread/compact`, `turn/steer`, `turn/interrupt`, `account/read`, and a per-turn `model` **and reasoning `effort`** override.
Farseer drives the cut-down face and then wonders why the runner will not report a thinking level.

## 2. This corrects `10 runner inventory`

`10` carried a **fails** column, so a later reader would not re-evaluate a tool already ruled out.
Two entries in it are now wrong, and the reason is a mode that did not exist when `10` was written.

- **Gemini CLI** failed `05`'s activity test because `--output-format json` emits one object per invocation, so nothing arrives mid-run.
  `gemini --acp` streams `session/update` notifications throughout the turn. **It passes.**
- **opencode** failed because its output is plain text by design, and `10` explicitly recorded that `opencode serve` "is a different surface and was not evaluated."
  It has since grown ACP session support. **Not ruled out; still unevaluated.**

The correction is not that `10` was careless.
It is that **a harness fails the activity test in one mode and passes in another**, so the fails column must name the mode it tested and re-open when a new mode ships.
Same shape as `05`'s hard disqualifier doing real work: it disqualified an invocation, not a product.

## 3. What ACP standardizes that farseer hand-rolled

Read this as a list of places farseer independently arrived somewhere, which is worth knowing either way.

| ACP | Farseer's own | Verdict |
|---|---|---|
| `usage_update` carrying `{used, size, cost{amount, currency}}` | `28`'s open "context info" item | **ACP is right and my own note on `28` was wrong**, below |
| `session/set_mode`, `availableModes` - Ask, Architect, Code | `12 autonomy and deny list`'s ceiling | same idea; farseer's is per-cell rather than per-session |
| `session/request_permission` - AllowOnce, AllowThread, AllowAlways, Deny | `12`'s deny list, decided **before** the run | different by design - farseer has no human at the prompt, which is the premise |
| `session/cancel` | `05`'s `cancel` | same |
| `session/load`, `loadSession` capability | `07 attach semantics` | farseer's is stronger - replay and live are one call with a cursor |
| `stopReason`: `end_turn`, `max_tokens`, `max_turn_requests`, `refusal`, `cancelled` | `05` plus `23`'s `ok`, `failed`, `cancelled`, `abandoned` | **neither maps onto the other cleanly** - `refusal` and `max_turn_requests` are both `failed` today, and `26` already suspects a fifth |
| `fs/read_text_file`, `fs/write_text_file`, `terminal/*` provided **by the client** | `19`'s runner-owned git worktree | **incompatible, and farseer must decline the capability** |
| `session/set_model`, `availableModels` | `28`'s settings surface | unstable in the spec and **reported broken in several shipped implementations** |

### The correction to `28`

`28` left "a token breakdown" open, in my words, arguing that `codex.rs` summing `input_tokens`, `cached_input_tokens`, `output_tokens` and `reasoning_output_tokens` into one number was discarding the honest answer to "context info".

ACP considered exactly that and **chose the opposite, deliberately**: `usage_update` carries `used` and `size`, and the per-turn breakdown lives in a separate proposal.
The reason is that `used`/`size` is the number a surface can *act* on - a percentage, a threshold, a "start a new session" prompt - and its clients render 75-90% yellow and above 95% red.
A four-way split of one turn answers a different question, and it is an analytics question, which on this map belongs to `11` rather than to the operator surface.

So the breakdown is **not** what `28` should build.
`used`/`size` is, and `size` is the field farseer currently has no source for at all.

### What ACP does not have, and farseer does

- **No compaction boundary.**
  Still an open discussion upstream: no `context_compacted` notification, and no way for a client to learn that the session head is continuing from a summary.
  Claude Code emits `compact_boundary` with `trigger: auto | manual`, `claude_code.rs` maps it, and `10` scored it as its own column for exactly the reason upstream now cites.
  **Farseer is ahead here and would lose ground by going ACP-only.**
- **No subscription window.**
  ACP usage is *context*, not quota. Nothing in it carries `resetsAt`, an account, or an exhaustion status.
  `27 quota accounting` is built on Claude Code's `rate_limit_event`, which **has no ACP representation**.
- **No delegation.**
  No harness has standardized subagents. Codex spawns them by starting another thread against a TOML file, Goose calls `Agent::new()`, OpenClaw has `sessions_spawn` with a `spawnedBy` restricted to `subagent:*` sessions.
  `06`'s `delegate_to_cell` over MCP is as principled as anything shipping.
- **No activity-versus-progress distinction.**
  `05`'s watchdog premise is farseer's own and has no counterpart anywhere in the table.

## 4. Decision

**ACP becomes a fifth runner kind, not a replacement for the four.**

1. `RunnerSignal` stays the internal vocabulary.
   It is already the union of these dialects, and this ticket is evidence for that rather than against it.
2. Add **one** adapter, `acp.rs`, speaking JSON-RPC 2.0 over stdio.
   It admits Gemini CLI, opencode, Amp, Droid, Copilot, Qwen, pi, OpenClaw and Aider through a single code path - nine harnesses for less code than the four native adapters cost.
3. Farseer **declines** the `fs` and `terminal` client capabilities.
   The agent falls back to its own, and `19`'s worktree ownership is untouched.
   This is not a workaround: capabilities are negotiated at `initialize` precisely so a client can refuse.
4. **Keep the native adapters for Claude Code and Codex.**
   Not inertia. ACP would cost farseer `rate_limit_event`, which is `27`'s entire foundation, and Codex's app-server carries `turn/steer` and a per-turn reasoning `effort` that ACP has no field for.
   The rule is **use the richest face a harness offers**, and for those two the richest face is not ACP.
5. Adopt the host/harness configuration boundary verbatim, because farseer derived half of it already:
   **the host owns the surface, permissions and MCP forwarding; the harness owns auth, provider, model and subscription.**
   That is `10`'s observed-never-advertised rule arrived at independently by someone else, which is the strongest evidence on this map that the rule is not a preference.

### Not decided here

- Whether ACP's `session/set_model` is ever used.
  It is unstable upstream and broken in several shipped clients; `28`'s settings surface must not be built on it.
- Whether `stopReason` needs a fifth farseer outcome.
  `26` already suspects one for exhaustion and `refusal` is a second candidate; one ticket should decide both rather than each adding an enum value.
- Whether Codex moves from `exec --json` to `app-server`.
  Strictly richer and strictly more work, and it breaks `spawn.rs`'s one-shot assumption - an app-server is long-lived, so `StdinMode` becomes a third case rather than two.

## 5. Steal immediately, no protocol change required

1. **`used` / `size`, not the four-way split.** Reverses `28`'s open item. Needs a source for `size`.
2. **Reasoning effort.** Codex accepts `none | minimal | low | medium | high | xhigh` per turn. The "thinking level" `28` reported as unreportable is **unrequested, not unavailable**.
3. **Threshold rendering.** 75-90% yellow, above 95% red is a shipped convention; do not invent another.
4. **Name things the way the protocols do** where farseer has no better word. `steer` already matches `turn/steer`; `compact` matches `thread/compact`.
5. **`acpx`**, a command-line ACP client with adapters for Codex, Claude, pi, OpenClaw, Gemini, Cursor, Copilot, Droid and Qwen.
   The cheapest way to test `acp.rs` against nine harnesses without installing nine harnesses.

## Sources

Read 2026-08-26.

- ACP prompt turn, session modes, and the usage RFD: `agentclientprotocol.com/protocol/prompt-turn`, `/protocol/session-modes`, `/rfds/session-usage`
- The compaction gap: `github.com/orgs/agentclientprotocol/discussions/871`
- The host/harness configuration boundary: `zed.dev/docs/ai/external-agents`
- Codex app-server method surface: `gist.github.com/oneryalcin/ee2c27e2d8aa040da8fbe7eebcc2ecea`
- Goose architecture and subagents: `wuu73.org/aiguide/infoblogs/coding_agents/goose.html`; ACP usage discarded: `github.com/block/goose/issues/8132`
- Crush omits the model from headless JSON: `github.com/charmbracelet/crush/issues/2412`
- pi's RPC mode and ACP status: `github.com/earendil-works/pi/discussions/4444`
- `acpx`: `acpx.sh`


## Implementation note, 2026-08-26: the parser, and what a real transcript changed

`goose acp` and `opencode acp` are **both already installed on this machine**, which settles section 1 empirically rather than from documentation.
A probe drove `goose acp` 1.47.0 through `initialize`, `session/new` and `session/prompt` and captured eleven lines. Every mapping in `acp.rs` is backed by one of them.

### The denominator exists

```json
{"sessionUpdate":"usage_update","used":4560,"size":1050000,"cost":{"amount":0.000918,"currency":"USD"}}
```

`size` is the field this map has never had a source for.
`28 operator surface` asked for "context info" and got a token count with no denominator, because neither Claude Code nor Codex sends one.
The first ACP agent farseer ever spoke to sent it **before the first turn**, unprompted, and again after.

It lands in the record as `usage_updated` rather than on `RunReport`, which is a correction to how I first wired it: a reading that only survives to the end of the run cannot show a window filling up *while* it fills, and `28`'s meta strip reads the event stream.

### And the breakdown was not rejected after all

Section 3 said ACP "chose `used`/`size` **instead of** the per-turn breakdown". That is too strong, and the capture shows why:

```json
{"id":3,"result":{"stopReason":"end_turn","usage":{"totalTokens":4560,"inputTokens":4554,"outputTokens":6}}}
```

Both exist, at **different scopes**: `used`/`size` is streamed and cumulative, the split rides the terminal response and is per turn.
So the real rule is not "one instead of the other" but **do not report cost from both**, since one denominator is a session and the other is a turn. `acp.rs` reads cost only from `usage_update`, with a test naming the reason.

### Three more things the capture said

- **The provider is advertised.** `session/new` returned `configOptions` including `{"id":"provider","currentValue":"chatgpt_codex"}` - the field `28` wanted and could only get from `runners.toml`. Not parsed yet.
- **Goose reports cost over ACP**, cumulative and in USD. `10 runner inventory` scored cost per *runner*; it is per **face**.
- **Declining `fs` and `terminal` was accepted without complaint**, and the turn ran. Section 4's requirement is not theoretical.

### The hazard this build takes seriously

ACP expects the client to answer `session/request_permission`, and farseer has nobody watching.
An unanswered request is a live process producing nothing - the identical failure `28` already paid for when a granted tool was missing from `--allowedTools`.
So `parse_line` **surfaces it as `permission_requested`** rather than dropping it, and `set_mode_frame` exists so a driver can open a session in a mode that does not ask.
`goose acp` happens to default to `auto`. That is luck, and the driver must not rely on it.

### Not built

**The driver.** Every native runner here is one-shot - spawn, read lines, exit - and ACP is a conversation: initialize, wait, open a session, wait, prompt.
`drive()` reads stdout and never writes, so ACP needs a stateful counterpart that owns request ids and the session id.
`acp.rs` ships the frames and the parse it will need, proven against a real agent; nothing yet sends them in anger.

Also unbuilt: `configOptions` parsing, and a `size` for the native runners, which have no source for one and should keep showing nothing.


## Implementation note, 2026-08-26: the driver, and the bug it walked straight into

`AcpSession` ([`acp_drive.rs`]) holds the conversation: spawn on a live stdin, `initialize`, `session/new`, `session/set_mode`, `session/prompt`.
It takes **the same sink `drive` takes**, so a line arriving while farseer waits on a response it asked for is still activity - `goose acp` sends a `usage_update` before the first prompt, and it reaches the record through that path rather than being eaten by the handshake.

### `drive()` cannot drive an ACP agent, and the first live run proved it by hanging

`drive` drains stdout **until end of stream**, which is right for every runner farseer had: spawn, read, exit.
An ACP agent **does not exit when a turn ends** - the session stays open for the next prompt, which is the entire point of a session.
So waiting for EOF is waiting forever, and the first live test sat there until it was killed.

This is the same family as the stdin bug on `28 operator surface`: **machinery that is correct for a one-shot runner and silently wrong for a conversational one**, presenting as a live process producing nothing.
Farseer has now hit that shape twice from opposite directions - once writing, once reading - which is worth naming as a class rather than fixing twice.
`drive_turn` returns at the terminal signal and leaves the session usable.

### Proven live, against `goose acp`

`cargo test -p farseer-runner acp_drive -- --ignored`, 2.2 seconds:

- the handshake completed with `fs` and `terminal` **declined**,
- the agent offered `auto`, `approve`, `smart_approve`, `chat`, and farseer asked for `auto` explicitly rather than trusting the default,
- the turn answered as text,
- and a `usage_update` named a window `size` greater than zero, which is the assertion the whole runner exists to satisfy.

The test is `#[ignore]`d for the reason every live test here is: it spends a real subscription.
Claude Code was not involved, per the operator's standing request.

### Still not built

Nothing **calls** `AcpSession` yet. `StartedWorker::spawn` chooses a native adapter by runner name and knows nothing about ACP, so wiring it is the next step and it needs a decision the map does not have: an ACP runner in `runners.toml` names an executable **and a subcommand** (`goose acp`, `opencode acp`), which is a shape `10 runner inventory`'s inventory has not carried before.
