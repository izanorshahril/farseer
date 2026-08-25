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

## Implementation note, 2026-08-25: the verbs are on the line

Section 5's table is built, and one thing it assumed turned out to be missing: `16 local api surface` had `GET /v1/runs/{id}` and no way to ask **which runs there are**.
A surface that can only read one run by id cannot draw a run line.
`GET /v1/runs` is therefore a new operation under the additive-only promise, sharing one view builder with the single read so the two can never disagree about what a run is.

Verified live, and the rule held visibly:

| Before cancel | After cancel |
| --- | --- |
| `running`, `$0.00`, verbs **steer** and **cancel** | `cancelled`, `$0.33`, **no verbs at all** |

**A surface never offers a verb the runtime would refuse**, so the verb list is derived from lifecycle and control exactly as liveness is derived from a timestamp.
`steer` appears only for a runner with a steering path, because `05 run state model`'s own correction by `20 worker control channel` established that Codex resumption is not steering - offering it there would be a button that fails when clicked.

`re-run` and `re-scope` are deliberately absent from the line. Both start a **new** run and re-scope changes a contract field, so the contract has to be on screen, and this widget does not fake a detail view it does not have.

### The verb is not on the sandbox bridge

`28 operator surface` gate 3 hands a widget `read`, `ask` and its own state slice.
A run verb is not among them, and adding one to the host bridge did not change that: a widget the operator did not write can **show** a run and cannot **cancel** one.

## Implementation note, 2026-08-25: cell zero wrote one, and what it cost to find out

The loop closes: the operator asked, cell zero read `widgets/AGENTS.md`, wrote two files, committed, left a branch, and its worktree was deleted. The canvas showed the branch as a pending widget, **keep** merged it, and it compiled and mounted sandboxed like any other.

Three things had to be fixed before that worked, and each was invisible until a real run hit it.

### A manager reads HEAD, not the working tree

The first attempt failed with `File does not exist` for `widgets/AGENTS.md`.
A run's workspace is a worktree of **HEAD**, so anything a manager must read has to be **committed**.
An uncommitted contract file is a contract the manager cannot see, and it spends real tokens exploring before it gives up.

### A manager that may not write is a manager that hangs

The second attempt read the contract, decided correctly, and stalled on:

> Claude requested permissions to write to `...\widgets\cost-today\widget.json`

`--allowedTools` named only the four `mcp__farseer__*` tools, so every built-in write needed a permission answer nobody was there to give.
This is the same class of bug as the one `06 cell transport` recorded: **a tool being reachable is not the same as being permitted**, and the failure looks exactly like a hang.

`Write`, `Edit` and `Bash` are now granted to a manager whose pinned cell grants a shell-capable tool, and to no other.
That is `12 autonomy and deny list`'s own boundary mapped onto the runner's flag rather than a second policy invented in the adapter - and the API already refuses to launch a native runner in a cell with no shell grant, so the two agree.

**This means `28`'s section 3 was unimplementable as written until now.** "Cell zero authors widget code" was a decision the runtime could not carry out.

### Delivery is a branch, because a worktree is detached

`04 spike workspace teardown` deletes the workspace when the run ends, and `create_worktree` uses `--detach`, so a plain commit there is unreachable once the worktree is pruned.
A **branch ref** survives: worktrees share one object store.

So the contract tells a manager to finish with `git branch farseer/widget/<id>`, and the canvas lists those refs as pending widgets. Keep merges, undo deletes the branch.

### What the widget got wrong, and what that says about the contract

It guessed the shape of `/analytics/cost`, hedging with `total_usd ?? total ?? usd`.
The endpoint answers an **array**, so the widget shows nothing useful.

The contract lists the paths a widget may read and **not what they return**, which is a gap in the contract rather than a mistake by the manager.

### Not verified

