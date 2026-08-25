# The canvas

Farseer's operator surface, decided in [28 operator surface](../.scratch/farseer/issues/28-operator-surface.md).

The canvas is the home screen, and **if it is not the canvas it is a widget on it**.
There is no second layout and no mode switch.

## Running it

`farseer serve` writes its port and token to its runtime file, and the dev server proxies `/v1` to it.

```bash
FARSEER_PORT=9000 FARSEER_TOKEN=<from farseer's runtime.json> bun run --cwd ui dev
```

The **proxy** attaches the token, so the browser never holds it.
That is `28`'s third gate applied one level up: a widget must not hold the operator token, and holding it in the host page would only move the problem into `localStorage`, where clearing site data or any script on the page reaches it.
Whatever packages this later - served by farseer itself, Tauri, Electron - has to keep that property.

## What is here

| File | What it decides |
| --- | --- |
| [`src/App.tsx`](src/App.tsx) | the canvas, the widget registry, and the one composer |
| [`src/bridge.ts`](src/bridge.ts) | **everything a widget may reach, and nothing else** |
| [`src/widgets/quota.tsx`](src/widgets/quota.tsx) | `27 quota accounting`'s utilisation surface |
| [`src/widgets/fleet.tsx`](src/widgets/fleet.tsx) | the loaded cell definitions |

### The bridge is the seam

A widget never calls `fetch`, never holds a token, and never touches the file system.
It reaches farseer only through the `Bridge` object the host passes in, and `28`'s import allowlist is what will catch a widget that tries to leave that seam.

There is deliberately **no `instructCell(id)`**.
`28`'s correction: a widget **displays** a cell and never **addresses** one, so `ask` goes to the top manager, always, carrying an anchor that says what the operator was looking at.
Routing is a decision, not a click.

### The arrangement lives in farseer, not the browser

`PUT /v1/ui-state/canvas`, which farseer stores as an **opaque blob it never parses**, per [24 ui state persistence](../.scratch/farseer/issues/24-ui-state-persistence.md).
`localStorage` was rejected there for three reasons the operator would actually hit: a Tauri window and a browser tab would not share a layout, clearing site data would destroy the dashboard silently, and it is not covered by any backup of farseer's data directory.

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

- **Cell zero writing one.** `widgets/run-tally` is hand-written on purpose - the contract gets proven before a manager is asked to satisfy it. The manager-facing half is a prompt that teaches the widget contract plus the file-writing turn.
- **The event stream.** `ask` returns a run id; the answer arrives on `/v1/stream`, which nothing here reads yet.
- **Run verbs.** `28`'s table puts `steer`, `cancel`, `observe`, `take over` and `release` inline on a run line, and there is no run-line widget yet.
