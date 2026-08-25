# What does the operator look at, and what is a widget?

Type: grilling
Status: closed
Blocked by: none

## Question

`map.md` kept **UI shape** as fog: "manager chat, fleet view, board, graph explorer ... what remains is layout and the surfaces themselves".
[24 ui state persistence](24-ui-state-persistence.md) removed persistence from that fog and left the shape.

Two things had to come out of it.

- **What the operator looks at first**, given a channel may be project status, content creation or trading monitoring - the same primitive with a different roster.
- **Which of [05 run state model](05-run-state-model.md)'s verbs are reachable from that surface, and from where.**

A third arrived during the session and turned out to be the load-bearing one: **what is a widget**.

## What the prototype settled

Three variants were built as one throwaway file, [prototypes/ui-command-center](../prototypes/ui-command-center/README.md), and driven by the operator.

- **A - channel rail plus thread.** Channels down the left, one channel's conversation in the middle, runs and verbs in a right inspector, board as a tab.
- **B - canvas of widgets.** No channel list. A tile per channel plus fleet-wide tiles, arrangeable, board as an expanding tile.
- **C - fleet blotter.** Every run in one dense table sorted by what needs a human, board as a mode over the same rows.

**B won.**
The reason matters more than the choice: A and C both assume the operator's day is made of **runs**, and it is not.
A trading-monitoring channel and a content-creation channel do not want the same face, and a surface that renders every channel identically is a surface that is wrong for all of them.

## Resolution

Resolved 2026-08-25 by grilling, after the prototype.

### 1. The canvas is the home screen, and every other surface is a widget on it

There is no second top-level layout to design.
Fleet view, board, manager chat, graph explorer - the four surfaces the fog listed - are **widgets**, arranged by the operator, not modes the application switches between.

This is the rule that keeps the surface finite: **if it is not the canvas, it is a widget on the canvas.**

Arrangement is exactly the opaque blob `24 ui state persistence` already stores, so this decision costs the record nothing and the runtime nothing.

`widget` and `canvas` are **not new nouns**: `24` and `27 quota accounting` already use both, and [14 vocabulary lock](14-vocabulary-lock.md) prefers widening an existing word to coining one.

### 2. A widget is a cell's face, not a second place agents run

A widget that shows work has a **cell** behind it, and that cell's manager is its manager.
The widget renders; it does not think.

This was the fork with real consequences, and the alternative was rejected on evidence rather than taste.

