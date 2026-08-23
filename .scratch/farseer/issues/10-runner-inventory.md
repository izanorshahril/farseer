# Runner inventory: what accounts, quotas and auth modes actually exist?

Type: task
Status: closed
Blocked by: none

## Question

Manual fact-gathering. Routing cannot be designed against a guess.

Enumerate, for real:

- Which harnesses are installed and on PATH, at which versions.
- How many accounts per harness, and the subscription tier of each.
- Auth mode per account: subscription login, API key, or both.
- Any pay-per-token key available as an overflow tier.
- Observed quota behaviour: what the rolling window looks like in practice, what a rate-limit response actually contains.

Record the numbers. Ticket `09` and the routing fog both depend on them.

## Carried from 08

**The inventory is wider than "agents".**

`08` redefined a runner as anything satisfying the worker control channel contract, so a process adapter around a local binary - `ffmpeg`, for instance - is a runner if it emits activity and progress and accepts cancellation.
So this inventory must cover **non-agent runners**, not only ACP-speaking coding agents.

That changes the shape of the question: it is not "which agents do we support" but "what does it take to make something a runner, and which things already are".

## Carried from 20

**Record failures, not only candidates.**

`20` ran the four contract tests and found two surveyed tools fail the **activity** test outright in their documented headless modes:

- **Gemini CLI** - `--output-format json` emits one JSON object per invocation, so nothing arrives while the run is in progress.
- **opencode** - output is plain text by design. `opencode serve` is a different surface and was not evaluated.

Neither can be supervised by farseer as-is, because farseer cannot distinguish thinking from hanging without activity.
That is `05`'s hard disqualifier doing real work rather than being a formality.

This inventory should therefore carry a **fails** column, so a later reader does not re-evaluate a tool already ruled out and does not assume omission means "not yet looked at".

## Carried from the compaction research (2026-08-23)

Findings: [context-compaction.md](../research/context-compaction.md).

A new selection criterion, and it is not the obvious one.

Server-side compaction is a property of the **account and the API**, not of the harness - any client hitting OpenAI-hosted models through the Responses API gets it.
So "does this harness compact well" is the wrong question.

The right question is **"does this harness say when it compacted"**.

Claude Code does, via a `system` event with subtype `compact_boundary` carrying `trigger: auto | manual`.
That is a point in its favour with nothing to do with compaction quality, and this inventory should score it as its own column.
A runner that compacts silently leaves farseer unable to explain a degraded result, and unable to segment its own cost queries.

## Carried from 14

This ticket was **retitled** from "seat inventory" by `14 Vocabulary and naming lock`.

A **runner** is anything satisfying the worker control channel contract, so the question changed shape:

Not "which agents do we support", but **"what does it take to be a runner, and which things already are"**.

That widens the inventory to non-agent runners - a process adapter around `ffmpeg` is a runner if it emits activity and progress and accepts cancellation - and it means the inventory should record **failures** as well as candidates, per `20`.

## Carried from 12

**A runner that provides shell access effectively grants every tool.**

`12` found that without a sandbox the tool grant is the only isolation v1 has, and that a deny list is advisory wherever a shell is reachable.

So this inventory needs a column it did not have: **what can this runner reach?**
Not only "does it satisfy the worker control channel contract", but "if farseer grants this runner to a worker, what has the worker just been given".

A coding agent with a shell and an `ffmpeg` adapter that can only render a file are both runners, and they are not remotely the same grant.

## Carried from 13

**This inventory is a menu, not a survey.**

`13` assembled the cell definition and found it names a **runner** for the manager and for each roster entry.
So what this ticket produces is the list an author picks from when writing a definition, not a landscape report.

That sharpens what each entry must carry: enough for an author to choose between two runners without going and reading their documentation.
Combined with what `20` and `12` already handed this ticket, an entry needs at minimum: does it pass the four contract tests, what can it reach, and does it report its own compaction.

