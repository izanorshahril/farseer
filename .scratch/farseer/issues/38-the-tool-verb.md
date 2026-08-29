# 38 the tool verb

**Status:** open 2026-08-29.
**Found:** 2026-08-29, taking up what `36 tool grant enforcement` left open - `tool_grants` recorded and unenforced - and what `37 inherited tool environment` sketched as its answer.

## The finding

**A roster has three kinds and two verbs.**

| roster kind | the verb that reaches it | enforced |
| --- | --- | --- |
| `kind = "worker"` | `delegate_to_worker` | yes - roster check, worker cap, per-child budget |
| `kind = "cell"` | `delegate_to_cell` | yes - roster check, ceiling minimum, budget cap |
| `kind = "tool"` | **none** | - |

Farseer's MCP face serves four tools: `read_memory`, `write_memory`, `delegate_to_worker`, `delegate_to_cell`.
None of them is a cell's tool entry, and there is no fifth.

So `post`, `draft-file`, `cargo-test` and `shell` exist in the definition, in the sealed contract and in the record, and nothing can call them.
`36` framed this as a grant that is not enforced.
That was too kind: **it is not a grant at all, because there is nothing to grant access to.**

## The part that is worse, and is the reason to write a ticket rather than a line in `36`

`12 autonomy and deny list` section 3 built a gate on top of these entries:

| Level | Default |
| --- | --- |
| `reversible` | ungated |
| `undoable` | gated, a cell may lower it |
| `irreversible` | **gated, never lowerable** |

> Tools declare their own level. Policy gates on it. The machinery already exists.

`social.toml` declares `post` at `undoable` under `autonomy_ceiling = "reversible"`, which by that table is an action requiring approval.
`grep` for the gate finds `gated_by_default` and `gate_is_lowerable` with **no caller outside their own unit tests**.

And then the citation drifted.
`35 notification plane` refused a gate bar, correctly, and wrote the refusal down in `notify.rs`:

> **This is not a gate, and must never become one.** `12 autonomy and deny list` refused mid-run approval outright.

`12` refused no such thing.
It refused **a prompt over arbitrary shell commands**, on the deny list's own argument: `deny read .env` is defeated by `cat .env`, so approving a command string is false assurance.
That argument does not reach a declared tool at a declared level - `post` is not defeated by `cat` - and `12` spent a section designing exactly that gate.

**A refusal of one thing was recorded as a refusal of the category, in a source file, where it now reads as settled.**
This is the map's recurring failure mode with the sign flipped: `36` and `37` each found a rule the code followed and nothing stated. Here a rule was stated, the code never followed it, and a later ticket wrote down that it had been declined.

## Why the gate was undeliverable when `12` wrote it, and is not now

The strongest argument against gating `post` has always been `12`'s own: **if shell is granted, everything is granted.** A cell holding `bash` posts with `curl`, and a gate on its `post` tool measures nothing.

Until 2026-08-28 every run farseer launched held a shell, because there was no way to ask for less.
`36 tool grant enforcement` added `ToolLevel`, and a cell at `read` or `edit` genuinely cannot reach the network - proven live, a worker at `read` answered `NO WRITE TOOL`.

So `36` made `12`'s gate deliverable and nobody noticed, three weeks after `35` recorded it as refused.
**A design blocked by a missing capability stays refused after the capability arrives, because the refusal is written down and the capability is not cross-referenced.**

## What this must decide

1. **Does farseer serve the tool, or only name it?** `37` sketched the mechanism: a tool entry names an MCP server declared in runner config, and farseer forwards exactly the granted ones. That makes the roster the grant. But a forwarded server is one farseer does not intercept, so a forwarded tool is **granted and ungateable** - which answers 2 by construction, and not in the direction `12` wanted.
2. **Can farseer gate anything it does not serve itself?** No. Gating requires the call to pass through farseer. This is the whole design fork, and it is a fork between `12`'s gate and `37`'s forwarding, which cannot both be had for one tool.
3. **What is `shell` doing in the roster?** `zero.toml` grants a `shell` tool at `irreversible` with `grants_shell = true`, and `36` put the same fact on a second axis as `ToolLevel::Shell`. Two places say one thing, which `runners.toml` already calls out as two places that can disagree.
4. **What happens to a tool entry nothing serves?** Today: silence. The three precedents all point one way - `31` refuses a delegation a manager cannot make, `32` a skill a runner cannot load, `36` a grant a runner cannot honour. **Refuse where a person can see it**, at validation time, in the report the canvas already renders.

## Not decided here

Whether `notify.rs`'s comment is wrong or merely over-broad. It is over-broad; the correction belongs wherever this lands, and `35`'s actual refusal - a notifier must never be answerable - stands untouched.

---

## Resolution of question 4, 2026-08-29: the entry says what it is

Every `kind = "tool"` entry now carries `Advisory::ToolHasNoVerb`, which reaches the two surfaces advisories already reach - `farseer validate` and `/v1/cells` on reload:

```
ok       social
note     cells\social.toml: tool `post` is declared and recorded, and no verb reaches it: farseer serves no tool call, so it grants nothing and gates nothing
note     cells\zero.toml: tool `shell` is declared and recorded, and no verb reaches it: ...
note     cells\zero.toml: tool `shell` reaches a shell, so the deny list is advisory for this cell
```

**An advisory, not an error.** The three precedents this cites - `31`, `32`, `36` - all refuse at *launch*, where the alternative is a run that silently does less than its contract says. Here the alternative is refusing `zero.toml` and `social.toml`, the two definitions this repo runs on, for declaring something they have always declared. `12 autonomy and deny list` set the pattern for exactly this case with `DenyListIsAdvisory`: a control that reads stronger than it is **says so** rather than failing the author who wrote it.

`shell` draws both advisories, and they are two different facts about one line: nothing calls it, *and* its presence makes the deny list advisory.

### The comment that had recorded a refusal nobody made

`notify.rs` said `12` "refused mid-run approval outright". Corrected in place to what `35` actually refused - a bar approving **shell command strings** - with the reason the argument stops there.

## Questions 1 to 3 stay open, and 1 and 2 are one fork

Stating it precisely, since the ticket's own framing improved while answering 4:

**Farseer can gate only what it serves.** A tool forwarded to the model as an MCP server is dialled by the model directly; farseer sees no call, records no call, and gates no call. So `37 inherited tool environment`'s sketch - *farseer forwards exactly the granted servers* - buys the roster-as-grant at the cost of `12`'s gate and of `02 record scope`, and the two cannot both hold for one tool.

The shape that keeps both is a **proxy**: farseer serves the tool, calls the declared server itself, and every call is a recordable, gateable event. That is one word different from `37`'s sketch and a substantially larger build - farseer becomes an MCP client per run, on top of being a server.

Not decided today, because there is no declared server to proxy yet and inventing one to justify the seam is the direction `13 harness build kit` names as its worst.

### What made the gate deliverable, and why it looked refused

Recorded here because it is the transferable part: `12`'s gate was theatre while every run held a shell, and `36 tool grant enforcement` ended that on 2026-08-28 by making `ToolLevel` expressible. Nobody revisited the refusal, because a refusal is written down and the capability that lifts it is not cross-referenced.

The re-check condition, stated in `33 google quota`'s terms - **name the capability, not the tool**: a gate on a declared tool becomes worth building the day farseer serves a tool call at all, for any cell whose `ToolLevel` is below `shell`.