**Rejected: a widget owns its own agent.**
That is [baby-menu](https://github.com/kunchenguid/baby-menu)'s `server.ts` model, and it is a good model for a tray menu with no fleet behind it.
For farseer it creates a **second place agents run** - one invisible to `02 record scope`'s log, to `11 analytics questions`, and to `27 quota accounting`'s window attribution.
Every question the record exists to answer would then have two answers, one of them wrong.

So: **farseer is the top manager, and it instructs other managers.**
That is `POST /v1/cells/{id}/instruct`, which [16 local api surface](16-local-api-surface.md) already shipped, and the composer on the canvas is addressed to a cell rather than to the application.

**The seam is designed in now and left unbuilt.**
A widget declares the cell it fronts.
If a widget later needs an AI layer of its own, it gets one by **being given a cell**, not by growing a runner inside the UI.
Every widget built from here holds that shape whether or not it uses it, which is the whole reason to name it before the first widget exists.

### 3. Cell zero authors widget code, it lives in git, and farseer never stores it

Widget code is **presentation only**, so it belongs on the operator's disk beside the cell definitions, not in farseer's store.

- **Where.** A `widgets/` directory beside `cells/`, versioned in plain git, exactly as [13 harness build kit](13-harness-build-kit.md) put cell definitions in git rather than in a database row.
- **Who.** [01 cell primitive](01-cell-primitive.md) already made cell zero the builder harness. Asking for a widget is a normal instruction to cell zero, and the widget it writes is a normal artefact of a run.
- **Audit.** Falls out for free: git history says what changed, and the record says which run changed it. Farseer holds neither the code nor a second copy of the truth.

**This is not the plugin ABI `01 cell primitive` ruled out.**
That ruling is about the **runtime** loading code, and it stands untouched: the runtime stays headless per `01`, serves `/v1`, and never learns that widgets exist.
The loader lives in the canvas application, which is an API client like any other.
`13 harness build kit` recorded deepseek-harness as the counter-datapoint to that ruling, and the split here is the same one it drew: **out-of-process protocols at the runtime boundary, an ABI only inside the client that renders.**

**Rejected: declarative widgets with no code.**
A widget would be a query plus a render kind, and its ceiling would be whatever the config language expresses.
The operator's own reference, baby-menu, exists precisely because fixed widget sets fail the moment you want a face nobody shipped.

**Rejected: farseer stores and serves widget code.**
It breaks `24 ui state persistence`'s opaque-blob rule, puts rendering concerns inside a headless runtime, and makes farseer the owner of something git already versions better.

### 4. Three gates on agent-authored code, and the fourth is refused

Agent-authored code renders in the operator's own command center, so the gates are named here rather than discovered later.

1. **Keep or undo, per turn.** A turn that changed widget files shows the real diff, labelled, and the operator keeps it or throws it away. This is `12 autonomy and deny list`'s `reversible` level expressed in the UI, and git is what makes it true.
2. **Import allowlist, enforced at compile.** A widget module may import the design system, the framework and its own local files; anything else fails to build. It is the only gate that constrains what code *can do* at author time rather than hoping about runtime.
3. **Sandboxed render with a host bridge.** A widget never holds the operator token and never touches the file system. It renders isolated and reaches farseer only through a host-provided bridge onto `/v1`. `01 cell primitive` already carries Docker Desktop **CVE-2025-9074** as proof that "it is only local" is not a boundary; a widget the operator did not write is exactly the case that proof was recorded for.

**Refused: operator review before a widget may mount.**
It duplicates gate 1 - the operator has already seen the diff and chosen - and it taxes every iteration of a loop whose entire value is that asking is faster than configuring.
A gate that fires twice for the same decision trains the operator to click through both.

### 5. Which verbs are reachable, and from where

| Verb | Reachable from | Why there |
| --- | --- | --- |
| **instruct** | the canvas composer, addressed to a cell | farseer is the top manager; this is the one verb aimed at a manager rather than a run |
| **steer** | inline on a run line, in any widget showing runs | same run, same contract - it needs the run visible and nothing else |
| **cancel** | inline on a run line | `03 spike job objects` measured reap at sub-millisecond, so there is no grace period to design a dialog around |
| **observe** / **take over** / **release** | inline on a run line | `07 attach semantics`'s control axis, and taking over pauses the liveness clock per `05 run state model` |
| **re-run** / **re-scope** | a run's detail only, never inline | both start a **new** run, and re-scope changes a contract field, so the contract has to be on screen to change it |
| **purge** | **not on the canvas at all** | `17 cell lifecycle` made it the only irreversible verb farseer owns, and a drag-arranged surface is the wrong place to keep one |

The rule underneath the table: **a surface never offers a verb the runtime would refuse.**
Liveness is derived per `05 run state model`, and the verb list is derived from lifecycle and control the same way - never stored, never guessed.

### What this does not decide

- **A widget's own AI layer.** Deliberately not built. Section 2 fixes the seam; nothing more.
- **Cross-cell instruct.** `AGENTS.md` records cross-cell delegation as still open. The canvas composer targets one cell at a time until it closes.
- **The framework the canvas is written in.** `01 cell primitive` made every UI a swappable client, and this ticket keeps that: it constrains the widget contract, not the renderer.
- **Which widgets ship mounted on a fresh install.** A product decision, and a cheap one to change.

### Tickets this informs

- `24 ui state persistence` - the blob it stores is now known to be a **canvas arrangement of widgets**, and it still never parses it. No change to the rule, a name for the content.
- `16 local api surface` - no new operation. `instruct`, the four run verbs and the stream already cover the whole surface, which is the strongest evidence the shape is right.
- `01 cell primitive` - the plugin ruling is **confirmed, not amended**: the widget ABI lives in the client, and the runtime still loads nothing.
- `13 harness build kit` - widget code joins cell definitions as **a file in git, not a database row**, and the kit may generate one.
- `27 quota accounting` - its utilisation surface is a widget, and it keeps its own rule: `allowed` / `exhausted_until` / `unknown` and farseer's own spend, never a percentage.
- `26 routing policy` - unchanged, and now unblocked: the display it feeds is a widget.