## Resolution

Resolved 2026-08-23 by probing the machine, plus a checklist handed to the operator for what only they can answer.

This ticket is **task** type and half of it is HITL. The machine half is below and settled. The account and quota half is the checklist in section 5 and remains open until the operator fills it in.

### 1. What is actually installed

Probed on 2026-08-23, `x86_64-pc-windows-msvc`.

| Runner candidate | Version | Path |
| --- | --- | --- |
| **Claude Code** | 2.1.233 | `~/.local/bin/claude.exe` |
| **Codex CLI** | codex-cli 0.148.0 | `~/AppData/Local/Programs/OpenAI/Codex/bin/codex.exe` |
| **opencode** | 1.18.18 | `~/.bun/bin/opencode.exe` |
| **cursor-agent** | 2026.08.11-e8db854 | `~/AppData/Local/cursor-agent/cursor-agent.ps1` |
| **ffmpeg / ffprobe** | 8.1.1-full_build | winget, Gyan.FFmpeg |

Not installed: `gemini`, `aider`, `goose`, `amp`, `droid`, `crush`, `dsh`, `openclaw`, `hermes`, `docker`.

Supporting: node v24.19.0, bun 1.3.14, git 2.54.0.windows.1, uv 0.11.23, ripgrep 15.1.0, pwsh 7.6.5, wsl present.

Config directories exist for claude, codex, opencode and cursor, so all four are configured rather than merely installed. **Contents were not read**, and no credential file was opened.

**No ACP adapter is installed.** `@agentclientprotocol/claude-agent-acp` is not present globally, so the ACP runner path from `20` is currently theoretical on this machine.

### 2. The menu

`13` established this is a menu an author picks from, not a landscape report. Four columns, one per criterion the closed tickets demand.

| Runner | Contract tests (`05`) | Can reach (`12`) | Reports compaction | Verdict |
| --- | --- | --- | --- | --- |
| **Claude Code**, `--print --output-format stream-json` | activity **pass**, progress **pass**, follow-up **pass** (see 3), cancel **weak** | **everything** - full shell | **yes**, `compact_boundary` with `trigger` | **primary native runner** |
| **Codex CLI**, `exec --json` | activity **pass**, progress **pass**, follow-up **fail** (resume is a new process), cancel **weak** | **everything** - full shell, and see 4 | not observed | usable, second choice |
| **ACP adapter** over either | activity **pass**, progress **pass**, follow-up **fail** (strictly sequential), cancel **pass** | whatever the wrapped agent reaches | inherits the wrapped agent | **default path once installed** |
| **cursor-agent** | **not evaluated** | presumably full shell | unknown | open |
| **ffmpeg adapter** | must be written; stderr gives progress, bytes give activity | **one file** it is told to render | n/a | the non-agent proof case |
| **Gemini CLI** | activity **FAIL** - one JSON object per invocation | n/a | n/a | **disqualified** |
| **opencode** | activity **FAIL** - plain text by design | n/a | n/a | **disqualified as CLI**; `opencode serve` unevaluated |

The two disqualified rows are kept deliberately, per `20`: a later reader must be able to tell "ruled out" from "not yet looked at".

Note the shape of the reach column. **A coding agent with a shell and an `ffmpeg` adapter that renders one file are both runners, and they are not remotely the same grant.** That is `12`'s point made concrete, and it is the column an author most needs when writing a roster.

### 3. A correction to `20`'s contract test 3

**Claude Code supports `--input-format stream-json`**, which works with `--print` and accepts multiple messages into a **running process**.

`20` scored Claude Code as "fail - turn boundary only" on test 3. That scoring was against the test's original wording, which `20` itself then corrected: steering is **turn-boundary granular**, not interrupt granular.

Under the corrected definition, test 3 asks whether the channel accepts a follow-up instruction **without tearing down the session**. Claude Code does, in the same process, with session state intact.

