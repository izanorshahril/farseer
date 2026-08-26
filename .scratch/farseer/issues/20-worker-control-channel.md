# Worker control channel: ACP (Zed) first, or headless JSON per harness?

Type: research
Status: closed
Blocked by: none

## Question

Split out of `05 Run state model and control semantics` on 2026-08-22, which had bundled a research question and a grilling question into one ticket.
This is the research half.
It is **not** to be resolved until the operator asks for a `/research` subagent to be spawned, per the map's standing preferences.

Decide the v1 control channel between farseer and a worker.

- How mature are `claude-agent-acp` and `codex-acp` in practice? Both are third party. What breaks, how often, who maintains them?
- What does each harness expose natively: Claude Code `--output-format stream-json` plus agent view, `codex exec --json`, Antigravity `agy`, Pi RPC mode.
- Is farseer's normalized event schema expressible over both paths without loss?
- What is the true cost of N headless adapters? Vibe Kanban's `executors` crate is the available evidence.
- Disambiguation to carry into every document: ACP here always means Zed's Agent Client Protocol, never IBM's Agent Communication Protocol.

Deliver a recommendation plus the contract-test list a harness must pass before farseer trusts it.

## What this ticket must satisfy

`05 Run state model and control semantics` fixes farseer's own vocabulary above the transport.
Whatever channel is chosen must be able to carry that model without loss, so the contract-test list should be written against it rather than invented fresh.

One question specifically inherited: `18` established a **no-progress-event watchdog** over structured events - tool call start, tool result, status change.
**`05` corrected that.** The watchdog keys on **activity**, not progress, so a channel that only streams assistant text does drive liveness perfectly well.
The two disqualifiers are set out under "Carried from 05" below, and they supersede this paragraph.

Note also that `01` scoped ACP to runners, while `16 What is the local API surface?` is separately weighing ACP as farseer's own client-facing transport, prompted by berd.
These are two different uses of the same protocol and should not be conflated.
Deciding one does not decide the other.

## Carried from 05

`05 Run state model and control semantics` closed and produced the contract-test list this ticket was asked to deliver.
Do not invent a fresh one. Test against these.

Two are **hard disqualifiers**, not quality concerns:

1. **The channel must emit activity at least every N seconds, or expose an explicit "still working" signal.**
   `05` split the watchdog input into **activity** (any bytes - token stream or adapter heartbeat) and **progress** (tool call start, tool result, status change).
   The liveness watchdog keys on activity, because a model reasoning for twenty minutes emits no progress events but is not hung.
   A channel that returns only a final blob makes thinking and hanging indistinguishable, so farseer cannot supervise it at all.
2. **The channel must emit the three progress event kinds** with enough fidelity to drive the record.

Two are quality concerns:

3. **The channel should accept a mid-run instruction without tearing down the session.**
   `05` established that the manager may **steer** mid-run, and that steering appends a `manager_steered` event without changing the immutable contract envelope.
   Every current harness accepts follow-up turns, so this should be widely available.
   A harness that cannot forces the manager to fall back to cancel-and-re-run, losing the session's context. Usable but degraded.
4. **The channel must surface a distinguishable cancellation**, so `cancelled` and `failed` do not collapse into one outcome. `05` made those opposite signals: `failed` invites a retry, `cancelled` records that a human decided not to.

Note that per-harness steering support is exactly the kind of fact this research must establish rather than assume.

## Resolution

Resolved 2026-08-23 by direct research, not by subagent.
The operator asked for the hardest remaining ticket while away, and this one is AFK by nature.

### Recommendation

**Do not choose. `08` already dissolved the question.**

`08` redefined a **runner** as anything satisfying the worker control channel contract.
So the contract is farseer's own, and ACP and headless JSON are two **implementations** of it, not two candidates for it.

For v1, ship **two** runner implementations:

1. **An ACP runner.** One integration reaching every ACP-speaking agent. This is the default path.
2. **One native headless runner** - Claude Code over `--output-format stream-json`, since it is the operator's primary harness.

The second exists for the same reason `01` hand-writes a second cell definition: **two implementations prove the seam is real, one does not.**
It also turns out to be necessary rather than merely tidy, for the reason in section 4.

### 1. Contract test results

The four tests from `05`, run against each candidate.

