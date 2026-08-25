# Working in this repository

Twenty-eight decision tickets are closed, and the foundation is implemented against them: `farseer-core`, `farseer-store`, `farseer-api`, `farseer-runner`, `farseer-manager`, and the `farseer` binary.
`farseer-runner` resolves `claude`, `codex`, `cursor-agent`, or `goose`, builds each argv, supervises the process under a Job Object, maps verified stream-json shapes, and creates or tears down workspaces according to `04 spike workspace teardown`.
Codex maps the locally verified `item.completed` `agent_message` answer but leaves other `item.*` activity-only.
Cursor-agent remains shallow past its verified terminal shape because no ticket captured a literal `tool_call` payload.
Goose maps terminal `complete` to `Outcome::Ok` because its verified line carries no failure field.
Claude Code managers use live stream-json stdin for the first goal and later steer messages, while Claude Code workers use one-shot positional goals so synchronous delegation can finish.
`farseer-manager` executes one sealed `WorkerContract`, appends progress with the correct manager or worker actor, finalizes the run row, and exposes liveness, cancellation, and optional steering handles.
The watchdog from `18 hang detection prior art` and `05 run state model` reports only and never auto-cancels.
Explicit cancellation remains `Cancelled` even after a live Claude session emitted a successful earlier turn, while preserving any terminal cost and tokens already observed.
Cancelling a manager marks its ownership context and cancels every active delegated child.
`farseer-api` creates the configured workspace, launches the manager, and tears the workspace down only after the process exits.
A Claude Code manager receives a generated strict MCP config naming farseer's bound `/v1/mcp` endpoint.
The config carries only a per-manager bearer, never the process-wide operator token, lives outside the git worktree under a current-user-only DACL, and is deleted independently when the manager exits.
All four manager verbs from `05 run state model` - cancel, rerun, rescope, and steer - are implemented.
A live Claude Code manager can call `delegate_to_worker`; the runtime validates its per-run capability, resolves the pinned roster, enforces the cell-wide worker cap, preserves the parent task, narrows the cell-root pool through the worker entry's cap and the manager request, draws down reported spend, and returns terminal text, outcome, cost, and tokens.
Every current native LLM runner exposes shell-equivalent reach, so the API refuses to launch one unless the pinned cell explicitly grants a shell-capable tool.
Every bounded budget dimension currently fails closed before spawn because no native runner has demonstrated pre-spend enforcement.
Cross-cell delegation and verified MCP launch wiring for non-Claude managers remain open.
A `Worktree` cell uses the repository named by `--repo`, which defaults to the directory where `farseer serve` started because `13 harness build kit` keeps git paths out of `CellDefinition`.

The operator surface is decided but not built: `28 operator surface` made the **canvas the home screen**, and anything that is not the canvas a **widget** on it.
A widget is a **cell's face** - it renders, and the cell behind it thinks - so there is no second place agents run and no new API operation.
Widget code is authored by cell zero into `widgets/` in git and farseer never stores it, which keeps `01 cell primitive`'s no-plugin-ABI ruling intact: the loader lives in the client, and the runtime still loads nothing.

The MCP face from `02 record scope` is nested at `/v1/mcp` in the same router and process because `09 store decision` requires one process and one writer.
The exact-pinned `rmcp` streamable-HTTP service shares `AppState`'s `Store` and loopback/token guard; only `/v1/mcp` accepts the per-manager bearer, while operator routes still require the process-wide token.
The three tools are manager-scoped `read_memory`, `write_memory`, and `delegate_to_worker`; every call derives identity and memory scope from the active pinned manager context, and no tool appends a raw event.
`write_memory` refuses the `global` tier because `25 memory lifecycle` gates global promotion on the operator.
The MCP tests use a real `rmcp` client over a real bound socket rather than hand-written JSON-RPC.
The ignored `instructing_a_manager_reaches_a_roster_worker_through_farseers_mcp_face` test proves HTTP instruction -> Claude manager -> farseer MCP -> Goose worker -> tool result in one live manager turn.

The spikes under `.scratch/farseer/spikes/` are **not** part of the workspace and are excluded from it.
They are evidence, not a foundation to build on.