**So Claude Code passes test 3, and the table in `20` should be read with this correction.**

This also sharpens the difference between the two native runners, which `20` had scored identically:

- **Claude Code**: follow-up into a live process. Session never leaves memory.
- **Codex CLI**: `exec resume` replays persisted events from disk into a **new process**.

That is a real advantage for Claude Code as the native runner `20` chose, and it was chosen before this was known.

### 4. A runner may claim a sandbox that does not enforce

`12` asserted that there is no OS sandbox on native Windows, so the tool grant is the only isolation v1 has. That assertion was reasoning; this is evidence.

**Codex CLI exposes `--sandbox` with `read-only`, `workspace-write` and `danger-full-access`.**

Tested on this machine:

```
codex exec --sandbox read-only --skip-git-repo-check \
  "Create a file named wrote.txt in the current directory containing the word yes. Use a shell command."
```

The flag was accepted without warning, the model ran a shell command, and **`wrote.txt` was created.**

A single observation rather than an audit, and it does not establish *why* - the sandbox may be unimplemented on Windows, or silently downgraded. But the operational conclusion holds either way:

**A runner that advertises a sandbox is more dangerous than one that does not, if the sandbox does not enforce, because it invites confidence that is not earned.**

So the inventory's reach column must record **observed** reach, never advertised reach. Codex CLI reaches everything on this machine regardless of what `--sandbox` is set to.

This strengthens `12` rather than changing it. The tool grant really is the only isolation, and now that is measured rather than assumed.

### 5. Checklist for the operator

Everything above is machine-detectable. The rest is not, and routing cannot be designed against a guess.

Per configured harness - **Claude Code, Codex CLI, opencode, cursor-agent**:

1. **How many accounts**, and the subscription tier of each.
2. **Auth mode per account**: subscription login, API key, or both.
3. **Any pay-per-token key** available as an overflow tier, and for which provider.
4. **Observed quota behaviour**: roughly what the rolling window looks like in practice, and what a rate-limit response actually contains - a retry-after header, a reset timestamp, or nothing useful.
5. Whether **cursor-agent** is worth evaluating as a runner at all, or is only there for interactive use.

Item 4 is the one that matters most and is hardest to get from documentation. The routing fog on the map cannot graduate into a ticket without it, because "what does exhaustion look like" is the whole question.

**Do not paste keys or tokens.** Tier names and observed behaviour are all that is needed.

### 6. What this settles and what it does not

**Settled**: what exists, what passes the contract tests, what each runner can reach, which two are disqualified, and that a `--sandbox` flag is not evidence of a sandbox.

**Not settled**: accounts, tiers, quotas and exhaustion behaviour. Those wait on the checklist.

The ticket said "record the numbers, `09` and the routing fog both depend on them". `09` closed without them - it turned out to depend on record shape rather than on runner quotas - so **only the routing fog still waits.**

### Tickets this informs

- **The routing fog on the map** - still cannot graduate. It needs item 4 of the checklist above, because runner routing under exhaustion is undesignable without knowing what exhaustion looks like.
- `12 autonomy and deny list` - **confirmed by measurement.** Codex CLI's `--sandbox read-only` did not prevent a write on this machine, so the tool grant really is the only isolation v1 has. Add: advertised sandboxes must not be trusted, and reach must be recorded as observed rather than claimed.
- `20 worker control channel` - **contract test 3 corrected for Claude Code**, which passes via `--input-format stream-json` under `20`'s own corrected definition of steering.

## Operator checklist answered 2026-08-23, items 1, 2, 3 and 5

### Items 1 and 2: the tiers are lower than this ticket assumed

- **Claude: Pro**, one account.
- **OpenAI: ChatGPT Plus**, one account.
- Auth is **subscription login** on both, confirmed for cursor by `apiKeySource: "login"`.

**This changes section 2's verdict column, and it is the most consequential thing on this page.**

