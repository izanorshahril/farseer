# Farseer: Brief and Research Report

Status: draft 1, research only, no code decisions locked.
Date: 2026-08-18.
Author: drafted by agent from user brief plus web research.

## 1. Problem statement

`D:\Dev\firstmate` gets the concept right and the substrate wrong.

Its concept: one manager agent owns the layer between operator intent and the agents that do the work, so the operator context-switches once instead of N times.
Its substrate: ~140 bash scripts driving tmux/herdr/wezterm/treehouse/gh, an "agent distro" that assumes a POSIX box with cheap process control and no file locks.

On Windows that substrate fails in ways that are not bugs but platform mismatches:

- Locked terminals and panes, because the orchestration layer is a terminal multiplexer and the agent lives inside a PTY it does not fully control.
- Folder permission and cleanup failures, because git worktrees plus `node_modules` plus running dev servers cannot be removed while any handle is open.
- Integration sprawl, because every capability is another external binary that must exist, be on PATH, and behave identically across Windows, WSL, and mac.

Goal: rebuild the concept on a substrate that is native to Windows first, portable to mac/Linux later, and that depends on process contracts rather than terminal scraping.

## 2. Product concept

One manager. Many projects. Many disposable workers.

- **Manager** is a single long-lived planning agent (Claude Code or Codex, chosen by available quota).
  It never edits project code.
  It plans, decomposes, writes task contracts, hands off, reads worker reports, escalates decisions, and stays idle-cheap so it is always available.
- **Workers** are short-lived harness runs in isolated workspaces.
  Any harness: Claude Code (cheaper model), Codex, Antigravity (`agy`), Pi, others.
  Workers are supervised, not trusted.
- **Board** is a kanban over projects and tasks, the durable source of truth.
  Manager state lives on disk, not in chat memory, so any restart is a non-event.
- **Router** picks the harness+model per task from quota, capability, and cost.
- **Operator** talks to the manager and to the board, never to individual worker terminals unless they choose to.

Explicit non-goals for v1:

