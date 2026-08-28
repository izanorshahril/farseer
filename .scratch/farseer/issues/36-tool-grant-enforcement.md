# 36 tool grant enforcement

**Status:** open.
**Found:** 2026-08-28, while looking at `32 harness capability floor`'s open item about omp's `hub`. The hub question turned out to be a special case of this one.

## The finding

**Every worker contract carries `tool_grants`, and farseer hands them to no runner.**

`grep tool_grants` reaches the cell definition, the sealed contract, the record and the delegation path.
It never reaches an argv.
`pi::build_args` takes a model, an effort, skills, extensions and a system prompt; `claude_code`, `codex`, `acp` and the rest take their own equivalents.
None of them takes a grant.

So a cell declaring a two-tool roster gets a worker holding whatever its runner ships with - `read`, `bash`, `edit`, `write`, `task`, `hub` and the rest.

## Why this is not a small bug

`12 autonomy and deny list` resolved that **the tool grant is the only isolation v1 has**:

> without a sandbox, grant lists beat deny lists ... The primary control is **what tools a worker is granted**, which `05` already put on the worker contract as `tool grants`.

And `05 run state model` made the contract immutable specifically so that

> "what was this worker allowed to do" has one answer, not a timeline of answers.

It has one answer, and the answer is not true.
That is worse than the deny list being advisory, which at least `12` wrote down: this is a control that reads as enforced everywhere it appears, including in the record an operator would use to reconstruct a run after the fact.

It is also the third instance of the rule this project keeps rediscovering - `31` refused to let a manager imply a delegation it cannot make, and `32` refused a skill the runner cannot load - arriving for a third time and, this time, not yet refused.

## What the probe found

Both runners the operator actually uses take an allowlist, which is the shape `12` asked for:

| runner | flag | shape |
| --- | --- | --- |
| pi 0.84.3 | `--tools, -t <names>` | allowlist over built-in, extension and custom tools |
| omp 18.0.4 | `--tools <names>` | allowlist, "default: all" |

Both also have `--no-tools`, so "granted nothing" is expressible rather than approximated.

## Why it is not a twenty-minute wire-up

**The two vocabularies are not the same vocabulary.**

`cells/zero.toml` grants `shell` and `cargo-test`.
Neither is a pi tool name. pi's are `read`, `bash`, `edit`, `write`, `ask_question` and so on; `cargo-test` is a farseer-level idea of a capability, not a thing a runner has.

So this ticket has to decide the mapping before any code, and the mapping is the whole question:

1. **Do cells name runner tools, or capabilities?** Naming runner tools makes a cell definition non-portable across runners, which `13 harness build kit` and `29 harness protocol` have both spent effort avoiding. Naming capabilities means farseer owns a translation table per runner, and owns being wrong about it.
2. **What happens to a grant no runner can express?** `cargo-test` is a command, not a tool. If the answer is "shell, and the deny list catches the rest", then `12`'s "**if shell is granted, everything is granted**" already tells us what that cell really is - and it should say so out loud rather than imply a narrower grant.
3. **What is the default when a cell grants nothing?** Today it is everything. Silently. The candidates are everything (status quo, honest once stated), nothing (`--no-tools`, which makes an unconfigured cell useless), or a named safe set.
4. **A runner that cannot take an allowlist** - and there will be one, `32` having just found omp missing pi's `--skill` - gets the refusal the other two capability gaps got. Same rule, third application.

## The omp `hub` question, which is a corollary

`32` left this open: an omp manager can now delegate through farseer's extension **and** through omp's own `task`/`hub` background jobs, and the two do not know about each other.
Work spawned the second way is invisible as a run - no child row, no roster check, no worker cap, no per-child budget.
`31`'s spend fix means it is at least counted, and counted is not the same as recorded.

Under this ticket that stops being a separate question.
`task` and `hub` are tools, `--tools` decides whether a manager has them, and "may an omp manager spawn its own subagents" becomes a grant an operator makes on purpose instead of a capability nobody chose.

## Not decided here

The deny list.
`12` settled that it prevents mistakes rather than attacks, and nothing found here changes that.
This ticket is about the control `12` named as primary, and only that.