| | 1. activity signal | 2. three progress kinds | 3. mid-run steering | 4. distinguishable cancel |
| --- | --- | --- | --- | --- |
| **ACP** | **pass** - `agent_message_chunk` and `thought` | **pass** - `tool_call`, `tool_call_update`, `plan` | **fail** - strictly sequential | **pass** - `StopReason::Cancelled` |
| **Claude Code `stream-json`** | **pass** - `content_block_delta` token deltas | **pass** - `stream_event` carries tool calls | fail - turn boundary only | weak - process kill, not a stated outcome |
| **`codex exec --json`** | **pass** - `item.*` incl. reasoning | **pass** - `turn.*` and `item.*` incl. command execution, file changes, MCP tool calls, plan updates | fail - `codex exec resume` is a new process | weak - `turn.failed` does not distinguish cancel |
| **Gemini CLI** | **FAIL** - one JSON object per invocation | fail | fail | fail |
| **opencode** | **FAIL** - plain text by design | fail | fail | fail |

**ACP maps onto `05`'s activity/progress split almost exactly**, which is the strongest single argument for it:

- `agent_message_chunk` and `thought` are **activity**. This is precisely what solves the twenty-minute-reasoning problem `05` identified, because a thinking model emits `thought` chunks continuously.
- `tool_call`, `tool_call_update` and `plan` are **progress**.

That correspondence was not designed for; it is independent convergence on the same distinction, which is reassuring about the distinction.

**Gemini CLI and opencode fail test 1 outright** and are therefore not supervisable by farseer at all in their current headless modes.
`05` called that a hard disqualifier and it disqualifies two real tools, which is evidence the test has teeth rather than being a formality.
opencode has `opencode serve`, which is a different surface and was not evaluated.

### 2. The finding that changes `05`: nobody supports mid-run steering

**Test 3 fails everywhere, and for a structural reason rather than an implementation gap.**

- **ACP is strictly sequential.** The client must wait for a turn to complete before sending another `session/prompt`. There is no provision for concurrent turns; the client receives a `stopReason` before it may prompt again.
- **`codex exec resume`** continues a session by replaying persisted events from disk into a **new process**. That is resumption, not steering.
- **Claude Code `-p`** is one-shot, with continuation keyed on `session_id`.

`05` decided the manager may **steer** mid-run, on the reasoning that "every current harness accepts follow-up turns in a session".
That reasoning was correct but the word "mid-run" was too strong.

**Steering is turn-boundary granular, not interrupt granular.**

The correction to carry into `05`:

- **Steer** = queue an instruction that is delivered at the next turn boundary. Same run, same contract, session context preserved. Universally available.
- Interrupting a turn already in flight is **not available anywhere**, and farseer must not promise it. The nearest thing is cancel-then-prompt, which loses the in-flight turn.

This does not invalidate `05`'s decision that steering keeps the run and the contract.
It bounds the latency of a steer to "however long the current turn takes", which the operator should see rather than discover.

### 3. ACP adapter maturity

Governance improved during 2026: the package moved from `@zed-industries/claude-code-acp` to **`@agentclientprotocol/claude-agent-acp`**, under the protocol's own organisation rather than a single vendor's.
That is a meaningful reduction in the "third party, single maintainer" risk the ticket asked about.

Real defects found, all open or recent:

- **v0.18.0 regression broke the Accept/Reject step for edits**, with edits auto-accepted instead. A permission-gate regression is the worst category for farseer, since `16` maps gated actions onto exactly this flow.
- **Agent registration fails after restart** - "Custom agent server claude-acp is not registered".
- **Extended context window not respected** - reported as ACP agents being limited to 200K instead of 1M on a Max subscription. If accurate this is a **material capability loss**, not a cosmetic bug.

Note the last item is reported against Zed's integration, so whether it is the adapter or the client is not established here.

**Also relevant: HTTP and WebSocket transport for remote agents is still work in progress; every ACP agent currently runs as a local subprocess.**
That happens to suit farseer, which is single-machine per `01`, but it means the transport story is less settled than the protocol's maturity suggests.

### 4. Why the native runner is necessary, not decorative

The context-window report is the concrete case.

If an ACP adapter silently caps context at 200K when the underlying model offers 1M, then routing everything through ACP costs capability that the native path retains.
Farseer cannot fix a bug in someone else's adapter, but it can **have a second path** and let the operator take it.

