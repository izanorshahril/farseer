# Working in this repository

Twenty-eight decision tickets are closed, and the foundation is implemented against them: `farseer-core`, `farseer-store`, `farseer-api`, `farseer-runner`, `farseer-manager`, and the `farseer` binary.

## Scope

Farseer builds, tests and runs with **`cargo` and `bun` alone**.
`cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt` and `bun run --cwd ui check` are the whole toolchain, and every live-runner test is `#[ignore]`d behind them.
**`--workspace` is not optional on either**: `cargo run` opens the desktop shell, which means `default-members` is the shell, which means a bare `cargo test` covers the shell and nothing else.

No push gate, review pipeline or daemon is part of this project. If one is installed on a machine it belongs to that machine, not to farseer, and farseer must keep working when it is absent.

**`herdr`, `firstmate` and `buzz` appear in the decision record as prior art, never as dependencies.**
`18 hang detection prior art` surveyed their Windows failures and `03 spike job objects` is the answer to them - Job Objects and explicit `.exe`/`.cmd` resolution exist *because* those tools' bugs traced to implementation choices rather than platform walls.
Reading those names as integration points inverts the finding.

`farseer-runner` resolves `claude`, `codex`, `cursor-agent`, or `goose`, builds each argv, supervises the process under a Job Object, maps verified stream-json shapes, and creates or tears down workspaces according to `04 spike workspace teardown`.
It also speaks **ACP** ([`acp.rs`](crates/farseer-runner/src/acp.rs), [`acp_drive.rs`](crates/farseer-runner/src/acp_drive.rs)), which is one adapter rather than a fifth dialect: `goose-acp` and `opencode-acp` ship today and the same code path admits Gemini CLI, Amp, Droid, Copilot, Qwen, pi and Aider.
An ACP runner name means an **executable and a subcommand**, because `goose` and `goose-acp` are one binary offering two faces and report different things - the ACP face names a **context window** and no native runner does, and it reports **no subscription window**, which is what `27 quota accounting` runs on.
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
Cross-cell delegation is implemented as the manager-scoped `delegate_to_cell`: a `kind = "cell"` roster grant, an ungranted name refused however it is phrased, a foreign `peer` refused while `06 cell transport`'s A2A endpoint is off, the ceiling narrowed by roster entry then callee policy, and the caller's budget **reserved** rather than drawn because the call is fire-and-forget.
The callee's manager runs in the callee's cell with the callee's workspace, runner and tool grants, under the caller's task id, and the caller keeps one `cell_called` event naming the callee's run.
The `a_manager_reaches_another_cell_through_farseers_mcp_face` test proves the live round trip: HTTP instruction -> Claude manager in cell zero -> farseer MCP -> a Goose manager in cell social, finishing `ok` under the caller's task.
`31 manager delegation reach` closed that: four transports cover every manager runner - a generated MCP config for `claude-code`, `thread/start` `config.mcp_servers` for `codex-app-server`, `session/new` `mcpServers` for the ACP runners, and farseer's own extension for `pi` and `omp` - and none of them tells a manager its own token.
A `Worktree` cell uses the repository named by `--repo`, which defaults to the directory where `farseer serve` started because `13 harness build kit` keeps git paths out of `CellDefinition`.

The operator surface is built: `28 operator surface` made the **canvas the home screen**, and anything that is not the canvas a **widget** on it.
A widget is a **cell's face** - it renders, and the cell behind it thinks - so there is no second place agents run and no new API operation.
A widget **displays** a cell and never **addresses** one: every AI input from every widget goes to the **top manager**, which decides where the work goes, while operator verbs on a run stay direct.
That made cross-cell delegation a **blocking dependency** of the operator surface rather than merely open, since a widget fronting any cell but zero needs the top manager to reach outward, and it is why `delegate_to_cell` was built before the canvas.
Widget code is authored by cell zero into `widgets/` in git and farseer never stores it, which keeps `01 cell primitive`'s no-plugin-ABI ruling intact: the loader lives in the client, and the runtime still loads nothing.
The dev server compiles a widget on request; `bun run --cwd ui build` compiles them into `ui/dist/widgets/`, which is where the **desktop shell** serves them from - so a new widget appears immediately under `bun run dev` and after a build in the packaged app.
`widgets/sandbox-probe/` asserts the seven boundaries from inside the frame and publishes its verdict through `saveState`, because the frame is deliberately unreadable from outside.