**Auto-height.** A sandboxed frame reports its own layout as `0` in this environment, because the browser pane driving the check runs with `document.visibilityState: "hidden"` and does not lay out frame content. The host's own measurements are correct - the frame's box reads 274x120 - so the guard keeps the default height rather than collapsing a widget to nothing. It wants a look in a real window.

### What it cost

Three runs, **$1.35** in total, and two of the three were spent discovering the two bugs above rather than doing the work.
That is the price of the finding, and it is cheaper than shipping a contract nobody could satisfy.

## Decided 2026-08-25: the desktop shell is Tauri, and `serve` stays

This ticket left the framework open on purpose and said only that whatever packaged the canvas had to keep the token out of the browser.
The operator has now fixed the shape: **the final product is a desktop application, and `farseer serve` is optional rather than the way in.**

**Tauri**, for three reasons that are about this project rather than about Tauri:

- **It is Rust.** The runtime is already a Rust binary, so the shell is one more crate in the same workspace rather than a second toolchain to keep current.
- **The shell can own the runtime.** Farseer ships as a sidecar the app spawns, which is what makes `serve` optional: headless stays available for anyone who wants it, and nobody has to run it by hand to open a window.
- **It keeps `28`'s gate 3 honest.** The token stays on the Rust side and the webview never holds it, which is the same property the dev proxy has today rather than a new one to invent.

**Rejected: Electron.** Node is built in, so the widget compiler would move across unchanged, and it matches [baby-menu](https://github.com/kunchenguid/baby-menu), the operator's own reference.
That is a real advantage and it buys **one dependency** at the price of a ~150MB installer and a second runtime beside the Rust one.
Esbuild already ships as a standalone binary, so Tauri can have the same compiler as a sidecar without the runtime.

**Rejected: farseer serves the page itself.**
It is the smallest change and it is not a desktop application - no window, no tray, no single-click launch - and it would put rendering concerns inside the headless runtime that `01 cell primitive` spent a section keeping out.

### What this obliges

The widget host currently lives in a Vite plugin and uses Node for esbuild and git.
Under Tauri it moves to the shell crate: git through `Command`, which farseer already does for worktrees, and esbuild as a sidecar binary.
**The three gates do not change** - only what runs them.

## Settings, 2026-08-25: the shell edits definitions, the API still cannot

The operator asked for a way to pick which harness stands in front of farseer.

That is a change to a **cell definition**, and `16 local api surface` gave `/v1` read, validate and reload with **no edit path** - because `01 cell primitive` made a definition data in git rather than a row in a database.

So the setting is not a new API operation. **The shell writes the file and asks the runtime to reload it**, which is the same split widget code already uses: the shell owns the filesystem, the runtime owns the record.

Three properties fall out, and all three were the reason for the original ruling:

- **The change leaves a git diff.** `22 cell addressing` already leaned on exactly that when it refused an in-conversation override: editing the definition and reloading "takes about ten seconds and leaves a git commit".
- **Validation stays in one place.** The shell writes, calls reload, and hands back whatever the runtime says. It does not judge the definition itself, so there is no second opinion to disagree with the first.
- **One line changes.** A whole-file rewrite through a TOML serializer would reformat the operator's own comments and ordering away, so this replaces a single line and keeps the file's own line endings. A rewrite that flips every ending shows up as a whole-file diff and buries the line that actually changed.

### What is offered is what is installed

`10 runner inventory`'s rule is that reach is **observed, never advertised**, and presence is the same kind of claim: a runner that is not on `PATH` is shown as unavailable rather than offered, because the alternative is a run that fails at spawn.

The runner name and its executable are **not the same string** - `claude-code` is driven by a binary called `claude` - and resolving the name instead of the executable reports a runner as missing while farseer launches it happily.

### Where this goes

The operator's stated direction: farseer eventually has **its own harness**, and the pluggable ones become floor managers and sub-agents rather than the thing in front.
Nothing here forecloses that. The top manager is a runner named in a definition, and a farseer-native harness would be one more name in the same field.