## Read the map before proposing architecture

[`.scratch/farseer/map.md`](.scratch/farseer/map.md) indexes every decision in one line each and links to the ticket holding the detail.
A decision lives in exactly one place: its ticket.

Before answering any question about how farseer should work, check whether a ticket already answers it.
Twenty-eight of them do, and they carry the reasoning, the rejected alternatives, and what the answer cost.

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

Nine tests in `farseer-api` are `#[ignore]`d: seven spawn a real headless `claude` process, one spawns Goose, and the full manager-loop test spawns both.
The "a one-word prompt cost $0.32 loading plugins" finding means the Claude tests are real minutes and real cost, not a hang.
`cargo test --workspace` skips them; run an ignored test by name rather than running all nine accidentally.

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
- **Claude Code 2.1.233's generated project MCP schema** was probed locally with `claude mcp add --transport http --scope project`: `{"mcpServers":{"name":{"type":"http","url":"http://127.0.0.1:<port>/v1/mcp","headers":{"Authorization":"Bearer <token>"}}}}`.
  Production writes that shape outside the git worktree under a current-user-only DACL, puts only the per-manager bearer in it, passes it through `--mcp-config <file> --strict-mcp-config`, and deletes it independently when the manager exits; the disposable probe directory was deleted after capture.
- **Claude Code 2.1.233 does not pre-enforce `--max-budget-usd`.** A live probe with a `$0.000001` cap reported `$0.131195` and only then returned `error_max_budget_usd`, so farseer refuses bounded currency runs before spawn rather than mapping the flag.
  The full manager-loop probe accepted `--input-format stream-json`, `--append-system-prompt`, `--mcp-config`, `--strict-mcp-config`, and `--allowedTools` together and reached Goose through farseer's MCP face; its `system/init` still listed ordinary built-in tools, confirming `--allowedTools` is not an exclusive allowlist.
- **Codex CLI's answer shape** was probed locally on 2026-08-25: `{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"codex-ok"}}`, followed by `turn.completed`; the adapter maps only that verified `agent_message` item and leaves other `item.*` activity-only.
- **Codex CLI and cursor-agent refuse a fresh directory**, needing `--skip-git-repo-check` and `--trust` respectively. Every run gets a fresh worktree, so an adapter omitting these fails every run while looking like a hang.
- **A runner inherits the operator's configuration directory** unless its adapter prevents it. A one-word prompt cost $0.32 loading plugins nobody granted it.
- **cursor-agent's own invocation and terminal shape**, probed 2026-08-24 against the real, installed `cursor-agent` 2026.08.11-e8db854 and cited in [`cursor_agent.rs`](crates/farseer-runner/src/cursor_agent.rs)'s doc comment: `--print --output-format stream-json --trust`, `is_error` on the terminal `result` event (not `subtype`, same authority order `claude_code.rs` found), and `usage`'s four token fields (`inputTokens`/`outputTokens`/`cacheReadTokens`/`cacheWriteTokens`) - no cost in currency, matching `10`'s finding that only Claude Code reports one.
- **goose's own invocation and terminal shape**, probed 2026-08-24 against the real, installed `goose` 1.47.0 and cited in [`goose.rs`](crates/farseer-runner/src/goose.rs)'s doc comment: `run --no-session -q --output-format stream-json -t "<goal>"`, no fresh-workspace trust gate (verified in a fresh `git init` directory), terminal `complete` line with `total_tokens` and `cost_usd` but no success/failure field at all. This machine's configured goose provider (`chatgpt_codex`) delegates through the already-authenticated `codex` CLI, so the probe spent no new credential.
- **`pi` (badlogic/pi-mono, `~/.bun/bin/pi` 0.84.2) is installed but has no ready provider on this machine** - `pi auth check --provider google --json` answers `credentials_not_configured`, and its default provider is `google`. Wiring it as a fifth runner needs an API key configured first (`pi auth`, or one of its many `*_API_KEY` env vars) - that is a credential decision for the operator, not one this session makes unilaterally, so `pi` stays unimplemented pending that choice.

What a runner can reach is **observed, never advertised**. Codex CLI accepted `--sandbox read-only` and created the file anyway.