`27 quota accounting` is wired: a runner's `rate_limit_event` becomes a `WindowObservation` keyed by the **account** declared in `runners.toml` (`farseer serve --runners`), appended to the record **on change only** with `actor: system`, and current state derives from the latest event exactly as liveness derives from a timestamp.
`GET /v1/quota` reports `allowed` / `exhausted_until` / `unknown`, a `resets_at` countdown, farseer's own spend since the window opened, and the runners sharing that account.
**It never computes a percentage**, and tests assert its absence: farseer's own spend is a lower bound on a window drained by sessions it cannot see, so a derived percentage would be most wrong exactly near exhaustion. A percentage the **provider** states travels as-is, which is a different number reached a different way. An undeclared runner is its own account, which declines to merge rather than guessing at a shared login.
`[usage] source = "omp"` in `runners.toml` reads every provider omp is signed into while nothing is running - five here, including the only Google quota farseer can see - and windows are grouped by the **provider** rather than the login, because one login spans several providers.

`GET /v1/runs` lists recent runs, newest first, sharing one view builder with `GET /v1/runs/{id}` so a list row and a single read can never disagree.
The **Runs** widget draws `05 run state model`'s verbs on that line and derives which ones to offer from lifecycle, control and the runner's steering path - a finished run offers none, and `steer` never appears for a runner that cannot take one.
A run verb is on the host bridge and deliberately **not** on the sandbox bridge: a widget the operator did not write can show a run and cannot cancel one.

**`session_started`** carries what a runner says about the session it opened - Claude Code names a model and a session on `system/init`, Codex names a thread and no model - and the observed model now fills `RunRow.model`, which was empty from the beginning because nothing read that line.
A field a runner declines to report stays absent rather than being defaulted, per `10 runner inventory`'s observed-never-advertised rule.

A manager's own words reach the record as **`manager_answered`**, appended per turn rather than at the end - `10 runner inventory` observed that a Claude Code manager on live stdin answers and stays alive for the next steer, so holding the text until the run finished would mean the operator hears nothing until they close the session they are talking to.
**`run_finished`** is `05 run state model`'s lifecycle kind, which nothing emitted until now; it carries outcome, text, cost and tokens, and a run with no report quotes its error rather than inventing an apology in the manager's voice.
Together they are what makes `16 local api surface`'s "the answer arrives on the event stream" true, and the **Conversation** widget is built from them.

The canvas reads the record live: `src/stream.ts` follows `/v1/stream` and the **Activity** widget renders it, so an instruction the composer fires has somewhere to land.
It parses SSE by hand rather than using `EventSource`, because farseer puts the **event kind** in the `event:` field and `EventSource.onmessage` fires only for unnamed events - it would silently receive nothing.
The cursor is exclusive, so a dropped connection resumes with no gap and no duplicate.

