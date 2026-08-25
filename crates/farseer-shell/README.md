# farseer-shell

The desktop shell, chosen in [28 operator surface](../../.scratch/farseer/issues/28-operator-surface.md).

It does three things and deliberately not a fourth.

1. **Finds a farseer.** Attaches to a running daemon if the runtime file names one that answers, and starts one as a sidecar otherwise. A daemon it started dies with the window; one the operator started does not, because `01 cell primitive` requires the runtime to outlive any UI.
2. **Serves the canvas** on a loopback port the OS chooses, so two windows never collide.
3. **Proxies `/v1`** with the operator token attached on this side.

The webview therefore loads **one origin** and the page never holds a credential - the same property the Vite dev proxy has, and the one `28`'s third gate is about.

## This is not the runtime serving HTML

`01 cell primitive` kept rendering out of the runtime, and it still is: farseer serves no HTML and knows nothing about widgets.
The shell is a client of `/v1` like the CLI, and it happens to hand the page to its own webview.

## Running it

```bash
bun run --cwd ui build
cargo run -p farseer-shell
```

A stale runtime file outlives a crashed daemon, so the file alone is a claim rather than a fact.
Both paths check the port actually answers before believing it.

## Not here yet

- **The widget host.** `/__widgets` still lives in the Vite plugin, so agent-authored widgets mount in `bun run dev` and not in the shell. Porting it means git through `Command`, which farseer already does for worktrees, and esbuild as a sidecar binary.
- **A tray icon, and a window that remembers its size.**
- **`tauri build`**, which is what turns this into an installer.
