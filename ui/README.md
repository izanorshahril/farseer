# The canvas

Farseer's operator surface, decided in [28 operator surface](../.scratch/farseer/issues/28-operator-surface.md).

The canvas is the home screen, and **if it is not the canvas it is a widget on it**.
There is no second layout and no mode switch.

## Home composition

The production home uses the accepted Berd-inspired direction: a pale dotted workbench, persistent widget sidebar, compact top bar, rounded saved widgets, and one floating composer.
The clock is a normal optional widget, so its visibility, order, and size use the same persisted arrangement as every other widget.
The visual shell changed without changing the product boundary: widgets still display cells, every AI request still goes to the top manager, and run verbs remain direct.
The version-eight default keeps four operational faces mounted: Conversation, Work, Fleet, and Capacity.
Settings is a top-bar popover, while narrower diagnostics remain optional widgets.

## Running it

For the desktop shell, rebuild `ui/dist` and then start Farseer:

```bash
bun run --cwd ui build
```

```bash
cargo run
```

For Vite development against a running `farseer serve` process:

```bash
bun run --cwd ui dev
```

The dev server reads farseer's own runtime file for the port and the token, so there is nothing to paste.
Set `FARSEER_PORT` and `FARSEER_TOKEN` only to point at a second daemon.

Check the canvas layout contract:

```bash
bun run --cwd ui test
```

```bash
bun run --cwd ui check
```

The **proxy** attaches the token, so the browser never holds it.
That is `28`'s third gate applied one level up: a widget must not hold the operator token, and holding it in the host page would only move the problem into `localStorage`, where clearing site data or any script on the page reaches it.
Whatever packages this later - served by farseer itself, Tauri, Electron - has to keep that property.

## What is here

| File | What it decides |
| --- | --- |
| [`src/App.tsx`](src/App.tsx) | the canvas shell, version-eight four-widget default, optional registry, settings popover, and one composer |
| [`src/layout.ts`](src/layout.ts) | validates, snaps and bounds the persisted widget arrangement |
| [`src/selection.ts`](src/selection.ts) | one shared conversation, task, run, project, and manager-runner context |
| [`src/widgets/work.tsx`](src/widgets/work.tsx) | task board, conversation list, causal graph, completed work, transcript custody, and manager selection |
| [`src/widgets/conversation.tsx`](src/widgets/conversation.tsx) | the selected durable conversation across all of its runs and harness sessions |
| [`src/widgets/fleet.tsx`](src/widgets/fleet.tsx) | the loaded cell definitions |
| [`src/widgets/quota.tsx`](src/widgets/quota.tsx) | `27 quota accounting`'s Capacity surface |
| [`src/widgets/clock.tsx`](src/widgets/clock.tsx) | the optional local-time widget |
| [`src/bridge.ts`](src/bridge.ts) | **everything a widget may reach, and nothing else** |
| [`src/stream.ts`](src/stream.ts) | follows `/v1/stream`, reconnecting on its own |
| [`src/widgets/activity.tsx`](src/widgets/activity.tsx) | the record, live - where an answer lands |
| [`src/widgets/runs.tsx`](src/widgets/runs.tsx) | optional run-level diagnostics and control |

### The bridge is the seam

A widget never calls `fetch`, never holds a token, and never touches the file system.
It reaches farseer only through the `Bridge` object the host passes in, and `28`'s import allowlist is what will catch a widget that tries to leave that seam.

There is deliberately **no `instructCell(id)`**.
`28`'s correction: a widget **displays** a cell and never **addresses** one, so `ask` goes to the top manager, always, carrying an anchor that says what the operator was looking at.
Routing is a decision, not a click.
Ask goals also carry the shared project, conversation, and selected manager candidate, and the accepted task/run becomes the new shared subject.

### The arrangement lives in farseer, not the browser

`PUT /v1/ui-state/canvas`, which farseer stores as an **opaque blob it never parses**, per [24 ui state persistence](../.scratch/farseer/issues/24-ui-state-persistence.md).
`localStorage` was rejected there for three reasons the operator would actually hit: a Tauri window and a browser tab would not share a layout, clearing site data would destroy the dashboard silently, and it is not covered by any backup of farseer's data directory.
Every widget starts at `1x1` and can use `1x1`, `2x1`, `2x2`, or `1x2`.
The sidebar configures the width and height represented by `1x` in one-pixel steps, and that metric is stored with the arrangement.
Dragging lifts the picked widget and pulses the hovered target, then places the picked widget into that target's original slot from either direction.
Right-click a widget to move it left or right, reset it to `1x1`, or unpin it from the canvas.

### The quota widget has no progress bar, on purpose

Farseer's own spend is a **lower bound** on a window that other sessions also drain, so a bar would be wrong in a way the operator could not detect - and most wrong exactly near exhaustion, when they would trust it most.
It shows what is true instead: `allowed` or `exhausted`, a countdown, and what the fleet itself spent.

## The three gates

Widgets under [`../widgets/`](../widgets) are discovered, compiled and sandboxed here - never by the runtime, which is what keeps `01 cell primitive`'s no-plugin-ABI ruling intact.

| Gate | Where | What it refuses |
| --- | --- | --- |
| import allowlist | [`plugins/widget-host.ts`](plugins/widget-host.ts) | `node:fs`, `node:http`, anything outside the widget's own directory |
| sandboxed render | [`src/SandboxWidget.tsx`](src/SandboxWidget.tsx) | the host page, `localStorage`, cookies, and every direct `fetch` |
| keep or undo | [`src/GateBar.tsx`](src/GateBar.tsx) | nothing - it makes the turn reversible, scoped to `widgets/` |

The sandbox is an iframe with `sandbox="allow-scripts"` and **no** `allow-same-origin`, so the widget has an opaque origin and its only channel is a `MessagePort`.
A widget written to attack the host reached the bridge and nothing else; the table of what it tried is on `28`.

Adding `allow-same-origin` would hand a widget the host's origin and undo every one of those properties at once.

## Not built yet

- **Response shapes in the contract.** [`widgets/AGENTS.md`](../widgets/AGENTS.md) lists the paths a widget may read but not what they return, and the first widget cell zero wrote guessed wrong about `/analytics/cost`.
- **Auto-height in a real window.** A frame reports its own layout as `0` under a hidden browser pane, so the check that would prove it has not run anywhere that composites.
- **A rendered stream inside the attach.** `07 attach semantics`'s control axis is on the run line now - `observe`, `take over`, `release`, and `intervene` once a run is taken over - but observing still means reading the Activity widget beside it rather than a stream scoped to the run the operator attached to.
