# Context compaction and what farseer can know about it

Researched 2026-08-23, prompted by the operator asking whether server-side compaction changes the runner story.

Short answer: **farseer must never compact, must record that compaction happened, and must accept that it cannot record what was lost.**

## What each harness actually does

### OpenAI, server-side, since 2026-02-11

Enabled on the Responses API by setting `context_management` with a `compact_threshold`.
When the rendered token count crosses the threshold, the server compacts.

Codex uses a proprietary endpoint, `POST /v1/responses/compact`, which returns an **opaque, AES-encrypted** compressed representation of the conversation.
OpenAI's servers decrypt it, prepend a handoff message, and feed the restored context to the model.

Only the compression step is remote. Pre-processing and post-processing stay client-side.

For non-OpenAI models the open-source Codex CLI compacts **locally**, with an LLM summarising the conversation against a compaction prompt.

### Claude Code

Compaction summarises older context as the conversation approaches the window limit, extending effective context beyond the nominal 200K.

Critically for farseer, **the Claude Code SDK emits an observable signal**: a `system` event with subtype **`compact_boundary`**, carrying a `trigger` field of `auto` or `manual`.

The ACP bridge also exposes a `session/fork` RPC, where an in-place fork branches the conversation while leaving the parent recoverable, with the branch carrying full prior context.

## Four consequences for farseer

### 1. Farseer must not compact

Farseer does not own the conversation. The runner does.
Compaction requires knowing the prompt, the tool schema and the model's own summarisation behaviour, none of which farseer holds.

An orchestrator that compacts from outside is fighting the harness for control of a buffer it cannot see.
This is the same reasoning `16` used against ACP-as-substrate and `06` used against A2A-shaped internals: **do not reimplement what lives on the other side of a boundary.**

### 2. Compaction is a progress event, and it is already emitted

`compact_boundary` with `trigger: auto | manual` is exactly the shape `02` defined for a progress event: a runtime-observable status change, small, semantically meaningful.

Proposed event kind: **`context_compacted`**, carrying the trigger and, where the harness supplies it, the token count before and after.

This matters because compaction is invisible in the artifact and highly visible in the outcome.
A run that quietly lost half its context and then produced something odd is otherwise indistinguishable from a model having a bad day.

### 3. Compaction breaks token accounting, which hits `11`'s cost metric

`11` chose **cost per successful run** on the grounds that raw cost cannot separate work from thrashing.

Compaction complicates that further: after a compaction the conversation is re-rendered, so cumulative input tokens either reset or double-count depending on the harness.
A cost query that sums naively across a compaction boundary is wrong, and wrong in the direction of overstating spend.

So `context_compacted` is not only a diagnostic event, it is a **segmentation boundary for cost queries**.

### 4. The opacity limit, stated plainly

**OpenAI's server-side compaction returns an AES-encrypted blob that farseer can never read.**

So farseer can record **that** a compaction happened, **when**, and **what the token counts were either side**.
It can never record **what was dropped**.

This is a hard boundary, not an implementation gap, and it should be stated in any claim the record makes about completeness.
It reinforces `02`'s decision that **the log is not session history**: the transcript was already unreliable as ground truth, and compaction means even the harness's own transcript is a lossy artifact of a process farseer does not control.

## The one that interacts with `05`

**A compaction is a quiet window, and `05`'s watchdog keys on activity.**

During a server-side compaction the client is waiting on a remote call that produces no streamed tokens and no tool calls.
From farseer's view that is **silence**, which is precisely what `stalled` at 120 seconds is designed to catch.

A long compaction on a large conversation could therefore trip a false `stalled`, and on a pathological one, `likely-hung` at 600 seconds.

Two ways out, and the choice belongs to whoever implements the runner:

1. **Treat `context_compacted` as an activity event** and emit it at compaction *start*, not only at the boundary. Requires the harness to signal the start, which Claude Code's `compact_boundary` may not do.
2. **Pause the liveness clock during a known compaction**, the same mechanism `05` already uses to pause it while control is not `autonomous`.

Option 2 reuses machinery that exists and does not depend on a harness emitting anything extra, so it is the cheaper of the two.
Either way this is a **known quiet window**, and `05`'s watchdog must learn about the category or it will cry wolf on exactly the longest, most expensive runs.

## Practical note on the operator's framing

The operator's instinct was that going direct to Codex avoids the problem while a third-party harness needs a workaround.

That is right about **capability** and irrelevant to **observability**.
Server-side compaction is a property of the OpenAI account and the Responses API, so any client hitting those models gets it.
What differs between harnesses is only whether they **tell you it happened**.

That reframes the runner selection criterion: not "does this harness compact well" but **"does this harness say when it compacted"**.
Claude Code does, via `compact_boundary`. That is a point in its favour that has nothing to do with compaction quality.

## Sources

- [OpenAI compaction guide](https://developers.openai.com/api/docs/guides/compaction)
- [Investigating how Codex context compaction works](https://kangwooklee.com/blogs/codex_context_compaction.html)
- [Context compaction deep dive: Codex CLI, Claude Code, OpenCode](https://codex.danielvaughan.com/2026/04/14/context-compaction-deep-dive-codex-cli-claude-code-opencode/)
- [Claude compaction docs](https://platform.claude.com/docs/en/build-with-claude/compaction)
- [How to control Claude Code context compaction](https://fazm.ai/t/control-claude-code-context-compaction)
- [claude-agent-acp](https://github.com/agentclientprotocol/claude-agent-acp)