The canvas from `28 operator surface` lives in [`ui/`](ui/README.md) - Vite, React and TypeScript, run with `bun`, and a client of `/v1` like any other per `01 cell primitive`'s headless ruling.
The dev proxy attaches the operator token so **the browser never holds it**, widgets reach farseer only through `src/bridge.ts`, and the canvas arrangement round-trips through `PUT /v1/ui-state/canvas` rather than `localStorage`.
`28`'s three gates are built and each was tested by attacking it: the **import allowlist** refuses `node:fs` and any path outside the widget's own directory at compile; the **sandboxed render** is an opaque-origin iframe where a hostile widget reached the host bridge and nothing else - not the parent page, not `localStorage`, not cookies, not a direct fetch of farseer; **keep or undo** is git scoped to `widgets/`.
Widgets in `widgets/` are discovered and compiled by `ui/plugins/widget-host.ts`, never by the runtime, so `01 cell primitive`'s no-plugin-ABI ruling still holds.
**Cell zero has now written one end to end**: it read [`widgets/AGENTS.md`](widgets/AGENTS.md), wrote the two files, committed, and left `farseer/widget/<id>` - a branch survives because worktrees share one object store, while `04 spike workspace teardown` deletes the detached worktree the commit was made in. The canvas lists such branches as pending widgets; keep merges, undo deletes.
Two facts that made it impossible until then: a manager's workspace is a worktree of **HEAD**, so anything it must read has to be committed; and `--allowedTools` naming only the MCP tools left every built-in write waiting on a permission prompt, which looks exactly like a hang.

The MCP face from `02 record scope` is nested at `/v1/mcp` in the same router and process because `09 store decision` requires one process and one writer.
The exact-pinned `rmcp` streamable-HTTP service shares `AppState`'s `Store` and loopback/token guard; only `/v1/mcp` accepts the per-manager bearer, while operator routes still require the process-wide token.
The four tools are manager-scoped `read_memory`, `write_memory`, `delegate_to_worker`, and `delegate_to_cell`; every call derives identity and memory scope from the active pinned manager context, and no tool appends a raw event.
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

The canvas has its own check, and it is a typecheck rather than a test suite - there is nothing there yet whose behaviour a test would pin:

```bash
bun run --cwd ui check
```

`cargo clippy --workspace --all-targets` is expected to be silent, and `cargo fmt --all` is applied before every commit.
`.github/workflows/check.yml` runs both on `windows-latest` with `RUSTFLAGS: -D warnings`, so a warning fails the build rather than accumulating.

Ten tests in `farseer-api` are `#[ignore]`d: seven spawn a real headless `claude` process, one spawns Goose, and the full manager-loop and cross-cell tests spawn both.
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

- **A conversational runner must not be read to end of stream.** Observed 2026-08-26: an ACP agent does not exit when a turn ends - the session stays open for the next prompt - so `drive`'s drain-until-EOF loop waits forever, and the first live ACP run hung until it was killed. This is `28`'s stdin bug seen from the other side, and both are the same missing distinction: **machinery correct for a one-shot runner is silently wrong for a conversational one, and both present as a live process producing nothing.** `Channel::{OneShot, Steered, Acp}` names whether a stdin exists, how the goal gets in, and what ends the read loop, in one place.
- **An ACP agent streams its answer a fragment at a time**, so recording each fragment puts several `manager_answered` events in the record for one sentence - `goose acp` sends "Hello" then "!". `05 run state model` had already ruled that **token streams are activity, not progress**; `RunnerSignal::OutputChunk` accumulates and one `Output` is emitted per turn.
- **A one-shot runner must be spawned with a *closed* stdin, not an open pipe nobody writes to.** Observed 2026-08-25: `codex exec` prints "Reading additional input from stdin..." and waits for EOF **before it starts work**, so a pipe held open for the process's lifetime means it never begins - a live process, zero output, and a run that sits at `running` forever with no events and no cost. `SupervisedProcess::spawn` now takes a `StdinMode`, and the **steer frame decides it**: a runner something will steer gets a pipe, and everything else gets EOF at spawn.
- **A manager stalls on any built-in tool it was not granted, too.** Observed 2026-08-25: a manager told to write a widget read its contract, decided correctly, then sat on "Claude requested permissions to write to ...". `Write`, `Edit` and `Bash` are granted only to a manager whose cell grants a shell-capable tool, per `12 autonomy and deny list`.
- **A run's workspace is a worktree of `HEAD`.** Anything a manager must read has to be **committed** - an uncommitted file is one it spends real tokens failing to find. A commit it makes is unreachable after teardown unless it leaves a **branch**.
- **A manager stalls on any MCP tool missing from `--allowedTools`.** Observed 2026-08-25 while wiring `delegate_to_cell`: the manager called the right tool and received "Claude requested permissions to use `mcp__farseer__delegate_to_cell`, but you haven't granted it yet", then sat there. Every offline test passed throughout, because the tool was on the face and only the grant was missing. `invocation.rs` now derives the flag from `MANAGER_ALLOWED_TOOLS` and a test walks the face's own `list_tools` output against it.
- **Claude Code emits `rate_limit_event` on every successful headless run**, carrying `resetsAt` as unix epoch **whether or not the window is exhausted** - confirmed 2026-08-25 by a live run reporting `allowed` with a `resetsAt` present, which is also why `Availability::Allowed` carries one. A cancelled run still reports the window it saw. The documented quota surface is a status line, which **does not fire in `-p` mode**.
- **Claude Code 2.1.233's generated project MCP schema** was probed locally with `claude mcp add --transport http --scope project`: `{"mcpServers":{"name":{"type":"http","url":"http://127.0.0.1:<port>/v1/mcp","headers":{"Authorization":"Bearer <token>"}}}}`.
  Production writes that shape outside the git worktree under a current-user-only DACL, puts only the per-manager bearer in it, passes it through `--mcp-config <file> --strict-mcp-config`, and deletes it independently when the manager exits; the disposable probe directory was deleted after capture.
