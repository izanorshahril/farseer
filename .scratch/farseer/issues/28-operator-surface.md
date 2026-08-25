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

## Corrected 2026-08-25: a widget displays a cell, it never addresses one

Section 2 got the right answer for the wrong reason, and the wrong half is load-bearing.

It said a widget's manager **is** the manager of the cell it fronts, which quietly puts an address on every widget.
The operator corrected it the same day: **every AI input, from any widget, goes to the top manager, and only the top manager.**
That manager is the one thing currently controlling farseer, and it decides from there where the work goes.

The prior art is [Claude Design](https://claude.ai/code): a comment left on a UI component returns to the **main agent**, not to a per-component agent.
The component is the **anchor**, not the recipient.

### What changes

- **One address.** A widget's AI input is `POST /v1/cells/zero/instruct`, whatever the widget is showing. Still no new API operation.
- **The widget supplies an anchor, not a destination.** Which widget, and what it was displaying - a cell, a run, a board column - prepended to the goal so the manager knows what the operator was looking at when they typed. It rides in `goal` as text, because the reader is an LLM and prose is what an LLM reads. `16 local api surface`'s additive-only promise leaves room for a structured `context` field the day something needs to machine-read the anchor; nothing does yet.
- **Section 5's `instruct` row now reads "addressed to the top manager"**, not "addressed to a cell".

### Why this is the better split

**Routing is a decision, not a click.**
A per-cell composer lets the operator route work by choosing which box to type in, which makes the surface a router and hides the routing decision from the record.
Routing belongs to `26 routing policy` and to the manager that owns the task.

**One address means one place** autonomy, policy and budget are enforced, and one place the decision is recorded.
Two composers would have meant two, differing quietly.

**The line it draws is clean**: operator **verbs** act on a run directly - `steer`, `cancel`, `observe`, `take over`, `release`, unchanged from section 5 - while **anything phrased as a request** goes to the top manager.
Clicking cancel is not a conversation. Asking for something is.

### The cost, and it is not small

**Cross-cell delegation stops being optional.**

Under section 2 as written, a widget fronting `social` could have reached social's manager directly and needed nothing new.
Under the correction that request lands on the top manager and has to travel outward, so **every widget fronting a cell other than zero is blocked until cross-cell delegation lands**.
`AGENTS.md` records it as "remains open"; this correction makes it a **blocking dependency of the operator surface**, and it should be built before the second widget rather than after.

Sections 1, 3 and 4 are unaffected.

## Implementation note, 2026-08-25: the three gates, and what an attack actually reached

Built, and each gate was tested by trying to get past it rather than by reading the code and agreeing with it.

### Gate 2, the import allowlist

`ui/plugins/widget-host.ts` compiles a widget with esbuild and refuses to resolve anything outside `react`, `react/jsx-runtime`, `react/jsx-dev-runtime`, `react-dom/client` and the widget's own local files.

A widget importing `../../ui/src/bridge` is told it *reaches outside the widget's own directory*.
A widget importing `node:fs` and `node:http` is told they are *not on the import allowlist*, and is given the list.
Neither mounts, and the operator gets the compiler's own words rather than "failed".

Two things the build taught, both now comments in the file.
The gate governs what the **widget** reaches, not what an allowed dependency does inside itself - policing React's own relative imports meant the allowlist could not allow anything real.
And an allowed bare import resolves from the **host's** dependencies, because `widgets/` holds source and nothing else; the cost is that a widget shares the host's React version.

### Gate 3, the sandboxed render

An iframe with `sandbox="allow-scripts"` and deliberately no `allow-same-origin`, so the widget has an **opaque origin**.
Its only channel is a `MessagePort` handed to it once.

A widget was written to compile cleanly and then attack at runtime, and it reported what it managed to reach through the one channel it had:

| It tried | It got |
| --- | --- |
| the host page (`window.parent.location.href`) | `blocked: SecurityError` |
| `localStorage` | `blocked: SecurityError` |
| `document.cookie` | `blocked: SecurityError` |
| `fetch("/v1/quota")` | `blocked: TypeError` |
| `fetch("http://127.0.0.1:9077/v1/quota")` | `blocked: TypeError` |
| the host bridge | **reached** - which is the one thing it is for |

The isolation is **bidirectional**: the host's own `frame.contentDocument` is `null` for the same reason.

Two properties are narrower than the frame alone:

- **The anchor is stamped by the host.** A widget may ask the top manager for something; it cannot claim to be a different widget while doing it.
- **State is namespaced per widget**, so one widget cannot overwrite another's slice or the canvas layout itself.

### Gate 1, keep or undo

`git status --porcelain -- widgets`, `git add -- widgets`, and `git restore` plus a scoped `git clean`.

Verified both directions: keep staged four files; undo reverted a modified **kept** widget to its committed state and removed an untracked one, while leaving everything outside `widgets/` untouched.
The bar names the files that changed rather than announcing that something did, because a bar saying "changes were made" is one the operator learns to click through.

### Two bugs the gates caused, both real

**A height-report feedback loop** took the dev server's heap: the frame reported its height, the host resized the frame, the resize changed the document height, which reported again. It now reports only a height that actually changed.

**Base64 of a megabyte** - the bundle carries React, and encoding it by spreading a million-element array into `String.fromCharCode` is a memory hazard for nothing. The source now rides in an inert element and the frame imports a blob it makes itself, which an opaque origin is allowed to do even though it cannot fetch a URL.

### What is still not built

Cell zero has not written a widget. `widgets/run-tally` was written by hand, deliberately: the contract gets proven before a manager is asked to satisfy it.
The manager-facing half - a prompt that teaches the widget contract, and the file-writing turn - is the next piece.
