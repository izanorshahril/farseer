# Farseer

A local-first agent orchestration runtime for Windows.
One operator, one Rust binary, no required external services.

**Status: the foundation is built, and a Claude Code manager can delegate to a real roster worker.**
Twenty-eight decision tickets are closed, and the domain model, the record, the local API, four native runners, all four manager verbs from [Run state model and control semantics](.scratch/farseer/issues/05-run-state-model.md), and the MCP face from [Record scope](.scratch/farseer/issues/02-record-scope.md) are implemented against them.
`POST /v1/cells/{id}/instruct` runs a cell's manager against a goal and returns a `run_id` immediately.
The operator surface is a separate client under [`ui/`](ui/README.md): a canvas of widgets whose arrangement farseer stores as an opaque blob, one composer addressed to the top manager, and a host bridge that is the only thing a widget may reach.

A Claude Code manager receives farseer's own MCP face under a per-run capability and can call `delegate_to_worker`, or `delegate_to_cell` for a granted cell, during the same live conversation; cancel and steer remain available through the run API.
See [What runs today](#what-runs-today).

## The one idea

Farseer is a **cell runtime**, not a coding orchestrator.

A **cell** is a harness: one manager, its workers, its own record scope, and an address.
The builder-and-management harness is cell #0.
Every harness farseer builds is a sibling cell rather than a plugin, so a social media harness and a coding harness differ only in roster, tools and policy values.

The operator talks to a manager. The manager writes contracts, supervises workers, and escalates.
Mechanics never surface in conversation; they are all in the record and on the fleet view.

```mermaid
graph TD
  OP([operator]) -->|instruction| M0

  subgraph C0["cell #0"]
    M0[manager]
    W0[workers]
    M0 --> W0
  end

  subgraph CS["social cell"]
    MS[manager]
    WS[post-writer, video-editor]
    MS --> WS
  end

  M0 -->|cell call, in-process| MS
  M0 -.->|ACP| RUN[foreign agent as runner]
  M0 -.->|A2A, off by default| PEER[foreign orchestrator as peer cell]
  M0 -.->|third-party MCP, not built| TOOL[tools]
  M0 -.->|MCP delegate/read/write| MCPFACE[farseer's own MCP face]
  MCPFACE --> W0
  MCPFACE --> REC

  M0 --> REC[(append-only record SQLite)]
  MS --> REC
  OP -.->|attach, any depth| W0
  OP -.->|attach| WS

  classDef core fill:#1a3a5c,stroke:#4a90d9,color:#fff
  classDef ext fill:#3a3a3a,stroke:#777,color:#ccc
  class M0,MS,W0,WS,REC,MCPFACE core
  class RUN,PEER,TOOL ext
```

Solid edges are native. Dotted edges cross a protocol boundary.
The operator attaches to any run at any depth, bypassing every manager, because observation is not delegation.

## Three protocol boundaries

| Boundary | Protocol | Role |
| --- | --- | --- |
| Foreign agent driven by a farseer manager | **ACP** (Zed) | a **runner** |
| Foreign orchestrator making its own decisions | **A2A** (Linux Foundation) | a **peer cell**, off by default |
| Tools | **MCP** | farseer's own `/v1/mcp` face provides memory and roster-worker delegation, never raw event append; calling third-party MCP tool servers is not built |

An external protocol is spoken at a boundary, never shaped into internals.

## Repository layout

```
.
├─ crates/
│  ├─ farseer-core/    domain model: cells, policy, run state, scrubbing. Pure, no I/O
│  ├─ farseer-store/   the record: one append-only SQLite log, memory, UI state
│  ├─ farseer-api/     local HTTP plus SSE on 127.0.0.1, token and loopback guard; nests the MCP face at /v1/mcp
│  ├─ farseer-runner/  runners: Claude Code, Codex, cursor-agent and goose, PATHEXT resolution, Job-Object spawn, stream-json mapping, worktree lifecycle
│  ├─ farseer-manager/ runs one sealed contract, captures terminal text, and records what happened
│  └─ farseer/         the binary: runtime and CLI in one
├─ ui/                 the canvas: the operator surface, a client of /v1 like any other
│  ├─ src/bridge.ts    everything a widget may reach, and nothing else
│  └─ src/widgets/     quota and fleet, hand-written until 28's gates exist
├─ cells/              cell definitions, hand-written, in git
│  ├─ zero.toml        cell #0, the builder harness
│  └─ social.toml      the second cell, thinner on purpose
├─ runners.toml        machine-wide runner facts: which account each signs in with
├─ BRIEF.md            landscape research, Windows failure catalogue, operator questions
├─ ARCHITECTURE.md     the cell model this map decided on
├─ AGENTS.md           conventions for agents working here (CLAUDE.md points at it)
└─ .scratch/farseer/
   ├─ map.md           the decision route: destination, decisions, fog, out of scope
   ├─ issues/          28 decision tickets, all closed
   ├─ research/        compaction, hang detection, headless UI boundary
   ├─ prototypes/      one operator turn, end to end
   └─ spikes/          jobspike, wsspike, storebench
```

## What runs today

```bash
cargo run --bin farseer -- validate
```

Parses every definition in `cells/`, prints one line each, and exits non-zero if any is broken.

```bash
cargo run --bin farseer -- serve --port 8787
```

Binds `127.0.0.1` only, opens the record, loads the definitions, and writes its port and a fresh token to a file whose DACL grants nobody but the current user.
`--repo <path>` picks which git repository a `Worktree`-strategy cell's runs are worktrees of; it defaults to the current directory if omitted.

| Surface | What it does |
| --- | --- |
| `GET /v1/cells`, `/v1/cells/{id}` | read definitions. There is deliberately **no edit path** - they are files in git |
| `POST /v1/cells/reload` | re-read from disk, reporting broken files rather than dying on them |
| `POST /v1/cells/{id}/instruct` | run the cell's manager against a goal; current native LLM runners require an explicit shell-capable roster grant; a Claude Code manager gets a generated strict MCP config outside the worktree and may delegate to named roster workers; returns `202` with a `run_id` after setup is accepted |
| `GET /v1/events?cell=&run=&since=` | the cursor read. `since` is exclusive, so a client resumes with no gap and no duplicate |
| `GET /v1/stream` | the same query as SSE, honouring `Last-Event-ID`. Attach and replay are one call with a different cursor |
| `GET /v1/runs/{id}` | a run's row: lifecycle, outcome, cost, tokens, and `18`/`05`'s liveness - `live`/`stalled`/`likely_hung`, or `null` once nothing in memory can answer |
| `POST /v1/runs/{id}/cancel` | end a run early, recorded as `05`'s `cancelled` outcome, never `failed`. `404` if it already finished or never existed - idempotent, not a silent no-op |
| `POST /v1/runs/{id}/steer` | send a follow-up message into a run's live process. `400` if the runner has no steering path - Codex, cursor-agent and goose today - `404` if the run is unknown or already finished |
| `POST /v1/runs/{id}/rerun` | same sealed contract, fresh run, fresh workspace; managers and delegated workers retain their pinned cell authority, and a worker reacquires that cell's shared cap; legacy records without a pinned definition fail closed; `404` on an unknown run |
| `POST /v1/runs/{id}/rescope` | a new run with a changed `goal`. `400` if `goal` is missing or unchanged from the original - that is `rerun`, not `rescope` |
| `GET`/`PUT /v1/ui-state/{key}` | an opaque blob farseer never parses, so a canvas survives a restart. `413` above 1 MiB |
| `GET /v1/analytics/{cost,intervention,rework,lessons}` | the four questions from [11 analytics questions](.scratch/farseer/issues/11-analytics-questions.md) |
| `/v1/mcp` | the streamable-HTTP MCP face nested into this router and guard; all three tools - `read_memory`, `write_memory`, and `delegate_to_worker` - derive identity from an active manager capability, and no raw event append exists because "an agent that can forge events can rewrite its own history" |

Every request must arrive on a loopback `Host`.
Operator routes require the process-wide bearer; `/v1/mcp` additionally accepts an active manager's per-run bearer, which is invalid everywhere else.
A generated manager config contains only the per-run bearer and never discloses the operator token.
A cross-site `Origin` is refused before the token is even looked at, because [16 local API surface](.scratch/farseer/issues/16-local-api-surface.md) found that a token alone does not stop DNS rebinding - the browser attaches it for the attacker.

### What is not built yet

- **Cross-cell delegation and non-Claude manager MCP wiring.** A Claude Code manager can delegate to a `kind = "worker"` roster entry and receive its terminal text in the same turn.
  `kind = "cell"` calls remain open, and Codex, cursor-agent, and Goose managers still execute their goal directly because no verified live MCP-config path exists for those CLIs.
- **Pre-spend enforcement for bounded native-runner budgets.** Task-root and per-worker caps narrow and draw down as `23 prototype loose ends` requires, but every bounded dimension fails closed before spawn today.
  Claude Code 2.1.233's `--max-budget-usd` exceeded a one-micro-dollar cap by more than five orders of magnitude before reporting `budget_exhausted`, while the other runners report only after spending.
- Gated actions.
- **Third-party MCP clients.** The manager process is now an MCP client of farseer's own server, but reaching arbitrary third-party MCP tool servers is still the `M0 -.->|MCP| TOOL` edge on the map above and is not implemented.
- **The ACP server adapter** and the A2A endpoint, both decided and both later.
- **The UI.** Backend support exists - `GET`/`PUT /v1/ui-state/{key}` per `24`, and `07` constrains the attach surface to a rendered event stream over one run - but "UI shape" itself is still fog on [the map](.scratch/farseer/map.md), not a closed ticket: manager chat, fleet view, board and graph explorer are options, not a decision. It waits on a `/wayfinder` grilling-and-prototype session, HITL, not something to design and build unilaterally.
- **A fifth runner, `pi` (badlogic/pi-mono).** Installed on the dev machine but has no provider credentials configured (`pi auth check` answers `credentials_not_configured`) - configuring one is an operator decision, so this stays a documented gap rather than a guess.

## The spikes

Three Rust programs that answered the scary platform questions before any design depended on the answers.
Each is reproducible and has its own README.

| Spike | Question | Answer |
| --- | --- | --- |
| [`jobspike`](.scratch/farseer/spikes/jobspike/README.md) | Does a Win32 Job Object reap a real harness process tree? | **Yes.** A five-deep tree reaps in **300-400µs** with zero survivors, where killing the root alone leaves **five of six alive indefinitely**. |
| [`wsspike`](.scratch/farseer/spikes/wsspike/README.md) | Can a workspace be destroyed under a running dev server? | **Yes, 60/60 supervised cycles**, p50 2.5ms. Every blocked attempt was the worker's own cwd, not the watcher or Defender. |
| [`storebench`](.scratch/farseer/spikes/storebench/README.md) | SQLite or an embedded graph engine? | **SQLite, not close.** At 100x the target (2M events, 291MB): cursor scan **p99 425µs**, recursive CTE over the rework chain **790ms**. |

Run one:

```bash
cargo run --release --manifest-path .scratch/farseer/spikes/jobspike/Cargo.toml -- job
```

Toolchain is `x86_64-pc-windows-msvc`, rustup stable, decided in [19 rust toolchain](.scratch/farseer/issues/19-rust-toolchain.md).

## Where the decisions live

[`.scratch/farseer/map.md`](.scratch/farseer/map.md) is the index.
It gists every closed ticket in one line and links to the ticket that holds the detail.

A decision lives in exactly one place: its ticket.
Corrections are recorded on the corrected ticket as well as the map, so nobody reads a stale version.

Start with [01 cell primitive](.scratch/farseer/issues/01-cell-primitive.md), then [14 vocabulary lock](.scratch/farseer/issues/14-vocabulary-lock.md) for the glossary every later document uses.

## Not in scope for v1

A plugin ABI, multi-human collaboration, cloud execution of workers, mac and Linux, a token-level model router, and autonomous cell generation by cell #0.
The map's **Out of scope** section records each with the reasoning.
