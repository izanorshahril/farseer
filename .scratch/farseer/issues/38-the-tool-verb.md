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

---

## Resolution of questions 1 and 2, 2026-08-30: neither fork, because farseer serves no third-party tool

The operator answered the fork by removing what both branches assumed:

> farseer have only minimal tools, harness should use their own tools, farseer tools is only to manage farseer, expose widgets to harness, or to improve farseer itself.

`37`'s forwarding and this ticket's proxy were two ways to put **somebody else's tool** in farseer's hands. The ruling is that farseer never holds one. A harness arrives with its own environment - that is what `37 inherited tool environment` found and what `32 harness capability floor` fenced - and farseer's own tool surface is closed at three purposes:

1. manage farseer,
2. expose farseer's widgets to a harness,
3. improve farseer itself.

Every tool farseer serves today is already in that set: `read_memory`, `write_memory`, `delegate_to_worker`, `delegate_to_cell`. There is no fifth, and now there is a rule saying why not.

### What that does to `kind = "tool"`

Nothing, and that is the answer rather than an omission. A tool entry is a **declaration about the harness's own environment**, enforced where farseer is actually standing: `36 tool grant enforcement`'s `ToolLevel` at launch, which decides what the runner is started with. `ToolHasNoVerb` already says on the canvas that no verb reaches the entry; it stops being a gap the moment farseer stops intending to serve one.

So `12 autonomy and deny list`'s gate is neither refused nor built. **It is scoped**: farseer can gate what farseer serves, that set is permanently small and self-referential, and farseer is already in the path for all of it. A gate over `post` was never reachable without becoming a proxy, and farseer is not becoming one.

### The research the operator asked for, and the one thing in it that argues the other way

Current practice separates the two words this ticket used interchangeably. A **proxy** bridges transport and aggregates servers behind one endpoint and judges nothing; a **gateway** adds authentication, per-tool filtered discovery, quotas, approval, and an audit trail of identity, tool, arguments and outcome. `02 record scope` and `12`'s gate describe a gateway, not a proxy - so the larger build this ticket estimated was under-estimated, not over.

MCP core `2026-07-28` removes protocol-level sessions and adds `Mcp-Method` and `Mcp-Name` headers **so an intermediary can route without parsing the body**. That lowers the cost of a proxy. It does not lower the cost of a gate: a gate on `post` decides on the *arguments*, which are in the body, so the header change buys routing and no policy. The one live argument for building an intermediary got cheaper at exactly the part farseer does not need.

The second finding cuts the same way: tool-space interference is measured, not theoretical - an agent's selection degrades as the catalog grows, and current guidance is per-agent tool filters and a small, domain-prefixed surface. **A minimal tool surface is the design, and forwarding would have been the regression.**

### Exposing widgets, which is purpose 2 and now has a standard

`MCP Apps` shipped as the first official MCP extension, spec `2026-01-26`, package `@modelcontextprotocol/ext-apps`. A tool points at a pre-declared `ui://` resource through `_meta.ui.resourceUri`; the host fetches it, renders it in a **sandboxed iframe**, and speaks JSON-RPC over `postMessage`. Supported today in Claude, Goose, VS Code Insiders and ChatGPT.

That is `28 operator surface`'s third gate, described by somebody else: farseer already renders agent-authored widgets in an opaque-origin iframe with a narrow bridge, and the sandbox probe already asserts the boundary from inside. The difference is the vocabulary - farseer's bridge is `farseer.read` / `ask` / `loadState` / `saveState`, and the extension's is JSON-RPC. Adopting it means an operator's widget renders in **their harness**, not only on farseer's canvas, and that a widget author writes to a published spec instead of to farseer.

This is a serving change, in purpose 2, with farseer on the server side of a boundary it already enforces. It is the opposite direction from the proxy this ticket was weighing, and it is the one worth building.

### Still open

Question 3 - `zero.toml`'s `shell` entry and `36`'s `ToolLevel::Shell` are two places stating one fact. Unaffected by this ruling and still two places that can disagree.