Section 2 named Claude Code the **primary native runner** on contract-test grounds, which still holds.
But a **Pro** subscription is the entry tier: Opus access is minimal to absent, and the rolling window is far tighter than Max.

The consequence is not a smaller allowance. It is that **exhaustion stops being an edge case and becomes routine**.
Every design on this map that treats a runner as reliably available needs re-reading with that in mind, and the map's **routing** and **credit and quota** fog entries are promoted from useful to load-bearing.

### Item 3: overflow is a third-party provider

No Anthropic or OpenAI pay-per-token key. An **other-provider key** exists, provider not yet named.

Recorded as a gap: `12` needs to know which provider before an overflow runner can be granted anything, since **reach is recorded as observed, never advertised**.

### Item 5: cursor-agent evaluated, and it is the surprise on this page

The operator believed they had unsubscribed and expected no quota. **Both assumptions were wrong**, established by running it.

`agent status` reports logged in. Two live runs completed against real quota.

#### It reaches models no other installed runner does

`--list-models` returns, among others: **Claude Opus 5 1M Thinking**, **GPT-5.6 Sol** (high and xhigh), **Claude Fable 5 1M Thinking**, Grok, Gemini, and the Codex 5.3 family.

**One runner, one subscription, many frontier models.**
That matters directly to the operator's stated design of a supervisor, a worker and a reviewer on different models: on Claude Pro that shape is not affordable through Claude Code, and through cursor-agent it is.

#### Contract tests from `05`

| Test | Result | Evidence |
| --- | --- | --- |
| **activity** | **pass** | `thinking` deltas, each with `timestamp_ms`. An explicit reasoning stream, which is stronger than inferring activity from output bytes. |
| **progress** | **pass** | `tool_call` with `subtype: started` / `completed`, structured per-tool args and results. |
| **follow-up** | **fail** | **No `--input-format` flag exists.** `--resume` and `--continue` start a new process, which is exactly `20`'s finding about Codex. |
| **cancel** | **not evaluated** | Would need a long run to observe. Left explicitly unevaluated rather than guessed, per `20`'s rule that ruled-out must be distinguishable from not-yet-looked-at. |

**Verdict: a real runner, and second only to Claude Code**, which it loses to on the follow-up test alone.

#### It emits usage per run, natively

The terminal `result` event carries `usage`: `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens`.

**That is `11`'s cost metric handed over for free**, at run granularity, with cache reads separated from fresh input - which is the distinction that makes a cost figure honest rather than merely large.

No other runner in section 2 was observed doing this. Worth checking whether they do, because if they do not, cost must be reconstructed from token counts farseer estimates itself.

#### One concrete integration trap

**cursor-agent refuses to run in a directory it does not trust**, printing a prompt and exiting rather than proceeding.

`04` chose **worktree** as the default isolation strategy, so **every run gets a fresh directory**, so **every run would hit this**.

`--trust` suppresses it. The note is that this is a per-directory gate rather than a per-account one, and a runner adapter that omits the flag will fail 100% of runs while looking like a hang to anything watching for activity.

Same category as section 4's sandbox finding: **what a runner does on a fresh workspace must be observed, not assumed.**

### Still open: item 4

**Observed quota behaviour** remains unanswered, and it is still what the routing and quota fog entries wait on.

## Item 4 answered 2026-08-23, by measurement rather than by waiting for exhaustion

The operator reported that a limit warns with a reset time, and that they rarely hit one - so the behaviour was researched and then **probed on this machine**, because documentation described a path that turns out not to apply.

### The documented path is the wrong one

Anthropic documents `rate_limits.five_hour.used_percentage` and `.resets_at` as fields delivered to a **status line** script.

**Measured: the status line does not fire in `-p` headless mode.**
A settings file with a `statusLine` command was configured, a headless run completed successfully, and the script was never invoked.