- **Claude Code 2.1.233 does not pre-enforce `--max-budget-usd`.** A live probe with a `$0.000001` cap reported `$0.131195` and only then returned `error_max_budget_usd`, so farseer refuses bounded currency runs before spawn rather than mapping the flag.
  The full manager-loop probe accepted `--input-format stream-json`, `--append-system-prompt`, `--mcp-config`, `--strict-mcp-config`, and `--allowedTools` together and reached Goose through farseer's MCP face; its `system/init` still listed ordinary built-in tools, confirming `--allowedTools` is not an exclusive allowlist.
- **Codex CLI's answer shape** was probed locally on 2026-08-25: `{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"codex-ok"}}`, followed by `turn.completed`; the adapter maps only that verified `agent_message` item and leaves other `item.*` activity-only.
- **Codex CLI and cursor-agent refuse a fresh directory**, needing `--skip-git-repo-check` and `--trust` respectively. Every run gets a fresh worktree, so an adapter omitting these fails every run while looking like a hang.
- **A runner inherits the operator's configuration directory** unless its adapter prevents it. A one-word prompt cost $0.32 loading plugins nobody granted it.
- **cursor-agent's own invocation and terminal shape**, probed 2026-08-24 against the real, installed `cursor-agent` 2026.08.11-e8db854 and cited in [`cursor_agent.rs`](crates/farseer-runner/src/cursor_agent.rs)'s doc comment: `--print --output-format stream-json --trust`, `is_error` on the terminal `result` event (not `subtype`, same authority order `claude_code.rs` found), and `usage`'s four token fields (`inputTokens`/`outputTokens`/`cacheReadTokens`/`cacheWriteTokens`) - no cost in currency, matching `10`'s finding that only Claude Code reports one.
- **goose's own invocation and terminal shape**, probed 2026-08-24 against the real, installed `goose` 1.47.0 and cited in [`goose.rs`](crates/farseer-runner/src/goose.rs)'s doc comment: `run --no-session -q --output-format stream-json -t "<goal>"`, no fresh-workspace trust gate (verified in a fresh `git init` directory), terminal `complete` line with `total_tokens` and `cost_usd` but no success/failure field at all. This machine's configured goose provider (`chatgpt_codex`) delegates through the already-authenticated `codex` CLI, so the probe spent no new credential.
- **`pi` (badlogic/pi-mono) is wired and is what `cells/zero.toml` runs today**, with `omp` beside it on the same adapter. The credential question that once blocked it was the operator's to answer and they answered it. What it still reports is an **API list price against a subscription**, so every surface labels its cost `at list price, not billed` rather than adding it to anything.

What a runner can reach is **observed, never advertised**. Codex CLI accepted `--sandbox read-only` and created the file anyway.