- No human-to-human collaboration (that is Buzz's problem, see 4.4).
- No hosted service, no cloud control plane, local-first single operator.
- No terminal multiplexer of our own, and no dependency on one.
- Not a harness. We drive harnesses, we do not implement an agent loop.

## 3. What to keep from firstmate

Concepts worth porting, mechanics worth discarding.

Keep:

- Single-interface command layer, operator talks to one agent.
- Manager reads projects but never writes them.
- Explicit task contract before work starts (goal, delivery path, autonomy grant, validation).
- Authority is explicit and never inferred; merges and destructive acts need a human word.
- Durability: everything survives session death, reconcile from disk.
- Deterministic scripts own mechanics, agents own judgment.
- Flat command structure, capped depth.
- Vendor independence, adapters earn trust through verification.
- Quota and model choices stay inspectable and operator-owned.

Discard:

- tmux/herdr/wezterm/zellij/orca backend matrix as the execution substrate.
- Bash as the implementation language for control logic.
- `gh` and treehouse as required dependencies.
- Hook-based supervision that depends on a specific harness's hook surface as the only signal.
- Terminal-pane state inference (`pane read`) as the primary way to know what an agent is doing.

## 4. Landscape research

### 4.1 firstmate (kunchenguid/firstmate, MIT)

An "agent distro": portable directory of instructions, skills, scripts, conventions that turns a terminal agent into an orchestrator spawning crewmates in tmux windows and disposable git worktrees.
Backends: tmux (reference), herdr, cmux, zellij, orca.
Strength: the vision document is the best articulation of the single-captain model available.
Limitation: mac-shaped, shell-heavy, integration-heavy, and orchestration quality is bounded by multiplexer fidelity.

### 4.2 Vibe Kanban (BloopAI, open source)

Closest architectural precedent.
Rust backend (Axum, SQLx, Tokio) plus React frontend; crates split into server / services / db / executors.
Key idea: "code state is managed by Git, workflow state is managed by SQLite."
Executor abstraction normalizes Claude Code, Codex, Amp, Cursor CLI, Gemini, and others.
Git worktree per task, kanban board, visual diff review.
Cross-platform, unlike Conductor.
Bloop shut down 10 Apr 2026; project continues community-maintained, cloud features removed, fully local.
Known Windows-class defect: SQLite locking causing hangs and OOM on WSL2 (issue #1941).
Limitation for us: it is task-board-driven with a human doing the planning; there is no persistent manager agent doing decomposition and handoff, and no quota-aware router.

### 4.3 Conductor / Crystal (now Nimbalyst) / Superset

- Conductor: macOS only, parallel Claude Code and Codex in isolated worktrees, central dashboard. Disqualified on platform.
- Crystal renamed Nimbalyst, broader workspace product.
- Superset positions itself as "agent workspace" against firstmate's "DIY agent distro".
Pattern across all: they are session viewers with worktree isolation.
The context switch stays with the human.

### 4.4 Buzz (Block, Apache-2.0)

Shared workspace where humans and agents are co-participants; Nostr protocol; channels, threads, DMs, voice, repos, workflows.
Agents get cryptographic identities and scoped permissions.
Model- and harness-agnostic via ACP, already driving Goose, Codex, Claude Code.
Self-host path plus Block-hosted relay.
Relevance: confirms ACP as the interop bet and confirms per-agent identity/permission as a real design axis.
Not our shape: multi-human collaboration, chat-first, no single-manager planning layer.

### 4.5 Herdr

Rust single-binary (~10MB) agent-aware multiplexer, tmux rebuilt with awareness of what the agent in each pane is doing.
Good tool. Wrong layer for us.
If we need agent state awareness we should get it from the harness's structured event stream, not from a terminal that infers it.

### 4.6 Interop and routing substrate

**ACP (Agent Client Protocol)**: JSON-RPC 2.0, "LSP for coding agents".
Gemini/Antigravity supports it natively (`--acp`), Claude Code via `claude-agent-acp` adapter, Codex via `codex-acp`.
Zed, VS Code extensions, OpenClaw, Buzz all consume it.
This is the strongest candidate for a stable worker-control interface that is not PTY scraping.

**Headless CLI contracts** (fallback and complement to ACP):
- Claude Code: `--output-format text|json|stream-json`, stdin/stdout piping, `--allowedTools`, `--permission-mode`, `--bare` for explicit credentials. Claude Agent SDK (TS/Python) exposes the same loop programmatically.
- Codex: `codex exec --json` emits newline-delimited JSON events plus a real exit code.
- Antigravity CLI (`agy`): successor to Gemini CLI (Gemini CLI shut down for free/Pro/Ultra 18 Jun 2026), scriptable, auto-approve modes.
- Pi: MIT, sub-1k-token system prompt, lazy skills, designed to be forked. Also a `pi-antigravity` provider with native streaming, model routing, quota diagnostics.
- `RobertTLange/headless-cli` is prior art for a unified headless wrapper; default fallback order codex, claude, pi, opencode, gemini, antigravity, cursor.

**Routing**:
- NVIDIA NeMo Switchyard (announced 11 Aug 2026, open source, `NVIDIA-NeMo/Switchyard`): classifier plus proxy that speaks OpenAI, Anthropic, and Responses APIs; routing strategies include LLM classifier with session affinity, stage router reading recent tool activity, escalation router that starts cheap and promotes on sustained difficulty; builds on RouteLLM, adds session state and action traces. LangChain eval: 74% cost cut vs Opus 4.8 alone, accuracy 86.0% to 80.0%.
- LiteLLM: gateway with fallback chains, documented Claude Code Max subscription tutorial.
- claude-code-router, `ypollak2/llm-router`, OmniRoute, 9Router: subscription-first then paid API then cheap then free tiers, quota-strain demotion.
Caveat: all of these route *tokens*.
Our harder problem is routing *seats*, since Claude Code and Codex subscription quotas are enforced per account on rolling windows (Claude: 5-hour rolling plus weekly caps) and are not visible as a clean API.

### 4.7 Windows sandbox and isolation reality

- Claude Code `/sandbox` uses Seatbelt on mac and bubblewrap on Linux; native Windows is unsupported (feature request anthropics/claude-code#46740).
- Without a sandbox, deny rules only bind built-in tools; `Bash(cat ~/.ssh/id_rsa)` bypasses `Read(~/.ssh/**)`.
- Current recommended mitigation on Windows is standardize on WSL2, which is exactly the UX the operator rejected.
- Emerging: Microsoft Execution Containers (MXC), announced Build 2026, early preview, dispatches to AppContainer and the new BaseContainer API on Windows plus WSL containers.
Implication: v1 must not promise OS-level sandboxing on Windows.
Isolation comes from workspace separation, allowlists, and a review gate, and MXC is a v2 upgrade path to watch.

## 5. Windows failure-mode catalog

This is the list the app must be designed against, not patched for.

**Process control**
- No POSIX signals. `pty.kill(signal)` throws on Windows in node-pty.
- Killing a parent leaves orphans. Correct pattern is `taskkill /pid <pid> /T /F` or, better, a **Win32 Job Object** with kill-on-job-close so the whole tree dies when the supervisor dies.
- ConPTY is used on build >= 18309 and has its own lifecycle quirks; there are open node-pty issues about unkillable pty processes on Windows.
- Ctrl-C delivery into a child console is a per-console-group affair, not a signal.

**Filesystem**
- Mandatory file locking. Any open handle (dev server, watcher, editor, antivirus, indexer) blocks delete and rename.
- `git worktree remove` fails on Windows when a node process holds locks (anthropics/claude-code#41740).
- `unlink` returns EPERM on `.git/*.lock` in sandboxed contexts (#61343), leaving stale lock files that wedge git.
- MAX_PATH 260. `core.longpaths true` helps git but not every consumer; deep `node_modules`/pnpm trees still break tools that use non-`\\?\` paths.
- Case-insensitive, case-preserving FS. Renames that only change case, and two files differing by case, both misbehave.
- CRLF. `core.autocrlf` interacts badly with diff review and with agents writing files.
- Reserved names (`con`, `aux`, `nul`, `prn`), trailing dots and spaces are illegal in filenames.
- Antivirus/Defender real-time scanning adds latency and transient sharing violations on fresh worktrees.

**WSL boundary**
- `\\wsl$\` and `/mnt/d` crossings are slow, break inotify, and break git file mode/permission assumptions.
- Two credential stores, two PATHs, two node installs, two sets of config.
- SQLite over the 9p/DrvFs boundary is a known corruption/locking hazard (vibe-kanban #1941).
Decision candidate: **never cross the boundary for state or repos**. Either fully native or fully inside WSL, chosen once, per install.

**Terminals**
- Terminal multiplexers on Windows are the weakest link: tmux needs WSL/Cygwin, wezterm mux and herdr are mac/Linux-first, and pane-scraping is fragile with ConPTY reflow.
Design consequence: **the terminal is a view, never the control plane**.

**Misc**
- Long-running background services and Windows sleep/hibernate: wake handling, and no systemd. Task Scheduler or a user-session tray process instead.
- Per-user PATH vs machine PATH, and `.cmd` shim resolution for node CLIs (`claude.cmd`, `codex.cmd`) requires `shell: true` or explicit resolution.

## 6. Proposed architecture

### 6.1 Shape

Local-first single binary plus local web UI.
No required external binaries beyond the agent CLIs the operator already has.

```
farseer/
  core (Rust)              orchestrator daemon, single binary
    api                    HTTP + SSE/WebSocket for UI and CLI
    store                  SQLite (WAL) = workflow state; git = code state
    scheduler              task queue, concurrency caps, wake events
    supervisor             process lifecycle, Job Objects, health, timeouts
    adapters               per-harness driver (ACP first, headless JSON fallback)
    router                 quota/capability/cost routing + seat accounting
    workspace              isolation strategy per project (worktree | clone | in-place)
    manager                the planning agent session, driven like any other harness
  ui (React or Svelte)     kanban board, project view, task detail, diff review, live log
  cli (farseer.exe)        non-interactive, JSON out, explicit exit codes
```

Why Rust: single self-contained binary, no runtime install, direct Win32 (Job Objects, `\\?\` paths, `CreateProcess` flags), and vibe-kanban proves the stack (Axum + SQLx + Tokio) works for this exact job.
Alternative considered: Bun/TS single-file executable. Faster to build, but node-pty and Win32 process-group work is exactly where TS is weakest.

### 6.2 Control plane rules

1. **No PTY as control channel.** Adapters speak ACP over stdio, or newline-delimited JSON over stdio (`claude --output-format stream-json`, `codex exec --json`).
   A PTY is attached only when the operator explicitly asks to watch or intervene, and it is a view.
2. **Every child in a Job Object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
   Daemon death kills the fleet, no orphans, no locked folders.
3. **Nothing runs from inside a workspace we intend to delete.**
   No cwd, no dev server, no watcher.
   Cleanup is a supervised state machine: stop children, wait for handle release, retry with backoff, then quarantine and report; never a blind `rm -rf` that half-fails.
4. **Workspaces are addressed with `\\?\` long paths** and created at a short root (for example `D:\fw\<hash>`) to stay under practical path limits.
5. **State is a single SQLite file in a native path** with WAL, never on a WSL/UNC path.
6. **Every worker run is fully reconstructible from disk**: contract, event log, artifacts, exit status.

### 6.3 Workspace isolation strategies

Per-project config, because one strategy cannot fit all.

| Strategy | Mechanism | Good for | Windows risk |
|---|---|---|---|
| `worktree` | `git worktree add` at short root, `.fwinclude` copy of gitignored deps | most repos | lock-on-remove, dep install cost |
| `clone` | local clone, shared object store via alternates | repos hostile to worktrees | disk |
| `snapshot` | copy-on-write via `Dev Drive` / ReFS block clone if available, else robocopy | huge non-git trees | availability |
| `in-place` | serialized single-writer lock on the real repo | small edits, docs, ops tasks | no parallelism |

Note the `.worktreeinclude` precedent: gitignore-syntax file listing gitignored paths to copy so tracked files are never duplicated.
Dev Drive (ReFS) on Windows 11 is worth measuring: block cloning plus reduced Defender surface could make `snapshot` the fastest option.

### 6.4 Manager agent

Manager is just another adapter session, with a distinct contract:

- Long-lived but **idle-cheap**: it sleeps on an event queue and wakes on board changes, worker completions, and operator messages. Idle costs zero tokens.
- Read-only on all projects. Enforced by adapter allowlist plus a read-only workspace mount where the strategy allows.
- Owns: intake, decomposition, contract authoring, routing hints, worker report triage, escalation, board mutation.
- Writes plans, contracts, and decisions to the store as structured records, not prose in a chat log.
- Compaction policy: manager context is a rolling window over the board plus recent events, rehydrated from the store on every wake, so a restart is a non-event and context never grows unboundedly.
- Escalation is the only thing that reaches the operator unprompted.

### 6.5 Router (autorouter)

Two distinct routing problems. Do not conflate them.

**A. Seat routing (the actual need).**
Which *harness account* runs this task, given subscription quota.
Inputs: rolling-window usage estimates per seat, observed rate-limit/429 events, weekly cap state, task class.
Behavior: manager prefers a seat that is not strained; workers get demoted to cheaper seats/models; if all seats are strained, tasks queue rather than silently degrade intelligence.
Hard constraint from firstmate's vision, which we keep: **never downgrade the intelligence doing the work without explicit standing permission.**
Reality check: subscription quota is not exposed as a clean API. We will have to infer it from response headers, rate-limit errors, and our own token accounting. Prior art: `pi-antigravity` ships quota diagnostics; OmniRoute does quota-share enforcement.

**B. Token routing (optional, later).**
Per-request model selection inside a worker.
Delegate to an existing router rather than building one.
Switchyard is the strongest fit because it speaks both OpenAI and Anthropic wire formats, so it can sit under Claude Code and Codex without either knowing.
LiteLLM is the pragmatic alternative with a documented Claude Code Max path.

Design decision: farseer owns (A), and treats (B) as a pluggable upstream base-URL setting per seat.

### 6.6 Data model sketch

```
project(id, name, path, vcs, delivery_policy, autonomy_grant, isolation_strategy, status)
task(id, project_id, title, contract_json, status, lane, priority, parent_id, created, updated)
  status: inbox | planned | ready | running | review | blocked | done | abandoned
run(id, task_id, seat_id, harness, model, workspace_path, pid, job_handle,
    started, ended, exit_code, cost_tokens, verdict)
event(id, run_id|task_id, ts, kind, payload_json)        -- append-only, the audit trail
seat(id, harness, account_label, auth_mode, quota_policy, window_state_json, health)
decision(id, task_id, question, options_json, answered_by, answer, ts)
artifact(id, run_id, kind, path, sha256)                  -- diffs, reports, logs
```

Kanban lanes map to `task.status`.
The board is a projection, not the truth; `event` is the truth.

### 6.7 UI

Local web UI, opened in the operator's browser or a thin webview. Not a terminal TUI, because the terminal is the thing that hurts today.

- Board: swimlanes by project or by status, cards show harness, seat, model, elapsed, cost, blocked-on.
- Manager chat: one conversation, the only place the operator talks to an agent.
- Task detail: contract, event timeline, live log tail, diff review, approve/reject/merge.
- Fleet: running processes, seat quota gauges, kill switch.
- Escalations: an inbox of decisions only a human can make.

## 7. Risks and open technical unknowns

1. **Subscription quota is not observable.**
   Seat routing may be reduced to heuristics plus reactive backoff. Needs a spike.
2. **ACP adapter maturity.**
   `claude-agent-acp` and `codex-acp` are third-party adapters. If they lag, we fall back to per-harness headless JSON, which means N adapters to maintain (vibe-kanban's `executors` crate is the precedent for how much work that is).
3. **Harness CLI churn.**
   Gemini CLI was killed 18 Jun 2026 and replaced by Antigravity CLI. Flags and event schemas will break. Adapters need contract tests and a verification suite before a harness is trusted.
4. **No Windows sandbox.**
   v1 cannot claim OS-level isolation. Watch MXC (AppContainer/BaseContainer) for v2.
5. **Worktree cleanup will still fail sometimes.**
   Design for quarantine and a background reaper, not for success.
6. **Defender/AV latency** on fresh workspaces may dominate task startup. Measure; consider Dev Drive and exclusion guidance in docs, never automatic AV config changes.
7. **Manager drift.**
   A long-lived planner that rehydrates from a store can still hallucinate board state. Every manager claim about a task must be a store read, not recall.
8. **Cost of the manager itself.**
   An always-on planner is a token sink if it wakes on noise. Event batching and a no-op short-circuit are mandatory, not nice-to-have.

## 8. Suggested milestones

- **M0 spike (throwaway).** Prove the three scary things on Windows: Job Object kill-on-close with a real Claude Code child; worktree create/destroy loop under a running dev server; `codex exec --json` and `claude --output-format stream-json` parsed end to end. Nothing else.
- **M1 single project, single worker.** Store, supervisor, one adapter, CLI only, no manager. Task in, diff out.
- **M2 board plus UI.** Kanban, event timeline, diff review, approve/merge gate.
- **M3 manager.** Planning agent, contracts, escalations, wake queue, restart-is-a-non-event proof (kill the daemon mid-run, recover).
- **M4 multi-harness plus seats.** Second and third adapter, seat registry, quota heuristics, demotion policy.
- **M5 portability.** mac/Linux, which should be a subtraction of Windows workarounds, not an addition.

## 9. Open questions for the operator

Answer whenever; each one changes design, none blocks M0.

**Scope and shape**
1. Is the operator-facing surface primarily the **board** or the **manager chat**? Which one do you open first in the morning?
2. Should farseer ever run headless with no UI (unattended, wake on schedule/webhook), or is it always operator-present?
3. Single machine only, or should farseer eventually dispatch workers to a second box or a cloud runner? This changes whether workspace paths can be assumed local.
4. Do you want the manager to be able to modify farseer itself (firstmate's self-evolving property), or is farseer strictly a tool that manages other projects?

**Runtime and platform**
5. Native Windows only for v1, or must WSL2 workers be supported from day one? If both, are we allowed to forbid mixing (no repo on `\\wsl$`, no state across the boundary)?
6. Is a Dev Drive (ReFS) volume available or acceptable to create for workspaces? It may be the single biggest perf lever.
7. Acceptable to require Rust toolchain for building, shipping a single `farseer.exe` binary? Or must it be installable via `bun`/`npm` for hackability?
8. Tray app, Windows service, or foreground process that the operator starts? Sleep/resume behavior expectations?

**Harnesses and seats**
9. Which harnesses must work at M4, in priority order? Current guess: Claude Code, Codex, Antigravity (`agy`), Pi.
10. How many seats/accounts per harness exist in practice? One each, or multiple accounts to rotate?
11. Standing policy on intelligence downgrade: when the good seat is exhausted, do you prefer (a) queue and wait, (b) run on a cheaper model with a marked-degraded flag, or (c) ask each time?
12. Are API keys (pay-per-token) in play as an overflow tier, or subscriptions only?
13. Do you want a token-level router (Switchyard/LiteLLM) under the harnesses in v1, or is that explicitly deferred?

**Policy and safety**
14. Default autonomy: may a worker commit and push to a branch without asking, or does every write stop at a diff for review?
15. Who merges? Never farseer, farseer with a policy per project, or farseer after CI green?
16. Is `gh`/GitLab integration required, or is "branch pushed, you take it from here" enough for v1?
17. What must never happen unattended? Draft the deny list now (force push, history rewrite, secret files, package publish, anything touching `.env`, migrations against a real DB).

**Process and validation**
18. Should farseer reuse the existing `no-mistakes` validation skill as the delivery rigor gate, or define its own?
19. What is the minimum task contract? Proposed fields: goal, non-goals, delivery path, autonomy grant, validation command, done-criteria, time/token budget. Anything missing?
20. Board vocabulary: are `inbox / planned / ready / running / review / blocked / done / abandoned` the right lanes, or do you think in different states?
21. Should farseer import existing state from firstmate (`D:\Dev\firstmate\state`, `data`), or start clean?

**Naming and identity**
22. Confirm the name "farseer" and the CLI verb. `farseer`, `fs` (collides with a lot), `fsr`, or something else?

## Sources

- [kunchenguid/firstmate on DeepWiki](https://deepwiki.com/kunchenguid/firstmate)
- [firstmate docs/herdr-backend.md](https://github.com/kunchenguid/firstmate/blob/main/docs/herdr-backend.md)
- [Talk to One Agent, Ship With a Crew (SudoAll)](https://sudoall.com/talk-to-one-agent-first-mate-agentic-stack/)
- [Superset vs Firstmate (2026)](https://superset.sh/compare/superset-vs-firstmate)
- [Herdr: The Rust Agent Multiplexer (CoddyKit)](https://www.coddykit.com/pages/blog-detail?id=512884&slug=herdr-the-rust-agent-multiplexer-that-runs-all-your-ai-coding-agents-in-one-term)
- [Herdr: Terminal Multiplexer with Built-in AI Agent State Awareness (Better Stack)](https://betterstack.com/community/guides/ai/herdr-ai-agent/)
- [Vibe Kanban](https://www.vibekanban.com/)
- [BloopAI/vibe-kanban on DeepWiki](https://deepwiki.com/BloopAI/vibe-kanban)
- [vibe-kanban issue #1941: SQLite locking on WSL2](https://github.com/BloopAI/vibe-kanban/issues/1941)
- [Vibe Kanban: The Git Worktree Strategy (Starlog)](https://starlog.is/articles/ai-dev-tools/bloopai-vibe-kanban/)
- [Vibe Kanban Tool Review (Eleanor Berger)](https://elite-ai-assisted-coding.dev/p/vibe-kanban-tool-review)
- [Best Tools for Managing Parallel AI Coding Agents 2026 (Nimbalyst)](https://nimbalyst.com/blog/best-agent-management-tools-2026/)
- [9 Open-Source Agent Orchestrators (Augment Code)](https://www.augmentcode.com/tools/open-source-agent-orchestrators)
- [awesome-agent-orchestrators](https://github.com/andyrewlee/awesome-agent-orchestrators)
- [Block: Introducing Buzz](https://block.xyz/inside/introducing-buzz-where-humans-and-agents-work-together)
- [Block's Buzz self-host guide (Rohit Raj)](https://rohitraj.tech/en/notes/block-buzz-agent-collaboration-platform-guide-2026)
- [Agent Client Protocol: Agents](https://agentclientprotocol.com/get-started/agents)
- [ACP: The LSP for AI Coding Agents (Marc Nuri)](https://blog.marcnuri.com/agent-client-protocol-acp-introduction)
- [Zed docs: External Agents](https://zed.dev/docs/ai/external-agents)
- [Headless AI Coding Agents in CI compared (Developers Digest)](https://www.developersdigest.tech/blog/headless-ai-coding-agents-ci-comparison-2026)
- [Claude Code Headless Mode guide (amux)](https://amux.io/guides/claude-code-headless/)
- [Claude Agent SDK Complete Guide](https://hidekazu-konishi.com/entry/claude_agent_sdk_complete_guide.html)
- [RobertTLange/headless-cli](https://github.com/RobertTLange/headless-cli)
- [Antigravity CLI (agy) guide](https://www.aibuilderclub.com/blog/antigravity-cli-guide)
- [Google Antigravity CLI: Orchestrating Parallel AI Agents (DataCamp)](https://www.datacamp.com/tutorial/antigravity-cli)
- [pi-antigravity package](https://pi.dev/packages/pi-antigravity)
- [NVIDIA-NeMo/Switchyard](https://github.com/NVIDIA-NeMo/Switchyard)
- [Route AI Agent Workloads with NeMo Switchyard (NVIDIA)](https://developer.nvidia.com/blog/route-ai-agent-workloads-across-models-with-nvidia-nemo-switchyard/)
- [Nvidia's Switchyard router (VentureBeat)](https://venturebeat.com/orchestration/nvidias-switchyard-router-reshuffles-ai-models-mid-task-cutting-task-costs-to-a-third-in-its-own-tests)
- [LiteLLM: Using Claude Code Max Subscription](https://docs.litellm.ai/docs/tutorials/claude_code_max_subscription)
- [ypollak2/llm-router](https://github.com/ypollak2/llm-router)
- [OmniRoute: Free Local AI Gateway for Claude Code](https://explainx.ai/blog/omniroute-ai-gateway-free-llm-proxy-claude-code-2026)
- [Claude Code Rate Limits and Usage Quotas Explained (TrueFoundry)](https://www.truefoundry.com/blog/claude-code-limits-explained)
- [claude-code issue #41740: Worktree removal fails, file locks on Windows](https://github.com/anthropics/claude-code/issues/41740)
- [claude-code issue #61343: unlink EPERM breaks .git lock cleanup](https://github.com/anthropics/claude-code/issues/61343)
- [claude-code issue #46740: Native sandbox support for Windows](https://github.com/anthropics/claude-code/issues/46740)
- [Claude Code docs: Choose a sandbox environment](https://code.claude.com/docs/en/sandbox-environments)
- [MXC Internals: how Microsoft's eXecution Containers isolate agent code](https://www.originhq.com/research/mxc-execution-containers-internals)
- [Microsoft's sandboxed WSL AI layer (XDA)](https://www.xda-developers.com/microsofts-new-sandboxed-wsl-ai-layer-changes-everything-about-how-windows-runs-agents/)
- [node-pty issue #437: Unable to kill pty process on Windows](https://github.com/microsoft/node-pty/issues/437)
- [node-tree-kill](https://github.com/pkrumins/node-tree-kill)
- [git worktree documentation](https://git-scm.com/docs/git-worktree)
- [git worktree node_modules: sharing dependencies properly](https://continuumcode.ai/guides/git-worktree-node-modules/)
- [Solving Windows Path Length Limitations in Git (Shady Nagy)](https://www.shadynagy.com/solving-windows-path-length-limitations-in-git/)
- [Codex CLI on Windows 2026: Native vs WSL2](https://ofox.ai/blog/codex-windows-wsl-installation/)