So the documented quota surface is **interactive-only and useless to farseer**.
This is section 4's rule applying to a second case: **what a runner exposes must be observed, not read off a page.**

### The real path: Claude Code emits a rate-limit event in-band, on every run

A headless `claude -p --output-format stream-json --verbose` run emitted, unprompted, on a **successful** turn:

```json
{"type":"rate_limit_event",
 "rate_limit_info":{"status":"allowed",
                    "resetsAt":1787473800,
                    "rateLimitType":"five_hour",
                    "overageStatus":"rejected",
                    "overageDisabledReason":"org_level_disabled",
                    "isUsingOverage":false}}
```

**This is the single most useful thing found on this ticket.**

It is not an error and not a warning. It is a **status event on a normal run**, which means:

- **Farseer never has to hit a limit to know where it stands.** Every run reports the window state, so routing can act on the approach rather than on the failure.
- **`resetsAt` is unix epoch seconds**, so "when does this runner come back" is answerable without parsing English.
- **`rateLimitType` names the window** (`five_hour` observed; `seven_day` is documented alongside it), so the two windows are distinguishable rather than conflated.
- **`overageStatus` and `isUsingOverage`** say whether paid overflow is available and whether it is currently in use, which is exactly the overflow-tier question item 3 asked about.

The operator's description was accurate and the mechanism is better than described: the reset time is available **continuously**, not only in the refusal.

### Cost arrives the same way, in dollars

The terminal `result` event carries `total_cost_usd` and a `modelUsage` map with per-model `costUSD`, `inputTokens`, `outputTokens`, `cacheReadInputTokens` and `cacheCreationInputTokens`.

**`11`'s headline metric is therefore native for this runner**, in currency rather than tokens farseer would have to price itself.

### The other two runners have neither

Probed the same way, same machine, same day:

| Runner | Quota telemetry | Cost telemetry |
| --- | --- | --- |
| **Claude Code** | **`rate_limit_event`** every run: window, reset epoch, overage state | **`total_cost_usd`** plus per-model `costUSD` |
| **Codex CLI** | **none observed** - `turn.completed` carries `usage` only | tokens only: `input_tokens`, `cached_input_tokens`, `output_tokens`, `reasoning_output_tokens` |
| **cursor-agent** | **none observed** - `result` carries `usage` only | tokens only: `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens` |

**This is a decisive differentiator and it was not visible from the contract tests.**
Section 2 ranked runners on `05`'s four tests, where Claude Code and cursor-agent are close. On quota and cost observability they are not close at all, and for a **Pro** account where exhaustion is routine, that gap matters more than the follow-up test does.

For Codex and cursor-agent, farseer must **infer** exhaustion from a failed run and **price** tokens itself.

### Two incidental findings worth more than they look

**Codex has a trust gate too.**
`codex exec` in a fresh directory refuses with "Not inside a trusted directory and `--skip-git-repo-check` was not specified" and exits.

That makes **two of three runners** that refuse a fresh workspace - cursor-agent needs `--trust`, Codex needs `--skip-git-repo-check`.
`04` gives every run a **fresh worktree**, so this is not an edge case, it is every run on two of three runners.

**A trivial prompt cost $0.32.**
`total_cost_usd: 0.32266` for a one-word reply, because the run created **32,255 cache tokens** loading the invoking environment's plugins and skills.

A farseer worker inherits the operator's configuration directory unless the adapter prevents it, so **every worker would pay a five-figure token tax per run for tooling it was never granted**.
`12` grants tools deliberately; a runner that silently loads dozens more is that decision being made elsewhere.
This belongs to the runner adapter, and it is measured rather than theoretical.

### What this unblocks

Both fog entries can now graduate:

- **Routing policy** - exhaustion is observable in advance on the primary runner, and inferable only after failure on the other two. That asymmetry is the shape a routing design has to accommodate.
- **Credit and quota accounting** - the quantity, the window and the reset are all available, from one runner, for free.
