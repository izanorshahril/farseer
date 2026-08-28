# 36 tool grant enforcement

**Status:** resolved and built 2026-08-28, proven live.
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

## Resolution

### The two vocabularies were never the same axis, and that was the whole blocker

This ticket asked whether cells should name runner tools or capabilities.
Neither, as it turns out - the question contained a false premise.

A roster's `kind = "tool"` entry names a **cell-level** capability: `post`, `draft-file`, `cargo-test`.
`social.toml` grants `post`; there is no runner on earth with a `post` tool, and there never will be.
These entries were never runner tools and were not failing to be mapped to them.

What was missing is a **second field on a second axis**: how much of the runner's own tool set the run gets.
`ToolLevel` - `read`, `edit`, `shell` - declared on `[manager]` and on each `kind = "worker"` roster entry.

Three levels because `12 autonomy and deny list` already fixed the shape: grant lists beat deny lists, and **if shell is granted, everything is granted**.
`Shell` is named for what it is rather than called `All`, because a level containing `bash` cannot honestly claim to exclude anything.

### Absent means everything, and now says so

The default is `Shell`, which is exactly what farseer has always done.
A stricter default would be safer and would also silently change what every existing cell may do - this ticket's own complaint, pointed the other way.
A cell opts down on purpose.
`zero.toml` and `social.toml` are untouched and behave identically.

### The table is probed, and `Shell` is an absence rather than a list

Tool names came from loading an extension that calls `getAllTools` on a live session, not from a help page:

- **pi** has eight: `bash`, `edit`, `find`, `grep`, `ls`, `powershell`, `read`, `write`.
- **omp** has twenty-three, including `task` and `hub`.

`Shell` passes **no `--tools` flag at all** rather than enumerating everything, so a tool a runner adds in a later version is not silently denied by a list frozen today.

### A finding that changed the design: pi accepts a bogus tool name in silence

`pi --tools nosuchtool -p "hi"` runs, answers, and grants nothing.
No error, no warning.
So the allowlist is built from farseer's own table and **never from a string in a cell file** - a typo would otherwise produce a worker holding nothing, for a reason nobody could see.

### `32`'s open `hub` question, answered as a level

`task` and `hub` are omp's subagent tools. They are present at `shell` and absent below it.
Whether an omp manager may run its own background jobs beside farseer's workers is now a decision somebody makes, not a capability nobody chose.

### The refusal, third application of one rule

A runner that cannot take an allowlist refuses at instruct and at delegate time when a cell asks for anything below `shell`:

```
runner `goose-acp` cannot be held to a tool allowlist, and this cell asks for `read`
```

`31` refuses a delegation a manager cannot make; `32` refuses a skill a runner cannot load; this refuses a grant a runner cannot honour.

### Proven

A throwaway `readonly` cell at `tools = "read"`, told to write a file:

```
manager_answered {'text': 'NO WRITE TOOL'}
```

argv: `--tools read,ls,find,grep`. No `write`, no `bash`, no `powershell`.

### Still open

- **`tool_grants` remains recorded and unenforced**, which is where this ticket started. It is now correctly *scoped*: it is the cell-capability axis, and enforcing `post` means a tool farseer serves, not a flag it passes. That is a different ticket.
- **Only pi and omp are in the table.** Every other runner refuses below `shell` rather than guessing, per `10 runner inventory`.