This is the same instinct `06` and `16` both applied: speak the external protocol at the boundary, but never let it be the only way in.
A runner interface with exactly one implementation is not an interface, it is a wrapper.

### 5. The cost of N headless adapters

Vibe Kanban's `executors` crate is the available evidence.
Its `StandardCodingAgentExecutor` trait defines one interface, and implementations for Claude Code, Codex, Cursor and Gemini spawn child processes and normalise output.

The shape confirms the cost is real but bounded: each adapter is a process spawn plus an output normaliser, and the burden is that every harness has different CLIs, different flags and different output formats, each moving independently.

No quantified maintenance figure was found, so this is a structural argument rather than a measured one.

The practical conclusion for farseer: **N adapters is a per-harness tax, so pay it once deliberately rather than N times by default.**
ACP is the way to avoid paying it N times. The one native adapter is the way to avoid being hostage to it.

### 6. Disambiguation, carried as instructed

**ACP in every farseer document means Zed's Agent Client Protocol.**
Never IBM's Agent Communication Protocol.

Note this now has three distinct uses in farseer and they must not be conflated:

- ACP **client** to runners - this ticket.
- ACP **server** to editors - `16`.
- ACP handshake advertising farseer as an orchestrator - `06`.

### Sources

- [Agent Client Protocol - prompt turn](https://agentclientprotocol.com/protocol/prompt-turn)
- [Agent Client Protocol - schema](https://agentclientprotocol.com/protocol/schema)
- [claude-agent-acp issues](https://github.com/zed-industries/claude-agent-acp/issues)
- [Zed issue 51648 - ACP context window](https://github.com/zed-industries/zed/issues/51648)
- [Zed external agents](https://zed.dev/docs/ai/external-agents)
- [Claude Code stream-json](https://backgroundclaude.com/blog/stream-json)
- [Codex non-interactive mode](https://developers.openai.com/codex/noninteractive)
- [Headless AI coding agents in CI compared](https://www.developersdigest.tech/blog/headless-ai-coding-agents-ci-comparison-2026)
- [vibe-kanban architecture](https://deepwiki.com/BloopAI/vibe-kanban)

### Tickets this informs

- `05 run state model` - **needs correction.** Steering is turn-boundary granular, not interrupt granular. Nothing supports interrupting a turn in flight. The decision stands; the promise must be narrowed.
- `10 runner inventory` - two of the surveyed tools, Gemini CLI and opencode, **fail the activity test outright** in their documented headless modes and cannot be runners as-is. The inventory should record failures, not only candidates.
- `13 harness build kit` - the kit's own output must pass the four contract tests, and the easiest way to guarantee that is to emit ACP.

## Corrected by 10

The contract test table above scores **Claude Code** as "fail - turn boundary only" on test 3.
That was scored against the test's original wording, which this same resolution then corrected: steering is **turn-boundary granular**, not interrupt granular.

`10 Runner inventory` found that **Claude Code supports `--input-format stream-json`**, which works with `--print` and accepts multiple messages into a **running process**, session state intact.

Under the corrected definition - accept a follow-up instruction without tearing down the session - **Claude Code passes test 3.**

That also separates the two native runners, which this table had scored identically:

- **Claude Code** takes a follow-up into a live process. The session never leaves memory.
- **Codex CLI** replays persisted events from disk into a **new process**.

A real advantage for the native runner this ticket had already chosen, discovered after the choice.


## Corrected by 30 (2026-08-26)

**Mid-turn steering exists, on a runner this ticket scored as having no steering path at all.**

This ticket concluded that steering is **turn-boundary granular** rather than interrupt-granular, and corrected `05 run state model` on exactly that point. `10 runner inventory` then scored Codex's follow-up as a **fail**, because `codex exec resume` replays into a new process.

`codex app-server` has `turn/steer`, and its parameters are `threadId`, `input` and **`expectedTurnId`** - a message delivered into a turn that is still running, guarded by optimistic concurrency so a steer aimed at a turn that has already ended is rejected rather than misapplied.

The conclusion held for every runner measured when it was written, and the measurement was of `codex exec`. It is the first evidence that turn-boundary granularity is a property of the **faces farseer happened to drive** rather than of agent harnesses.

Not yet exercised - `30` records it as the fourth and last step of its build order, deliberately after the read half.
