# Current implementation and documentation status

**Snapshot:** `main` at `e9d2e76`.
**Updated:** 2026-09-05.

This file is a current status snapshot.
It replaces the 2026-08-31 branch review, whose findings were resolved or superseded by later changes.

## Current implementation

- `farseer-core`, `farseer-store`, `farseer-api`, `farseer-runner`, `farseer-manager`, the CLI and the desktop shell are implemented as one workspace.
- Native and ACP runner paths supervise processes with Windows Job Objects, map only verified runner shapes, and create or tear down run workspaces according to the sealed contract.
- Manager delegation covers roster workers and granted cells through the MCP face, plain manager routes, Codex app-server handshakes, ACP session configuration, and the pi/omp extension.
- Run control includes cancel, steer, rerun, rescope, observe, take over, release, heartbeat and intervene, with lifecycle and purge routes for cells.
- A2A is available through the optional Agent Card and JSON-RPC routes, with per-peer authentication and cell binding.
- The durable work model covers conversations, tasks, project roots, transcripts, session references, causal edges, search and the shared canvas projections.
- The canvas, compiled agent widgets, sandbox gates, live event stream, quota surface, tray and persisted layout are shipped in the current UI and shell.

## Current limits

- Bounded native-runner budgets fail closed before spawn because pre-spend enforcement is not verified for the installed runners.
- Gated actions remain a decided model without a complete operator interaction surface.
- Third-party MCP tool-server clients are not supported; farseer exposes its own manager-scoped MCP face.
- Provider-trusted cost is unavailable for several runners, and pi's value is labelled as list price rather than billed spend.
- A packaged widget appears after the UI build and reload, and the canvas does not yet provide automatic frame height or a run-scoped stream inside an attached view.
- Ticket 38 still records the open question about whether a shell roster entry and `ToolLevel::Shell` should remain separate statements.
- Retention policy, runtime self-modification, migration, sandbox upgrade and Dev Drive/ReFS snapshot support remain future scope in the decision map.

## Documentation authority

| Question | Read first |
| --- | --- |
| Current product and API behavior | [README.md](README.md) and source route types |
| Agent constraints and repository workflow | [AGENTS.md](AGENTS.md) |
| Decision, correction or open question | [.scratch/farseer/map.md](.scratch/farseer/map.md) and its linked ticket |
| Canvas and authored widget behavior | [ui/README.md](ui/README.md) and [widgets/AGENTS.md](widgets/AGENTS.md) |
| Runner contract and observed provider behavior | [HARNESS.md](HARNESS.md) and the runner inventory ticket |
| Historical reasoning | [BRIEF.md](BRIEF.md) and [ARCHITECTURE.md](ARCHITECTURE.md), both marked as historical |

Do not infer an open implementation gap from a question in a historical draft.
Check the map and the current source before creating follow-on work.

## Validation

The current documentation and source-adjacent changes pass the repository checks.

| Check | Result |
| --- | --- |
| `cargo test --workspace` | Passed outside the restricted sandbox; live-runner tests remain ignored by design |
| `cargo clippy --workspace --all-targets` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `bun run --cwd ui test` | Passed, 6 tests |
| `bun run --cwd ui check` | Passed |
| Markdown relative-link scan | Passed |
| `git diff --check` | Passed |

The first sandboxed Rust run hit Windows ACL and temporary-file permissions in environment-sensitive tests.
The same workspace command passed outside that restricted environment.

The repository validation loop is:

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets
```

```bash
cargo fmt --all -- --check
```

```bash
bun run --cwd ui test && bun run --cwd ui check
```
