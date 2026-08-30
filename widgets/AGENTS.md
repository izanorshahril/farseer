# Writing a farseer widget

You are being asked for a widget on the operator's canvas.
This file is the whole contract. Read it, write the two files, leave the branch.

## What a widget is

A **face for a cell**, per [28 operator surface](../.scratch/farseer/issues/28-operator-surface.md).
It renders. The cell behind it thinks.

A widget **displays** a cell and never **addresses** one.
There is no call that reaches a particular manager: `farseer.ask` goes to the top manager, which decides where the work goes.
That is deliberate, and there is no way around it.

## The two files

Create `widgets/<id>/` with a kebab-case id, holding exactly:

**`widget.json`**

```json
{ "title": "Run tally", "subtitle": "outcomes by cell", "cell": "zero" }
```

`cell` is optional and names what the widget fronts, if anything.

**`widget.tsx`** - renders into `#root`, with React:

```tsx
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

declare const farseer: {
  read: <T>(path: string) => Promise<T>;
  ask: (text: string) => Promise<string>;
  loadState: <T>(key: string) => Promise<T | null>;
  saveState: (key: string, value: unknown) => Promise<void>;
};

function Widget() {
  const [rows, setRows] = useState<unknown[] | null>(null);
  useEffect(() => { farseer.read<unknown[]>("/runs?limit=10").then(setRows); }, []);
  return <div>{rows ? rows.length : "loading"} runs</div>;
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<Widget />);
```

Local helper files in the same directory are fine. Nothing else is.

## What you may import

`react`, `react/jsx-runtime`, `react-dom/client`, and your own local files.

**Everything else fails to compile**, including `node:fs`, `node:http`, any npm package, and any path outside your own directory.
This is not advisory. The build refuses it and the widget does not mount.

## What `farseer` can reach

| Call | What it does |
| --- | --- |
| `farseer.read(path)` | any `/v1` **read**: `/runs?limit=25`, `/cells`, `/quota`, `/analytics/cost`, `/events?limit=50` |
| `farseer.ask(text)` | sends a request to the **top manager**, anchored to your widget. Returns a run id, not an answer |
| `farseer.loadState(key)` / `saveState(key, value)` | your own slice of the canvas blob, namespaced to you |

There is nothing else. No `fetch`, no token, no file system, no run verbs.
Your code runs in a frame with an opaque origin, so a direct `fetch` fails and reading the host page throws.
Write as if the network does not exist, because for you it does not.

Style: dark background, `#e6edf3` text, `#8b97a6` for secondary, `#58a6ff` for accent, `system-ui` at 13px.
Inline styles are fine. Keep it under about 200px tall.

## How to leave it

Your workspace is a **detached** git worktree. It is deleted when your run ends, so a plain commit is lost.
A branch is not. Finish like this:

```bash
git add widgets/<id>
git commit -m "Add the <id> widget"
git branch farseer/widget/<id>
```

The branch lives in the operator's repository after your worktree is gone.
The canvas shows it as a pending widget with **keep** and **undo**, which is the operator's decision and not yours.

Do not merge it, do not touch `main`, and do not edit anything outside `widgets/`.

## Where a widget runs, and when it appears

Two places, and they are not the same:

- **`bun run --cwd ui dev`** compiles a widget on request, so a file you write appears on the next reload. Gate 1's keep-or-undo lives here too - the canvas can stage, keep and undo what is under `widgets/`, because that is git and the dev server can run git.
- **The desktop app** serves widgets compiled into `ui/dist/widgets/` by `bun run --cwd ui build`. **A new widget needs that build before it appears**, because gate 2's import allowlist is enforced *at compile* and the shipped application does not carry a bundler to re-decide it at runtime.

That is a real limit, stated rather than hidden: authoring is a dev-server loop, and the packaged app shows what the build compiled.

## What the sandbox actually refuses

`widgets/sandbox-probe/` asserts the boundary from the inside rather than describing it, and publishes its verdict through `saveState` so it is checkable from outside a frame that is deliberately unreadable from outside. It is not mounted by default; add it from the header when you want to see it.

| tried | result |
| --- | --- |
| reach the host page | denied by the browser - opaque origin |
| read `localStorage` | denied - there is none |
| `fetch('/v1/runs')` directly | denied - an opaque origin fails CORS everywhere |
| `farseer.read('/runs')` | allowed - the sanctioned channel |
| `farseer.read('/ui-state/canvas')` | **denied by the host** - see below |
| `saveState` then `loadState` | allowed, and scoped to this widget |
| `loadState('../cost-today.x')` | allowed, and reaches nothing - keys are prefixed with your id |

`ui-state` is the one read the host refuses. `saveState` is namespaced per widget so one widget cannot overwrite another's slice **or the canvas layout**; a generic read walked straight past that, and namespacing the write while leaving the read open protects nothing.
