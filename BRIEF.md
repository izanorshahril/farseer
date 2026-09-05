# Farseer: Brief and Research Report

Status: draft 2, research only. **The decisions this document deferred are now locked** - see [.scratch/farseer/map.md](.scratch/farseer/map.md) for the route and the 27 closed tickets.
Date: 2026-08-18.
Scope of this revision: layered priorities (orchestration core, record layer, improvements), plus research on memory / knowledge base / session logs / graph engineering and their Windows failure modes.

**Amendment, 2026-08-18.** The operator's corporate framing (owner appoints a CEO, two PAs track state and knowledge, each product is its own team of agents that farseer talks to manager-to-manager) introduces a primitive this document lacks: the **cell**.
See `ARCHITECTURE.md` for the cell model, the three-protocol boundary, and the revised runtime shape.
See `.scratch/farseer/map.md` for the decision route.
Two corrections that apply to the text below:

- "ACP" throughout means **Zed's Agent Client Protocol**. IBM ships a different specification with the same acronym. Always qualify it.
- Section 13 describes a coding orchestrator. The cell model in `ARCHITECTURE.md` supersedes it as the top-level shape, and section 13's control-plane rules survive unchanged underneath.

## 1. Problem statement

`D:\Dev\firstmate` gets the concept right and the substrate wrong.

Its concept: one manager agent owns the layer between operator intent and the agents that do the work, so the operator context-switches once instead of N times.
Its substrate: ~140 bash scripts driving tmux / herdr / wezterm / treehouse / gh, an "agent distro" that assumes a POSIX box with cheap process control and no mandatory file locks.

On Windows that substrate fails in ways that are not bugs but platform mismatches:

- Locked terminals and panes, because the orchestration layer *is* a terminal multiplexer and the agent lives inside a PTY it does not fully control.
- Folder permission and cleanup failures, because git worktrees plus `node_modules` plus running dev servers cannot be removed while any handle is open.
- Integration sprawl, because every capability is another external binary that must exist, be on PATH, and behave identically across Windows, WSL, and mac.

Goal: rebuild the concept on a substrate native to Windows first, portable to mac/Linux later, depending on process contracts rather than terminal scraping.

## 2. Layers and priority

Three layers. All ship in the same product, but the core is what makes it exist.

### Layer 1: orchestration (the core, non-negotiable)

- The operator interacts with **one** agent: the manager.
- The manager **always delegates**. It never edits project code, however trivial the change.
- The manager **reviews completion**, not process.
- A worker can be a **reviewer** of another worker's output. Review is a task shape, not a special subsystem.
- The operator can **attach to any worker, unhindered**, watch it live, talk to it, take over, and detach. Attaching must not block, pause, or corrupt the worker, and must not require the manager's permission.
- Workers are short-lived, isolated, supervised, and disposable.
- Everything survives a restart of anything, including the manager.

### Layer 2: the record (platform, shared across all harnesses)

The thing that makes farseer more than a launcher, and the thing no existing tool does well.

- **Session logs**: every worker run captured as a normalized, append-only event stream, harness-agnostic.
- **Memory**: what the manager and workers know, scoped (global / project / task), curated, decaying, auditable.
- **Knowledge base**: durable notes, decisions, conventions, gotchas, retrievable by any harness.
- **Graph**: entities and relations over projects, tasks, runs, files, symbols, issues, artifacts, decisions and errors, so history is explorable and analyzable rather than a pile of transcripts.
- **Shared across harnesses**: Claude Code, Codex, Antigravity, Pi all read and write the same record. The record is the moat; harnesses are interchangeable.
- Two consumers: the **system** (self-improvement, routing hints, avoiding known dead ends) and the **operator** (exploration, insight, analytics).

### Layer 3: improvements (ship together, not first)

- **Kanban board**: a projection of task state. Optional as a mental model, useful as a UI, not the source of truth.
- **Auto-router**: seat and model selection from quota, capability, cost. Optional in that the system must work with one hard-coded seat.

Design rule that follows: Layer 3 must be deletable without breaking Layer 1, and Layer 1 must not need the board or the router to function.

## 3. Interaction model

```
operator  <-->  manager (plan, contract, delegate, triage, escalate)
   |                 |
   |                 +--> worker A (implement)   --> report + diff + events
   |                 +--> worker B (review A)    --> verdict + findings
   |                 +--> worker C (scout/learn) --> standalone report
   |
   +-- attach/observe any worker directly (read-only tail, or interactive takeover)
```

Rules:

- Manager to operator: outcomes, consequences, decisions. Never mechanics.
- Manager to worker: an explicit contract. Never a vibe.
- Worker to manager: a structured report plus artifacts. Never a chat transcript to be re-read.
- Operator to worker: allowed at any time, logged as an event, and the manager is told it happened so it does not plan against stale reality.
- Escalations are the only unprompted interruption of the operator.

Design tension worth naming now: **an operator side-channel into a worker mutates state the manager believes it owns.** Existing tools either forbid this or ignore the consequence. Proposal: an operator intervention closes the worker's current contract and forces a re-report, so the manager re-reads reality from the record instead of trusting its plan.

## 4. What to keep from firstmate

Keep the concepts:

- Single-interface command layer.
- Manager reads projects but never writes them.
- Explicit task contract before work starts (goal, delivery path, autonomy grant, validation).
- Authority is explicit and never inferred; merges and destructive acts need a human word.
- Durability: reconcile from disk, restart is a non-event.
- Deterministic scripts own mechanics, agents own judgment.
- Flat command structure, capped depth.
- Vendor independence; adapters earn trust through verification.
- Quota and model choices inspectable and operator-owned; never a silent intelligence downgrade.

Discard the mechanics:

- tmux / herdr / wezterm / zellij / orca backend matrix as the execution substrate.
- Bash as the implementation language for control logic.
- `gh` and treehouse as required dependencies.
- Terminal-pane state inference as the primary way to know what an agent is doing.
- Harness-specific hooks as the only supervision signal.

## 5. Landscape: orchestrators

### 5.1 firstmate (kunchenguid/firstmate, MIT)

Portable directory of instructions, skills, scripts, conventions that turns a terminal agent into an orchestrator spawning crewmates in tmux windows and disposable git worktrees.
Backends: tmux (reference), herdr, cmux, zellij, orca.
Best available articulation of the single-captain model.
Limitation: mac-shaped, shell-heavy, integration-heavy; orchestration fidelity is bounded by multiplexer fidelity.

### 5.2 Vibe Kanban (BloopAI, open source, community-maintained since Bloop shut down 10 Apr 2026)

