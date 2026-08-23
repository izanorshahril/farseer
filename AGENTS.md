# Working in this repository

Twenty-seven decision tickets are closed, and the foundation is implemented against them: `farseer-core`, `farseer-store`, `farseer-api`, `farseer-runner`, `farseer-manager`, and the `farseer` binary.
`farseer-runner` resolves `claude`, `codex` or `cursor-agent`, builds any of the three's argv, spawns and reaps under a Job Object, drains stream-json output through each runner's own mapping (Codex's and cursor-agent's are intentionally shallow past their verified terminal shapes - `20` names `item.*` and cursor-agent's own `tool_call` events, but no ticket captured a literal payload for either, so those count as activity only rather than a guess), and creates and tears down worktrees per `04` (reap first, delete with backoff, no rename-quarantine - `04` disproved it). Claude Code is spawned with `--input-format stream-json`: no positional prompt, so `farseer-manager` writes the goal as `claude_code::steer_envelope`'s first stdin line before the read loop starts, and the same envelope carries later steer messages into the same live process. `farseer-manager` calls all of that for one `WorkerContract`, appending its events, finalizing its run row, and exposing a `LivenessHandle`, `CancelToken` and (for a runner with a steering path) `SteerHandle` for `18`/`05`'s watchdog, cancellation and steer - the watchdog itself never calls `cancel`, per `05`'s no-auto-kill rule. `CancelToken` carries a shared `was_cancelled` flag alongside the job handle, so a run ended via `cancel()` is recorded as `05`'s `Cancelled` outcome rather than `Failed`, even though the process's own terminal result never arrives to say so. `SteerHandle` wraps a `StdinHandle` - a clone of the child's stdin, fetched before the blocking read loop takes `&mut` on the process, same pattern as `CancelToken` - so an HTTP request on another thread can reach a live process's stdin; Codex and cursor-agent both get `None` here, since `codex exec resume` and cursor-agent's `--resume`/`--continue` all start a new process rather than continuing this one, per `10`. `farseer-api` calls it: `POST /v1/cells/{id}/instruct` creates a worktree or plain directory per the cell's `workspace_strategy`, runs the cell's manager against a goal (never a roster worker - nothing calls `run_worker` for one yet), and tears the workspace down after; `POST /v1/runs/{id}/cancel` ends a run early; `POST /v1/runs/{id}/steer` writes a follow-up message into a run's live process, `400` if the runner has none; `POST /v1/runs/{id}/rerun` and `/rescope` reconstruct a past run's sealed contract from the `run_queued` event `run_worker` now writes before spawning anything, and start a fresh run linked to the original via the same `rescoped_from` edge `11`'s rework-depth query already reads. `GET /v1/runs/{id}` now also answers `18`/`05`'s liveness question, read from the `LivenessHandle` `on_started` hands to an in-memory registry - `null` once a run finishes and its entry is removed, or after a restart, since `17` chose no orphan survival over run survival. A `Worktree` cell's runs are worktrees of whatever `--repo` names, defaulting to wherever `farseer serve` was started - `13` keeps no git flag on `CellDefinition`, so there is nowhere else for that path to come from. All four of `05`'s manager verbs - cancel, rerun, rescope, steer - are now implemented; there is still no manager loop deciding delegation.

`02` section 8's MCP face is built too, nested into the same router at `/v1/mcp` rather than spawned as a second process: `09` frames "one process, one writer" as a property of the architecture, and a stdio-transport MCP server is normally spawned per client, which here would have meant a second process opening the same SQLite file. The streamable-HTTP transport (`rmcp`, exact-pinned at `3.1.4`) instead nests a `tower` service into `farseer-api`'s existing `Router`, sharing `AppState`'s `Store` and its loopback/token guard. Two tools only, matching the ticket: `read_memory` and `write_memory` - nothing that appends a raw event, since `02`: "an agent that can forge events can rewrite its own history." `write_memory` refuses the `global` tier outright, since `25` gates promotion on the operator and this face does not offer it. Tested against a real `rmcp` client over a real bound socket, not a hand-rolled JSON-RPC request, in `crates/farseer-api/src/lib.rs`'s `the_mcp_face_writes_reads_back_and_refuses_the_global_tier`.

The spikes under `.scratch/farseer/spikes/` are **not** part of the workspace and are excluded from it.
They are evidence, not a foundation to build on.

## Read the map before proposing architecture

[`.scratch/farseer/map.md`](.scratch/farseer/map.md) indexes every decision in one line each and links to the ticket holding the detail.
A decision lives in exactly one place: its ticket.

Before answering any question about how farseer should work, check whether a ticket already answers it.
Twenty-seven of them do, and they carry the reasoning, the rejected alternatives, and what the answer cost.

When a ticket turns out to be wrong or superseded, **append the correction to that ticket and to the map**, rather than editing the original text in place.
The map's own list of corrections is how a reader knows a resolution has moved.

## Use the locked glossary

[14 vocabulary lock](.scratch/farseer/issues/14-vocabulary-lock.md) is authoritative and every later document uses it.

Two words were retired and stay retired:

- A slot that executes work is a **runner**. Never a *seat*.
- The payload a manager gives a worker is a **worker contract**; the payload a manager sends another cell is a **cell call**. Never an *envelope*, which meant both.

A new noun needs a reason. Prefer widening an existing one.

## Writing conventions

- **Use a plain dash.** The em dash appears nowhere in this repository.
- **One sentence per line** in markdown, keeping normal markdown structure otherwise.
- Refer to a ticket by its **name**, not its number alone. `01, 07, 22` is illegible; names read at a glance.

## Platform

**Windows native first.** mac and Linux are a later milestone and are expected to be a subtraction of Windows workarounds, never a v1 constraint.

Toolchain is `x86_64-pc-windows-msvc` on rustup stable, chosen in [19 rust toolchain](.scratch/farseer/issues/19-rust-toolchain.md) because crates are tested on MSVC first.

Two platform facts the spikes established the hard way:

- **Process identity is `(pid, creation_time)` or job membership, never a pid alone.** Windows recycles pids aggressively, and parent-pid tree reconstruction terminated an unrelated application during `jobspike`.
- **Resolve a bare command against `PATHEXT` before accepting it.** An extension-less `npm` on PATH is a POSIX shell script, not the `npm.cmd` you wanted.

## Building

```bash
cargo test --workspace
```

`cargo clippy --workspace --all-targets` is expected to be silent, and `cargo fmt --all` is applied before every commit.

Two conventions the code follows and a reader should keep:

- **Cite the ticket in the doc comment.** Every non-obvious rule in the code says which ticket decided it and why, so the reasoning is one grep away rather than one archaeology session away.
- **Name a test after the behaviour, not the function.** `a_rebound_host_is_refused_before_the_token_is_even_checked` states a decision; `test_guard` states nothing.

`farseer-core` is pure: no clock, no filesystem, no network. Anything needing those takes them as arguments, which is why the liveness rules are testable without waiting ten minutes.

## Running a spike

Each spike has its own README with modes and expected output.

```bash
cargo run --release --manifest-path .scratch/farseer/spikes/jobspike/Cargo.toml -- job
```

Spikes exist to unblock decisions and are not the product. Treat them as evidence, not as a foundation to build on.

## Facts observed on this machine

Recorded in [10 runner inventory](.scratch/farseer/issues/10-runner-inventory.md), and each was measured rather than read from documentation:

- **Claude Code emits `rate_limit_event` on every successful headless run**, carrying `resetsAt` as unix epoch. The documented quota surface is a status line, which **does not fire in `-p` mode**.
- **Codex CLI and cursor-agent refuse a fresh directory**, needing `--skip-git-repo-check` and `--trust` respectively. Every run gets a fresh worktree, so an adapter omitting these fails every run while looking like a hang.
- **A runner inherits the operator's configuration directory** unless its adapter prevents it. A one-word prompt cost $0.32 loading plugins nobody granted it.
- **cursor-agent's own invocation and terminal shape**, probed 2026-08-24 against the real, installed `cursor-agent` 2026.08.11-e8db854 and cited in [`cursor_agent.rs`](crates/farseer-runner/src/cursor_agent.rs)'s doc comment: `--print --output-format stream-json --trust`, `is_error` on the terminal `result` event (not `subtype`, same authority order `claude_code.rs` found), and `usage`'s four token fields (`inputTokens`/`outputTokens`/`cacheReadTokens`/`cacheWriteTokens`) - no cost in currency, matching `10`'s finding that only Claude Code reports one.

What a runner can reach is **observed, never advertised**. Codex CLI accepted `--sandbox read-only` and created the file anyway.
