# farseer-shell

The desktop shell is the Tauri client for the headless farseer runtime, chosen in [28 operator surface](../../.scratch/farseer/issues/28-operator-surface.md).

It finds or starts a local daemon, serves the compiled canvas and widget bundles, proxies authenticated `/v1` requests, and owns desktop-only state such as the tray and window geometry.
The runtime remains headless and the webview never receives the operator token.

## Runtime and browser boundary

The shell attaches to a running daemon when its runtime file names one that answers.
If no daemon answers, it starts one as a sidecar and leaves that daemon running when the window closes.
The daemon lifecycle is controlled through the runtime commands, independently of whether this shell started it.
Both paths verify that the advertised port answers before accepting the runtime file.

The shell serves the built canvas from `ui/dist` and widget bundles from `ui/dist/widgets`.
It proxies `/v1` with the operator token on the native side and rejects an untrusted browser origin before forwarding a request.
The shell also serves its local settings routes and the widget bundle routes used by the canvas.

## Running it

Build the UI before starting the desktop shell:

```bash
bun run --cwd ui build
```

Build the daemon sidecar once in a fresh checkout because the workspace default builds only the shell:

```bash
cargo build -p farseer --bin farseer
```

```bash
cargo run -p farseer-shell
```

`cargo run` uses the workspace default and opens the same shell.
Use `bun run --cwd ui dev` when iterating on the canvas against a separately running `farseer serve` process.

## Desktop behavior

The shell remembers window geometry through the runtime UI-state API and exposes a tray entry for quota windows.
The tray displays provider observations and does not duplicate run controls that belong on the canvas.
The shell does not load widgets from a runtime plugin directory and does not add a plugin ABI.

## Packaging

The packaging command is:

```bash
bun run --cwd ui build && cargo build --release --workspace && cargo tauri build
```

The Tauri CLI is a packaging prerequisite outside the normal cargo and bun validation loop.