Closest architectural precedent.
Rust backend (Axum, SQLx, Tokio), React frontend, crates split server / services / db / executors.
Core idea worth stealing verbatim: **git owns code state, SQLite owns workflow state.**
Executor abstraction normalizes Claude Code, Codex, Amp, Cursor CLI, Gemini and others.
Worktree per task, kanban board, visual diff review, cross-platform.
Known Windows-class defect: SQLite locking causing "Loading History" hang and OOM on WSL2 (issue #1941).
Gap: the human does the planning. No persistent manager agent, no record layer, no quota routing.

### 5.3 Conductor / Nimbalyst (ex-Crystal) / Superset

Conductor is macOS only. Nimbalyst is the renamed Crystal, a broader workspace product. Superset markets itself as "agent workspace" versus firstmate's "DIY agent distro".
Pattern across all three: session viewers with worktree isolation. The context switch stays with the human.

### 5.4 Omnara

"PagerDuty for AI agents." Mobile and web command center that decouples the agent from the local terminal: monitor progress, review changes, approve steps, intervene in live sessions running locally or in the cloud.
Directly relevant to Layer 1's attach requirement: proof that remote observe-and-intervene over a local agent is a shipped pattern, and that the intervention channel can be the product.

### 5.5 OpenClaw ecosystem (claw-orchestrator, claworc)

Runs Claude Code, Codex, Gemini, Cursor Agent and custom CLIs as one unified runtime, standalone or as a plugin.
Structural decomposition worth copying: an agent is **workspace** (files it reads and writes, including AGENTS.md, skills, notes, memory-like material) plus **agentDir** (per-agent state: auth profiles, config) plus **sessions** (isolated conversation history and routing state).
Also ships ACP session support for external harnesses.

### 5.6 Buzz (Block, Apache-2.0)

Shared workspace where humans and agents are co-participants; Nostr protocol; channels, threads, DMs, voice, repos, workflows.
Agents get cryptographic identities and scoped permissions.
Harness-agnostic via ACP, already driving Goose, Codex, Claude Code.
Confirms ACP as the interop bet, and per-agent identity plus scoped permission as a real design axis.
Not our shape: multi-human, chat-first, no single-manager planning layer.

### 5.7 Herdr

Rust single binary, agent-aware multiplexer, tmux rebuilt with awareness of what the agent in each pane is doing.
Good tool, wrong layer. Agent state should come from the harness event stream, not from a terminal inferring it.

### 5.8 headless-cli (RobertTLange)

Prior art for a unified headless wrapper across harnesses. Default fallback order codex, claude, pi, opencode, gemini, antigravity, cursor.
Useful as a reference list of invocation contracts, not as a dependency.

## 6. Landscape: worker control interfaces

**ACP (Agent Client Protocol)**: JSON-RPC 2.0, the "LSP for coding agents".
Antigravity/Gemini supports it natively (`--acp`), Claude Code via the `claude-agent-acp` adapter, Codex via `codex-acp`.
Consumed by Zed, VS Code extensions, OpenClaw, Buzz.
Strongest candidate for a stable worker-control interface that is not PTY scraping.

**Headless CLI contracts** (fallback and complement):

- Claude Code: `--output-format text|json|stream-json`, stdin/stdout piping, `--allowedTools`, `--permission-mode`, `--bare` for explicit credentials. Claude Agent SDK (TS/Python) exposes the same loop programmatically. Sessions are resumable, and native **agent view** plus `/attach <session-id>` already implements dispatch-to-background then attach; docs note headless `--print` runs do not show up in agent view the same way and need external aggregation.
- Codex: `codex exec --json` emits newline-delimited JSON events plus a real exit code.
- Antigravity CLI (`agy`): successor to Gemini CLI (shut down for free/Pro/Ultra on 18 Jun 2026), scriptable, auto-approve modes.
- Pi: MIT, sub-1k-token system prompt, lazy skills, built to be forked. Has an **RPC mode** for headless integration plus a `pi-interactive-shell` package, and `pi-antigravity` adds native streaming, model routing and quota diagnostics.

Implication for the attach requirement, and the most important Layer 1 finding: at least three of four target harnesses already expose a structured, resumable session concept with an event stream. Attach should be implemented as "subscribe to the normalized event stream, optionally inject input", with a PTY attached only as a last-resort human view. That is what makes attach non-blocking, and it is exactly what firstmate could not do through a multiplexer.

## 7. Landscape: memory and knowledge base

Two camps in 2026: temporal knowledge graphs, and memory operating systems where the agent edits its own memory.

| System | Shape | License / host | Fit |
|---|---|---|---|
| Mem0 | Dual store: vector for semantic search plus optional graph for entities | managed API plus OSS | Breadth, drop-in personalization. LongMemEval 49.0% |
| Zep / Graphiti | Graph-native, **time as a first-class dimension**, bi-temporal edges | OSS engine plus hosted | Depth, reasoning about how facts change. LongMemEval 63 |
| Cognee | ECL pipeline (Extract, Cognify, Load), hybrid graph plus vector; graph via Neo4j or Kuzu, vector via pgvector or LanceDB | OSS, local-first | Relationship-rich reasoning over docs plus conversations. Heaviest to stand up and keep healthy |
| Letta / MemOS | Memory OS, agent edits its own memory blocks | OSS | Self-editing, agent-authored curation |
| Basic Memory | Markdown notes, wikilinks, observations, knowledge-graph context, MCP server | AGPL, local-first, optional paid cloud sync | Human-readable, git-friendly, inspectable |
| Memorix | Cross-agent memory bridge, knowledge graph, workspace sync, auto-memory hooks | MCP server | Explicitly targets Windsurf, Cursor, Claude Code, Codex, Copilot |
| claude-mem | Claude Code memory tool | OSS | Open discussion #1329 asks for it to become a shared backend for Codex-style agents, so the portability gap is acknowledged upstream |
| MemPalace | Cross-tool memory | OSS | Portability is its stated thesis |

Key architectural takeaway: **MCP decouples the memory store from the agent runtime.** One memory server can serve Codex, Claude Code, Cursor and others through the same protocol. That is the shape Layer 2 needs, and it means farseer's record should expose an MCP face, not only an internal API.

Counter-consideration: an MCP memory server is *pull* only, so the agent must choose to query it. For the manager that is acceptable. For workers the reliable path is **injection at contract time** (the relevant slice of the record is written into the worker's prompt and workspace), with MCP as the on-demand escape hatch.

Risk from the literature: memory hygiene. Agents can store bad lessons that mislead future tasks; mitigations are versioned memories, scoring and decay. There is a 2026 paper specifically on **memory confabulation in reflexive agents** ("Honest Lying"), which is the failure mode to design against: a plausible but false lesson, stored once, trusted forever.

## 8. Landscape: graph and store engines

**The embedded graph situation changed recently and matters a lot.**

- **Kuzu** was the obvious embedded-graph pick. Its GitHub repo was archived without warning on 10 Oct 2025; a European Commission filing later confirmed **Apple acquired the team**. Do not start new work on it.
- **LadybugDB** is the live successor fork (MIT, "DuckDB for graphs"), aiming at a graph lakehouse that interoperates with DuckDB storage, reads and writes Arrow/Parquet, and connects to object stores. Python API is intentionally Kuzu-compatible; migration is EXPORT then IMPORT. Graphiti has an open issue (#1509) to add a LadybugDB driver.
- **Vela-Engineering/kuzu** is a separate fork adding concurrent multi-writer support for multi-agent use, and claims 374x faster path queries than Neo4j (0.009s vs 3.22s). Relevant because original Kuzu and most forks are **single-writer**, which is a direct problem for a fleet of concurrent workers writing to a shared record.
- **FalkorDB / FalkorDBLite** is embeddable and Python-friendly, supports multiple separate graphs in one database, but is source-available, not MIT.
- **Neo4j** is the safe, boring, well-documented option, but it is a JVM server process, which contradicts "no required external services" and pays 3-4s per path query in the cited multi-agent comparison.

Vector and analytics options that are safe on Windows without build tools:

- **sqlite-vec**: SQLite extension, vectors as BLOBs in ordinary tables, KNN, multiple distance metrics, SIMD, ~30MB default memory, ships for Windows / Linux / mac / iOS / Android. Lowest-risk choice.
- **LanceDB**: embedded, in-process, ANN plus FTS plus SQL; IVF-PQ so approximate and compressed, tunable recall.
- **DuckDB (+ VSS, and now a Lance integration)**: best answer for the *analytics* half of the requirement, since operator-facing insight is columnar aggregation, not graph traversal.

Practical read: **one graph engine is probably the wrong answer.** The requirement splits three ways, and each has a different best tool:

1. Truth and workflow state: SQLite (WAL), boring and native.
2. Relationship traversal for agent reasoning: embedded graph (LadybugDB), or, at small scale, recursive CTEs over SQLite edge tables with zero extra dependency.
3. Operator analytics and exploration: DuckDB over exported Parquet, which also gives cheap historical analysis without touching the live store.

Option (2) deserves a real decision point: at farseer's likely data volume (thousands of tasks, tens of thousands of events, not billions of edges) a plain edge table in SQLite plus recursive CTEs may cover every query the manager actually asks, and one fewer engine is worth a lot on Windows.

## 9. Landscape: session logs, traces, observability

**Existing harness log formats** (what we can ingest, and why we must normalize):

- Claude Code writes every session to `~/.claude/projects/<munged-path>/<session-id>.jsonl`. Each line is a typed record chained by `parentUuid`: user prompts, assistant responses with content blocks (text, tool calls, thinking), tool results, system prompts, summaries, git snapshots.
  Critical caveat from the docs: **the entry format is internal and changes between versions**, so direct parsers break on releases. The sanctioned paths are `/export` and the script interfaces.
  Community tooling proves the appetite: `simonw/claude-code-transcripts`, `daaain/claude-code-log`, claude-devtools (cross-session search, per-tool renderers).
- Codex: `codex exec --json` newline-delimited events, the cleanest machine contract of the set.
- Antigravity, Pi: their own event streams and RPC modes.

Design consequence: farseer must define its **own normalized event schema** and treat harness transcripts as an untrusted input format with per-version adapters. Never let the record's schema be a downstream copy of a vendor's internal JSONL.

**Trace standards to align with rather than reinvent:**

- **OpenTelemetry GenAI semantic conventions**, as of spec v1.41, define agent, workflow, tool and model spans plus required latency and token-usage metrics. This is the emerging standard; farseer's event schema should be mappable onto it.
- **Langfuse**: MIT (excluding an `ee` folder), self-hostable via Docker Compose, Kubernetes or Terraform; tracing, prompt management, datasets, evals, dashboards.
- **Arize Phoenix / OpenInference**: OpenInference is a set of OTel-based instrumentation SDKs with 40+ framework integrations.
- OpenObserve is fully OTel-native over OTLP.

Recommendation: emit OTel-shaped spans as an *optional export*, keep the internal record as the primary. That gets Langfuse or Phoenix as a free power-user dashboard without making them a dependency, and it means the record is not locked to one vendor's viewer.

## 10. Graph engineering: proposed practice

The user's framing (record everything to db plus graph, for self-improvement and operator analytics) has a direct precedent in the literature.

**"The Log is the Agent: Event-Sourced Reactive Graphs for Auditable, Forkable Agentic Systems"** (arXiv 2605.21997, `yoheinakajima/activegraph`, Apache-2.0) states the pattern exactly: an **append-only event log is the source of truth**, and **replay folds the log into the graph**, which is a projection of typed objects and relations. Claimed properties: auditable lineage, deterministic replay, and cheap counterfactual forks even though the log contains nondeterministic model calls.

That is the design. Adopt it:

- `event` is append-only and immutable. It is the truth.
- The graph, the board, memory, and every dashboard are **projections**, rebuildable by replay. A corrupt projection is never a data loss event.
- **Bitemporality** from Graphiti's model: record both when a fact was true in the world and when the system learned it. Without this, "why did the agent believe that then" is unanswerable, and that question is the whole point of the record.
- **Forkability**: replaying a task's log with one decision changed is how you evaluate a routing or prompt change against real history rather than a synthetic benchmark.

Proposed entity types for the graph projection (drawing on the repository-knowledge-graph literature, where Users / Commits / Issues / Files with multi-hop analysis is the established core, and KGCompass-style work shows the win comes from linking *repository artifacts* like issues and PRs to code, not just code structure):

```
Project, Task, Contract, Run, Seat, Harness, Model
File, Symbol, Test, Commit, Branch, PR/MR, Issue
Artifact (diff, report, log), Decision, Escalation
Error, Failure mode, Lesson, Convention, Note
Operator intervention
```

Edge examples that make the record earn its keep:

```
Run       --touched-->      File
Run       --introduced-->   Error
Error     --resolved_by-->  Run
Lesson    --derived_from--> Run(s)
Lesson    --applies_to-->   Project | File | Harness
Task      --blocked_by-->   Decision
Task      --reviewed_by-->  Run
Seat      --strained_at-->  Time window
Convention--violated_by-->  Run
```

Queries this makes cheap, and which are the actual product value:

- Which files repeatedly break, and under which harness or model.
- Which lessons actually reduced failure rate after being adopted (lesson efficacy, which is how you fight confabulated memory).
- Cost and token spend per project, per task shape, per seat, over time.
- Where the manager escalated versus where it should have.
- Which contracts were ambiguous, measured by rework rate.
- Dead ends already explored, so a future worker is told before repeating them.

**Self-improvement grounding from the research**, so this is not a vibe:

- **Reflexion**: persistent natural-language reflections appended across attempts; lessons compound over repeated tasks. This maps onto `Lesson --derived_from--> Run`.
- **ExpeL** (Zhao et al., AAAI 2024): agent autonomously gathers experience across tasks, derives natural-language insights from successes and failures, and reuses successful experiences as in-context examples, **with no parameter updates**, so it works with closed-source API models. This is the exact mechanism available to farseer.
- **Voyager**'s skill library (2023): store successful solutions as named reusable functions and check the library before writing new code. Maps onto reusable task templates and scripts, which fits firstmate's "scripts own mechanics" principle.
- **Memory hygiene** is mandatory: versioned lessons, efficacy scoring, decay, and a promotion gate. Proposal: a lesson starts as `candidate`, requires operator or reviewer-worker confirmation to become `active`, and is auto-demoted when tasks that applied it fail more often than tasks that did not.

## 11. Multi-agent risk data

Worth recording because it argues for the flat, contract-first design rather than against the product:

- **Coordination failures are 36.94% of all failures** across AutoGen, CrewAI and LangGraph in the cited analysis. The number-one mode is infinite handoff loops: A to B to C back to A, with nobody owning the task.
- **Context loss compounds per handoff.** Full context forwarding is expensive and eventually overflows; summarization cuts 70-90% of tokens but is lossy and adds 500ms to 1.5s per handoff.
- **Multi-agent systems use roughly 15x the tokens of chat interaction.**
- Orchestrator-worker is the pattern most production systems start with, because it gives clear accountability, debuggable control flow and predictable cost.

Direct design consequences:

1. Depth is capped. Manager to worker, worker to reviewer-worker, and that is it. No worker spawns a worker without an explicit grant.
2. One owner per task, always. Handoff transfers ownership explicitly; there is no ambient re-planning.
3. Handoff is a **contract plus a record pointer**, not a context dump. The worker rehydrates from the record, which is exactly what the record is for. This is where Layer 2 pays for Layer 1.
4. Token accounting is first-class, per run and per task, or the 15x will not be noticed until the bill.

## 12. Windows failure-mode catalog

The list the app must be designed against, not patched for.

**Process control**

- No POSIX signals. `pty.kill(signal)` throws on Windows in node-pty.
- Killing a parent leaves orphans. Correct pattern is a **Win32 Job Object** with kill-on-job-close so the tree dies with the supervisor; `taskkill /pid <pid> /T /F` is the after-the-fact fallback.
- ConPTY is used on build >= 18309 and has lifecycle quirks; there are open node-pty issues about unkillable pty processes on Windows.
- Ctrl-C into a child console is a per-console-group operation, not a signal.
- Node CLIs resolve as `.cmd` shims (`claude.cmd`, `codex.cmd`), which need `shell: true`, `cmd /c`, cross-spawn, or explicit resolution.

**MCP transport (new, and directly hits Layer 2)**

- Stdio MCP servers whose command is bare `npx` fail with `spawn npx ENOENT` / `EINVAL` on Windows, because `npx` is a `.cmd` script and some clients cannot spawn batch files without the shell option.
- This is the single most common MCP setup error, tracked across Claude Code (#58510, where a plugin-shipped `npx` command regressed because an earlier LSP fix missed the MCP spawn path), GitHub Copilot CLI (#3576), VS Code (#299595), plus Cursor and Windsurf.
- Consequence: if farseer exposes its record over MCP, it must ship as a **native executable with an absolute path**, never as `npx some-package`. This is a strong argument for a single compiled binary.

**Filesystem**

- Mandatory file locking. Any open handle (dev server, watcher, editor, antivirus, indexer) blocks delete and rename.
- `git worktree remove` fails on Windows when a node process holds locks (claude-code #41740).
- `unlink` returns EPERM on `.git/*.lock` in sandboxed contexts (#61343), leaving stale locks that wedge git.
- MAX_PATH 260. `core.longpaths true` helps git but not every consumer; deep `node_modules` / pnpm trees still break tools that do not use `\\?\` paths.
- Case-insensitive, case-preserving FS: case-only renames and case-colliding files both misbehave.
- CRLF and `core.autocrlf` interact badly with diff review and with agents writing files.
- Reserved names (`con`, `aux`, `nul`, `prn`), trailing dots and spaces are illegal in filenames, and an agent will eventually generate one.
- Defender real-time scanning adds latency and transient sharing violations on freshly created worktrees.

**Store durability**

- SQLite over the WSL 9p / DrvFs boundary is a documented locking and corruption hazard (vibe-kanban #1941: "Loading History" hang plus OOM).
- SQLite is single-writer. With a fleet of concurrent workers all emitting events, WAL plus a single writer task in-process is required; concurrent processes writing the same DB file over any network or virtualized path is not an option.
- Most embedded graph engines (original Kuzu and its non-Vela forks) are also single-writer. Same conclusion: one owning process, everything else goes through it.

**WSL boundary**

- `\\wsl$\` and `/mnt/d` crossings are slow, break inotify, and break git file-mode assumptions.
- Two credential stores, two PATHs, two node installs, two config trees.
- Proposed hard rule: **never cross the boundary for state or repos.** Fully native or fully inside WSL, chosen once per install.

**Isolation and sandbox**

- Claude Code `/sandbox` uses Seatbelt on mac and bubblewrap on Linux. **Native Windows is unsupported** (claude-code #46740).
- Without a sandbox, deny rules only bind built-in tools: `Bash(cat ~/.ssh/id_rsa)` bypasses `Read(~/.ssh/**)`.
- The current official mitigation is "standardize on WSL2", which is the UX being rejected.
- Emerging: **Microsoft Execution Containers (MXC)**, announced at Build 2026, early preview, dispatching to AppContainer and the new BaseContainer API on Windows plus WSL containers. This is the v2 upgrade path to watch, not a v1 dependency.

**Misc**

- No systemd. Sleep and hibernate must be handled. Use a tray or user-session process, or Task Scheduler.
- Python-based memory or graph stacks (Cognee, Graphiti) pull native wheels; on Windows without build tools this is a recurring install failure, and an argument for keeping the record layer in the main binary rather than adopting a Python framework wholesale.

## 13. Proposed architecture

### 13.1 Shape

Local-first single binary plus local web UI. No required external services. No required external binaries beyond the agent CLIs the operator already has.

```
farseer/
  core (Rust)              orchestrator daemon, single binary
    api                    HTTP + SSE/WebSocket for UI and CLI
    mcp                    record exposed to any harness (native exe, absolute path)
    store                  SQLite (WAL) event log = truth; git = code state
    projections            board, graph, memory, KB, analytics (all rebuildable by replay)
    scheduler              task queue, concurrency caps, wake events
    supervisor             process lifecycle, Job Objects, health, timeouts
    adapters               per-harness driver (ACP first, headless JSON fallback)
    router                 seat/quota accounting, model selection (Layer 3)
    workspace              isolation strategy per project
    manager                the planning agent session, driven like any other harness
  ui (web)                 manager chat, worker attach, diff review, board, graph explorer
  cli (farseer.exe)        non-interactive, JSON out, explicit exit codes
```

Why Rust: single self-contained binary (which also solves the MCP `npx` problem), no runtime install, direct Win32 access (Job Objects, `\\?\` paths, `CreateProcess` flags), and vibe-kanban proves Axum plus SQLx plus Tokio works for exactly this job.
Alternative considered: Bun/TS single-file executable. Faster to build, but node-pty and Win32 process-group work is precisely where TS is weakest, and the whole thesis is that process control is the thing firstmate got wrong.

### 13.2 Control plane rules

1. **No PTY as control channel.** Adapters speak ACP over stdio, or newline-delimited JSON over stdio. A PTY is attached only when the operator explicitly asks to watch or intervene, and it is a view.
2. **Every child in a Job Object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Daemon death kills the fleet: no orphans, no locked folders.
3. **Nothing runs from inside a workspace we intend to delete.** No cwd, no dev server, no watcher. Cleanup is a supervised state machine: stop children, wait for handle release, retry with backoff, then quarantine and report. Never a blind recursive delete that half-fails.
4. **Long paths everywhere** (`\\?\`), workspaces created at a short root such as `D:\fw\<hash>`.
5. **One writer to the store**, in-process, WAL, native path only.
6. **The event log is truth; everything else is a projection** and can be dropped and rebuilt.
7. **Every worker run is fully reconstructible from disk**: contract, event stream, artifacts, exit status, cost.

### 13.3 Workspace isolation strategies

Per-project config, because one strategy cannot fit all.

| Strategy | Mechanism | Good for | Windows risk |
|---|---|---|---|
| `worktree` | `git worktree add` at short root, `.fwinclude` copy of gitignored deps | most repos | lock-on-remove, dep install cost |
| `clone` | local clone, shared objects via alternates | repos hostile to worktrees | disk |
| `snapshot` | copy-on-write via Dev Drive / ReFS block clone if available, else robocopy | huge non-git trees | availability |
| `in-place` | serialized single-writer lock on the real repo | small edits, docs, ops tasks | no parallelism |

Precedent: the `.worktreeinclude` pattern (gitignore syntax listing gitignored paths to copy, so tracked files are never duplicated) is already used by several tools for the `node_modules` problem.
Dev Drive (ReFS) on Windows 11 is worth measuring: block cloning plus reduced Defender surface could make `snapshot` the fastest option and the single biggest perf lever.

### 13.4 Manager agent

The manager is just another adapter session with a distinct contract:

- Long-lived but **idle-cheap**: sleeps on an event queue, wakes on worker completion, board change, operator message, or schedule. Idle costs zero tokens.
- Read-only on all projects, enforced by adapter allowlist plus a read-only workspace where the strategy allows.
- Owns intake, decomposition, contract authoring, routing hints, report triage, escalation, and record curation.
- Writes plans, contracts and decisions as **structured records**, not prose in a chat log.
- Context is a rolling window rehydrated from the record on every wake, so restart is a non-event and context never grows unboundedly.
- Every manager claim about task state must be a store read, never recall.

### 13.5 Router (Layer 3)

Two distinct problems. Do not conflate them.

**A. Seat routing (the actual need).** Which harness *account* runs this task, given subscription quota.
Inputs: rolling-window usage estimates per seat, observed 429 and rate-limit events, weekly cap state, task class.
Behavior: prefer an unstrained seat; queue rather than silently degrade intelligence; demotion requires standing permission.
Reality: subscription quota is not exposed as a clean API (Claude Code enforces a 5-hour rolling window plus weekly caps per account), so this is inference from headers, errors and our own token accounting. `pi-antigravity` ships quota diagnostics and OmniRoute does quota-share enforcement, so it is tractable but heuristic.

**B. Token routing (optional, later).** Per-request model selection under a worker. Delegate, do not build.
**NVIDIA NeMo Switchyard** (open source, announced 11 Aug 2026) is the best fit because its reference server speaks OpenAI, Anthropic and Responses formats, so it can sit under Claude Code and Codex without either knowing. It builds on RouteLLM and adds session-affinity classification, a stage router that reads recent tool activity, and an escalation router that starts cheap and promotes on sustained difficulty. LangChain's eval: 74% cost cut versus Opus 4.8 alone, accuracy 86.0% to 80.0%, which is the honest tradeoff to show the operator.
LiteLLM is the pragmatic alternative with a documented Claude Code Max subscription path.

Farseer owns (A) and treats (B) as a pluggable base URL per seat.

### 13.6 Data model sketch

```
-- truth
event(id, ts_wall, ts_ingest, actor, subject_type, subject_id, kind, payload_json)
   append-only, immutable, bitemporal (world time vs learned time)

-- projections (rebuildable)
project(id, name, path, vcs, delivery_policy, autonomy_grant, isolation_strategy, status)
task(id, project_id, title, contract_json, status, lane, priority, parent_id, owner_run_id)
run(id, task_id, seat_id, harness, model, workspace_path, pid, job_handle,
    started, ended, exit_code, tokens_in, tokens_out, cost, verdict)
seat(id, harness, account_label, auth_mode, quota_policy, window_state_json, health)
decision(id, task_id, question, options_json, answered_by, answer, ts)
artifact(id, run_id, kind, path, sha256, bytes)
node(id, type, key, props_json)                 -- graph projection
edge(id, src_node, dst_node, type, props_json, valid_from, valid_to, learned_at)
memory(id, scope, kind, body, status, score, superseded_by, created, last_used)
   status: candidate | active | demoted | retired
lesson_application(id, memory_id, run_id, outcome)   -- efficacy measurement
```

`task.status` drives the kanban lanes. The board is a view over `task`, which is itself a view over `event`.

### 13.7 UI

Local web UI in the browser or a thin webview. Not a TUI, because the terminal is the thing that hurts today.

- **Manager chat**: one conversation, the only place the operator talks to an agent.
- **Fleet**: running workers, live status, seat quota gauges, kill switch, and one-click attach.
- **Worker attach**: live event tail, injectable input, takeover and detach.
- **Task detail**: contract, event timeline, diff review, approve / reject / merge.
- **Escalations**: inbox of decisions only a human can make.
- **Board** (Layer 3): swimlanes by project or status.
- **Explorer** (Layer 2): graph and analytics over history, cost, failure clusters, lesson efficacy.

## 14. Milestones

- **M0 spike (throwaway).** Prove the four scary Windows things: Job Object kill-on-close with a real Claude Code child; worktree create/destroy loop under a running dev server; `codex exec --json` and `claude --output-format stream-json` parsed end to end; and non-blocking attach to a running worker with input injection. Nothing else.
- **M1 core, one project, one worker.** Event log, supervisor, one adapter, CLI only, no manager. Task in, diff out, full event trail, attach works.
- **M2 manager.** Planning agent, contracts, delegation, reviewer-worker task shape, escalations, wake queue. Proof: kill the daemon mid-run and recover with nothing lost.
- **M3 record.** Normalized session logs, memory with candidate/active promotion, knowledge base, graph projection, replay rebuild, MCP face so other harnesses can read it.
- **M4 improvements.** Kanban UI, seat registry plus quota heuristics, optional token router, analytics and graph explorer.
- **M5 portability.** mac and Linux, which should be a subtraction of Windows workarounds rather than an addition.

## 15. Risks and open technical unknowns

1. **Subscription quota is not observable.** Seat routing may reduce to heuristics plus reactive backoff. Needs a spike.
2. **ACP adapter maturity.** `claude-agent-acp` and `codex-acp` are third-party. If they lag, fall back to per-harness headless JSON, which means N adapters to maintain (vibe-kanban's `executors` crate shows the cost).
3. **Harness format churn.** Gemini CLI was killed on 18 Jun 2026 and replaced by Antigravity CLI. Claude Code's session JSONL is explicitly internal and version-unstable. Adapters need contract tests, and a harness is not trusted until it passes them.
4. **No Windows sandbox.** v1 cannot claim OS-level isolation. Isolation is workspace separation plus allowlists plus a review gate. Watch MXC.
5. **Worktree cleanup will still fail sometimes.** Design for quarantine plus a background reaper, not for success.
6. **Single-writer stores versus a concurrent fleet.** Both SQLite and most embedded graph engines are single-writer. The whole record must funnel through one owning process, which becomes a throughput and liveness concern under many workers.
7. **Kuzu's disappearance is a warning about the graph layer.** Any embedded graph dependency is young and may be acquired or abandoned. Mitigation: keep the graph a projection so the engine is swappable, and consider whether SQLite edge tables plus recursive CTEs are enough at this scale.
8. **Memory confabulation.** A stored-but-false lesson is worse than no memory. Promotion gates and efficacy scoring are load-bearing, not polish.
9. **Manager drift.** A long-lived planner that rehydrates from a store can still hallucinate board state. Every claim must be a read.
10. **Cost.** Multi-agent runs about 15x chat token usage, and an always-on planner is a token sink if it wakes on noise. Event batching, a no-op short-circuit, and per-task budgets are mandatory.
11. **Defender and AV latency** on fresh workspaces may dominate task startup. Measure; document exclusions and Dev Drive; never change AV config automatically.
12. **Record growth.** Full event capture over months of fleet activity gets large. Need retention tiers: hot events in SQLite, cold events exported to Parquet for DuckDB analytics.

## 16. Open questions for the operator

Answer whenever; none of these block M0.

**Core orchestration**

1. Is the operator-facing surface primarily the **manager chat** or the **fleet/board view**? Which do you open first in the morning?
2. When you attach to a worker, do you want **read-only tail by default** with an explicit "take over" action, or immediate interactive control?
3. After you intervene in a worker directly, should the manager (a) be told and re-plan, (b) treat that worker's contract as void and require a fresh report, or (c) be kept unaware unless the outcome changes?
4. Should the manager be allowed to spawn a worker that spawns a worker, ever? Or is depth hard-capped at two?
5. Is review always a separate worker, or may the manager review small diffs itself? (firstmate says the manager never does the work; review is arguably not work.)
6. Should farseer ever run unattended (wake on schedule or webhook, no operator present), or is it always operator-present?
7. Single machine only, or should workers eventually dispatch to a second box or a cloud runner? This decides whether workspace paths can be assumed local.
8. May the manager modify farseer itself (firstmate's self-evolving property), or is farseer strictly a tool that manages other projects?

**Record, memory, graph**

9. Who writes memory: the manager only, any worker, or an explicit curator task? My inclination is workers propose, manager promotes, operator can veto.
10. Should lessons require **explicit operator confirmation** before becoming active, or is reviewer-worker confirmation plus efficacy scoring enough?
11. Do you want memory to be **human-readable files in git** (Basic Memory style: markdown, wikilinks, diffable, reviewable) alongside the DB, or is the DB the only home? Files cost sync complexity but make the record inspectable and portable.
12. Should the record be **per-machine** or syncable across machines later? This changes ID strategy (UUIDv7 versus autoincrement) from day one, so it is cheap now and expensive later.
13. How much of a project's *code* graph do you want (symbols, call edges, tests-to-files)? That is a much larger ingestion job than the process graph (tasks, runs, errors, lessons), and needs language-specific parsing. Process graph first, code graph later?
14. Retention: keep every event forever, or tier and archive? Any privacy or secret-scrubbing requirement on stored transcripts (they will contain file contents and sometimes credentials)?
15. Should farseer's record be exposed over MCP so your *other* tools (Claude Code used directly, Codex, editors) can read and write it outside farseer? This is the "shared among all harnesses" goal taken literally.
16. What analytics questions do you actually want answered? The graph schema should be driven by your real questions, not by what is graphable. Three or four concrete examples would pin the design.

**Runtime and platform**

17. Native Windows only for v1, or must WSL2 workers be supported from day one? If both, may we forbid mixing (no repo on `\\wsl$`, no state across the boundary)?
18. Is a Dev Drive (ReFS) volume available, or acceptable to create, for workspaces?
19. Acceptable to require the Rust toolchain to build and ship one `farseer.exe`? Or must it be installable via `bun`/`npm` for hackability? (Note: a native binary also dodges the MCP `npx ENOENT` class of bugs entirely.)
20. Tray app, Windows service, or foreground process the operator starts? Expected sleep and resume behavior?
21. Graph engine preference: start with **SQLite edge tables plus recursive CTEs** (zero extra dependency), or commit to an embedded graph engine (LadybugDB) now for Cypher and path queries?

**Harnesses and seats**

22. Which harnesses must work at M2, in priority order? Current guess: Claude Code, Codex, Antigravity (`agy`), Pi.
23. How many seats per harness exist in practice? One each, or multiple accounts to rotate?
24. Standing policy when the good seat is exhausted: (a) queue and wait, (b) run on a cheaper model flagged as degraded, or (c) ask each time?
25. Are pay-per-token API keys in play as an overflow tier, or subscriptions only?
26. Token-level router (Switchyard or LiteLLM) in scope for M4, or explicitly deferred?

**Policy and safety**

27. Default autonomy: may a worker commit and push to a branch without asking, or does every write stop at a diff for review?
28. Who merges? Never farseer, farseer per project policy, or farseer after CI is green?
29. Is `gh`/GitLab integration required, or is "branch pushed, you take it from here" enough for v1?
30. What must never happen unattended? Draft the deny list now: force push, history rewrite, secret files, package publish, anything touching `.env`, migrations against a real database.
31. Should farseer own a delivery gate, or rely on repository tests and GitHub CI?

**Vocabulary**

32. Minimum task contract fields. Proposed: goal, non-goals, delivery path, autonomy grant, validation command, done-criteria, token/time budget, record slice to inject. Anything missing?
33. Board lanes: are `inbox / planned / ready / running / review / blocked / done / abandoned` right, or do you think in different states?
34. Import existing firstmate state (`D:\Dev\firstmate\state`, `data`), or start clean?
35. Confirm the name "farseer" and the CLI verb: `farseer`, `fsr`, or something else? (`fs` collides with too much.)

## Sources

**Orchestrators**

- [kunchenguid/firstmate on DeepWiki](https://deepwiki.com/kunchenguid/firstmate)
- [firstmate docs/herdr-backend.md](https://github.com/kunchenguid/firstmate/blob/main/docs/herdr-backend.md)
- [Talk to One Agent, Ship With a Crew (SudoAll)](https://sudoall.com/talk-to-one-agent-first-mate-agentic-stack/)
- [Superset vs Firstmate (2026)](https://superset.sh/compare/superset-vs-firstmate)
- [Vibe Kanban](https://www.vibekanban.com/)
- [BloopAI/vibe-kanban on DeepWiki](https://deepwiki.com/BloopAI/vibe-kanban)
- [vibe-kanban issue #1941: SQLite locking on WSL2](https://github.com/BloopAI/vibe-kanban/issues/1941)
- [Vibe Kanban: The Git Worktree Strategy (Starlog)](https://starlog.is/articles/ai-dev-tools/bloopai-vibe-kanban/)
- [Best Tools for Managing Parallel AI Coding Agents 2026 (Nimbalyst)](https://nimbalyst.com/blog/best-agent-management-tools-2026/)
- [awesome-agent-orchestrators](https://github.com/andyrewlee/awesome-agent-orchestrators)
- [9 Open-Source Agent Orchestrators (Augment Code)](https://www.augmentcode.com/tools/open-source-agent-orchestrators)
- [Omnara AI Command Center](https://apps.apple.com/qa/app/omnara-ai-command-center/id6748426727)
- [Enderfga/claw-orchestrator](https://github.com/Enderfga/claw-orchestrator)
- [gluk-w/claworc: orchestrator for OpenClaw](https://github.com/gluk-w/claworc)
- [OpenClaw Multi-Agent Orchestration (Liam Beeton)](https://www.liambeeton.com/openclaw-multi-agent-orchestration)
- [Block: Introducing Buzz](https://block.xyz/inside/introducing-buzz-where-humans-and-agents-work-together)
- [Block's Buzz self-host guide (Rohit Raj)](https://rohitraj.tech/en/notes/block-buzz-agent-collaboration-platform-guide-2026)
- [Herdr: The Rust Agent Multiplexer (CoddyKit)](https://www.coddykit.com/pages/blog-detail?id=512884&slug=herdr-the-rust-agent-multiplexer-that-runs-all-your-ai-coding-agents-in-one-term)
- [Herdr with built-in agent state awareness (Better Stack)](https://betterstack.com/community/guides/ai/herdr-ai-agent/)

**Worker control and harnesses**

- [Agent Client Protocol: Agents](https://agentclientprotocol.com/get-started/agents)
- [ACP: The LSP for AI Coding Agents (Marc Nuri)](https://blog.marcnuri.com/agent-client-protocol-acp-introduction)
- [Zed docs: External Agents](https://zed.dev/docs/ai/external-agents)
- [Claude Code docs: Manage multiple agents with agent view](https://code.claude.com/docs/en/agent-view)
- [Claude Code docs: Manage sessions](https://code.claude.com/docs/en/sessions)
- [Claude Code Headless Mode guide (amux)](https://amux.io/guides/claude-code-headless/)
- [Claude Agent SDK Complete Guide](https://hidekazu-konishi.com/entry/claude_agent_sdk_complete_guide.html)
- [Headless AI Coding Agents in CI compared (Developers Digest)](https://www.developersdigest.tech/blog/headless-ai-coding-agents-ci-comparison-2026)
- [RobertTLange/headless-cli](https://github.com/RobertTLange/headless-cli)
- [pi-agent: RPC Mode and Headless Integration (DeepWiki)](https://deepwiki.com/agentic-dev-io/pi-agent/7-rpc-mode-and-headless-integration)
- [pi-interactive-shell](https://pi.dev/packages/pi-interactive-shell)
- [pi-antigravity](https://pi.dev/packages/pi-antigravity)
- [Antigravity CLI (agy) guide](https://www.aibuilderclub.com/blog/antigravity-cli-guide)
- [Antigravity CLI: Orchestrating Parallel AI Agents (DataCamp)](https://www.datacamp.com/tutorial/antigravity-cli)

**Memory and knowledge base**

- [Mem0 vs Zep (Graphiti) compared (Vectorize)](https://vectorize.io/articles/mem0-vs-zep)
- [Agent Memory Systems and Knowledge Graphs: Letta, Mem0, Graphiti, Cognee](https://codepointer.substack.com/p/agent-memory-systems-and-knowledge)
- [Survey of AI Agent Memory Frameworks (Graphlit)](https://www.graphlit.com/blog/survey-of-ai-agent-memory-frameworks)
- [Best AI Agent Memory Frameworks 2026 (Atlan)](https://atlan.com/know/best-ai-agent-memory-frameworks-2026/)
- [Advanced Agent Memory: Temporal Knowledge Graphs (NomadX)](https://nomadx.ae/blog/advanced-agent-memory-knowledge-graphs-zep-graphiti-2026/)
- [Basic Memory MCP Server](https://heyclau.de/entry/mcp/basic-memory-mcp-server)
- [Memorix MCP Server](https://mcpservers.org/servers/avids2/memorix)
- [Memory MCP server category (176 servers)](https://mcpservers.org/category/memory)
- [Cross-Tool Agent Memory and the Portability Problem](https://codex.danielvaughan.com/2026/04/17/cross-tool-agent-memory-mempalace-portability/)
- [claude-mem discussion #1329: shared backend for Codex-style agents](https://github.com/thedotmack/claude-mem/discussions/1329)
- [Honest Lying: Memory Confabulation in Reflexive Agents (arXiv 2605.29463)](https://arxiv.org/pdf/2605.29463)

**Graph and store engines**

- [From Kuzu to Ladybug (The Data Quarry)](https://thedataquarry.com/blog/from-kuzu-to-ladybug/)
- [Kuzu's Legacy and the New Wave of Embedded Graph Databases (gdotv)](https://gdotv.com/blog/kuzu-legacy-embedded-graph-database-landscape/)
- [LadybugDB](https://ladybugdb.com/)
- [LadybugDB on Database of Databases](https://dbdb.io/db/ladybugdb)
- [graphiti issue #1509: LadybugDB driver support](https://github.com/getzep/graphiti/issues/1509)
- [Vela-Engineering/kuzu fork: concurrent multi-writer for agent memory](https://github.com/Vela-Engineering/kuzu)
- [KuzuDB Fork for AI Agents (Vela Partners)](https://vela.partners/blog/kuzudb-ai-agent-memory-graph-database)
- [Neo4j Alternatives in 2026 (ArcadeDB)](https://arcadedb.com/blog/neo4j-alternatives-in-2026-a-fair-look-at-the-open-source-options/)
- [Kùzu, an extremely fast embedded graph database (The Data Quarry)](https://thedataquarry.com/blog/embedded-db-2/)
- [sqliteai/sqlite-vector](https://github.com/sqliteai/sqlite-vector)
- [How sqlite-vec works for storing and querying embeddings](https://dev.to/stephenc222/how-sqlite-vec-works-for-storing-and-querying-vector-embeddings-2g9b)
- [Choosing an embeddable vector database (Shaharia Azam)](https://shaharia.com/blog/choosing-embeddable-vector-database-go-application/)
- [DuckDB Lance vector search roundup](https://media.patentllm.org/news/database/duckdb-lance-vector-search-sqlite-benchmarking-postgresql-va-20260705)

**Session logs, traces, observability**

- [Claude Code JSONL transcript format explained](https://claude-dev.tools/docs/jsonl-format)
- [Reading Claude Code session transcripts](https://claude-dev.tools/docs/transcripts)
- [Inside Claude Code: The Session File Format (Yi Huang)](https://databunny.medium.com/inside-claude-code-the-session-file-format-and-how-to-inspect-it-b9998e66d56b)
- [Claude Code Session Files: JSONL Format (Aditya Bawankule)](https://www.adityabawankule.io/blog/claude-code-session-jsonl-format)
- [simonw/claude-code-transcripts](https://github.com/simonw/claude-code-transcripts)
- [daaain/claude-code-log](https://github.com/daaain/claude-code-log)
- [AI Agent Observability 2026: Tracing and Monitoring Stack](https://www.digitalapplied.com/blog/ai-agent-observability-2026-tracing-monitoring-stack-guide)
- [Langfuse vs Arize Phoenix (QASkills)](https://qaskills.sh/blog/langfuse-vs-arize-phoenix)
- [Top 5 LLM and Agent Observability Tools 2026 (MLflow)](https://mlflow.org/top-5-agent-observability-tools/)

**Graph engineering, event sourcing, self-improvement**

- [The Log is the Agent: Event-Sourced Reactive Graphs (arXiv 2605.21997)](https://arxiv.org/html/2605.21997v1)
- [Event Sourcing Pattern (Microsoft Azure Architecture Center)](https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing)
- [Is the audit log a proper architecture driver for Event Sourcing? (Event-Driven.io)](https://event-driven.io/en/audit_log_event_sourcing/)
- [Synergizing LLMs and Knowledge Graphs for repository QA (arXiv 2412.03815)](https://arxiv.org/pdf/2412.03815)
- [Enhancing repository-level repair via repository-aware knowledge graphs (arXiv 2503.21710)](https://arxiv.org/pdf/2503.21710)
- [Knowledge Graph Based Repository-Level Code Generation (arXiv 2505.14394)](https://arxiv.org/html/2505.14394v1)
- [Nalanda: A Socio-Technical Graph for Software Analytics (arXiv 2110.08403)](https://arxiv.org/pdf/2110.08403)
- [JonnoC/CodeRAG](https://github.com/jonnoc/coderag)
- [GraphRAG for Devs: Graph-Code demo (Memgraph)](https://memgraph.com/blog/graphrag-for-devs-coding-assistant)
- [Agent Reflection: How AI Agents Self-Improve (Stackviv)](https://stackviv.ai/blog/reflection-ai-agents-self-improvement)
- [Reflexion technical review](https://www.zhongzhuzhou.org/blog/2026-02-20-2026-02-20-Reflexion-technical-review-en/)
- [Experiential Reflective Learning for Self-Improving LLM Agents (arXiv 2603.24639)](https://arxiv.org/html/2603.24639v1)

**Multi-agent orchestration risk**

- [Orchestrator-Worker Agents: framework comparison (Arize)](https://arize.com/blog/orchestrator-worker-agents-a-practical-comparison-of-common-agent-frameworks/)
- [AI Agent Handoff Protocols: passing context without data loss](https://semnexus.com/ai-agent-handoff-protocols-passing-context-between-agents)
- [Multi-Agent Orchestration: A Practical Architecture (Augment Code)](https://www.augmentcode.com/guides/multi-agent-orchestration-architecture-guide)
- [Microsoft Agent Framework: Handoff orchestration](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/handoff)

**Windows platform issues**

- [claude-code #41740: worktree removal fails, file locks on Windows](https://github.com/anthropics/claude-code/issues/41740)
- [claude-code #61343: unlink EPERM breaks .git lock cleanup](https://github.com/anthropics/claude-code/issues/61343)
- [claude-code #46740: native sandbox support for Windows](https://github.com/anthropics/claude-code/issues/46740)
- [claude-code #58510: Windows plugin MCP servers fail with spawn ENOENT on bare npx](https://github.com/anthropics/claude-code/issues/58510)
- [copilot-cli #3576: Windows stdio MCP servers fail to spawn](https://github.com/github/copilot-cli/issues/3576)
- [vscode #299595: MCP fails to start with ENOENT on npx.cmd](https://github.com/microsoft/vscode/issues/299595)
- [Fix "spawn npx ENOENT" in MCP server setup](https://mcptools.tools/guides/fix-spawn-npx-enoent/)
- [Claude Code docs: Choose a sandbox environment](https://code.claude.com/docs/en/sandbox-environments)
- [MXC Internals: how Microsoft's eXecution Containers isolate agent code](https://www.originhq.com/research/mxc-execution-containers-internals)
- [Microsoft's sandboxed WSL AI layer (XDA)](https://www.xda-developers.com/microsofts-new-sandboxed-wsl-ai-layer-changes-everything-about-how-windows-runs-agents/)
- [node-pty #437: unable to kill pty process on Windows](https://github.com/microsoft/node-pty/issues/437)
- [node-tree-kill](https://github.com/pkrumins/node-tree-kill)
- [git worktree documentation](https://git-scm.com/docs/git-worktree)
- [git worktree node_modules: sharing dependencies properly](https://continuumcode.ai/guides/git-worktree-node-modules/)
- [Solving Windows Path Length Limitations in Git (Shady Nagy)](https://www.shadynagy.com/solving-windows-path-length-limitations-in-git/)
- [Codex CLI on Windows 2026: Native vs WSL2](https://ofox.ai/blog/codex-windows-wsl-installation/)

**Routing**

- [NVIDIA-NeMo/Switchyard](https://github.com/NVIDIA-NeMo/Switchyard)
- [Route AI Agent Workloads with NeMo Switchyard (NVIDIA)](https://developer.nvidia.com/blog/route-ai-agent-workloads-across-models-with-nvidia-nemo-switchyard/)
- [Nvidia's Switchyard router (VentureBeat)](https://venturebeat.com/orchestration/nvidias-switchyard-router-reshuffles-ai-models-mid-task-cutting-task-costs-to-a-third-in-its-own-tests)
- [LiteLLM: Using Claude Code Max Subscription](https://docs.litellm.ai/docs/tutorials/claude_code_max_subscription)
- [ypollak2/llm-router](https://github.com/ypollak2/llm-router)
- [OmniRoute: local AI gateway for Claude Code](https://explainx.ai/blog/omniroute-ai-gateway-free-llm-proxy-claude-code-2026)
- [Claude Code Rate Limits and Usage Quotas Explained (TrueFoundry)](https://www.truefoundry.com/blog/claude-code-limits-explained)
