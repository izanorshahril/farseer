# Command-center reference review

Date: 2026-09-06.

Scope: primary-source verification of Berd, Orca, Hermes Agent, and NVIDIA NeMo Switchyard for the Farseer review.

The local inspiration list names Hermes as a self-improvement reference and Orca as the closest orchestration reference in [Inspirationst.txt](../../../Inspirationst.txt), lines 16 and 22.

The historical [BRIEF.md](../../../BRIEF.md), line 54 onward, discusses runner routing separately from token routing and optional board/router surfaces.
Use the current decision map and implementation rather than that historical brief as authority for shipped behavior.

## Verified references

### Berd

- The canonical source is [block/berd](https://github.com/block/berd).
- Berd describes itself as an open-source desktop app for AI-agent work, built with Tauri 2 and React 19, using the Goose backend through an ACP WebSocket sidecar.[[README](https://github.com/block/berd#readme)]
- Its product contract emphasizes a persistent desktop workspace, explicit project/files/agent/model/session context, session history, extensions, providers, and automations.[[PRODUCT.md](https://github.com/block/berd/blob/main/PRODUCT.md)]
- Berd's own design guidance says operational state should be visible and actionable, and that the interface should remain calm rather than become a dashboard packed with metrics.[[PRODUCT.md](https://github.com/block/berd/blob/main/PRODUCT.md)]
- The repository exposes a distribution seam for optional companion tools and managed provider settings, which is a useful precedent for keeping optional integrations outside the minimum runtime.[[README](https://github.com/block/berd#bundling-and-distributions)]

Supported implication for Farseer: borrow Berd's context legibility, continuity, and quiet canvas principles, while treating its Goose/ACP desktop backend choice as an implementation detail rather than a Farseer core dependency.

### Orca

- The canonical source is [stablyai/orca](https://github.com/stablyai/orca).
- Orca positions itself as a multi-agent orchestrator that runs coding agents side by side, each in its own worktree, and tracks them in one place.[[README](https://github.com/stablyai/orca#readme)]
- Its published feature set includes mobile monitoring and steering, parallel worktrees, terminal splits with restart-persistent scrollback, browser design mode, issue/PR workflows, remote SSH worktrees, diff annotation, notifications, and an agent-driving CLI.[[README](https://github.com/stablyai/orca#features)]
- Orca states that it works with any CLI agent that runs in a terminal and lists many harnesses, which supports a broad adapter boundary rather than a single-vendor runtime.[[README](https://github.com/stablyai/orca#supported-agents)]
- The repository includes an orchestration skill guide describing structured task dispatch, worker completion/escalation waits, task DAGs, decision gates, and coordinator loops.[[orchestration.md](https://github.com/stablyai/orca/blob/main/skill-guides/orchestration.md)]

Supported implication for Farseer: borrow worktree-per-run isolation, resumable observation, mobile or remote control as a future client, and an agent-independent adapter surface.

Limitation: Orca's public description establishes a strong workspace and terminal orchestration surface, but it does not establish Farseer's append-only record, manager-owned contracts, cell permissions, quota truth, or durable cross-harness knowledge model.

### Hermes Agent

- The canonical source is [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent).
- Hermes describes itself as a self-improving agent with persistent memory, reusable skills, session search, user modeling, multiple model providers, and messaging gateways.[[README](https://github.com/NousResearch/hermes-agent#readme)]
- Its documented feature surface includes a CLI, messaging gateway, model switching, retry/undo, usage and insights commands, skills, memory, MCP integration, cron scheduling, context files, and a documented architecture.[[README](https://github.com/NousResearch/hermes-agent#cli-vs-messaging-quick-reference)] [[README](https://github.com/NousResearch/hermes-agent#documentation)]
- Hermes exposes both terminal and messaging entry points and supports changing the model during a conversation, which is relevant to a future remote operator client and model policy layer.[[README](https://github.com/NousResearch/hermes-agent#cli-vs-messaging-quick-reference)]
- Hermes' memory and skill systems are first-class agent features, but the repository describes one agent runtime rather than a neutral fleet control plane.[[README](https://github.com/NousResearch/hermes-agent#documentation)]

Supported implication for Farseer: borrow the distinction between durable memory, reusable skills, context files, usage inspection, and the runtime loop; keep memory promotion, rollback, and review under Farseer's record policy so self-improvement cannot silently change core behavior.

Limitation: Hermes' self-improvement claim is a product capability of its own runtime, not evidence that autonomous edits are safe for a supervisor or orchestration kernel.

### NVIDIA NeMo Switchyard

- The canonical source is [NVIDIA-NeMo/Switchyard](https://github.com/NVIDIA-NeMo/Switchyard).
- Switchyard exposes a provider-neutral Rust routing core through a standalone HTTP server and an embeddable Rust library.[[Core concepts](https://github.com/NVIDIA-NeMo/Switchyard/blob/main/docs/core_concepts.md#runtime-surfaces)]
- Its deployment separates upstream clients, targets, and client-visible routes, keeping transport and routing policy distinct and keeping secrets outside the TOML configuration.[[Core concepts](https://github.com/NVIDIA-NeMo/Switchyard/blob/main/docs/core_concepts.md#llm-clients-targets-and-routes)]
- The documented route types include passthrough, weighted random selection, an LLM classifier between weak and strong targets, and a stage router that uses tool-result and progress signals to choose an efficient or capable target.[[Core concepts](https://github.com/NVIDIA-NeMo/Switchyard/blob/main/docs/core_concepts.md#routing-algorithms)]
- The server supports OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages endpoints, plus translation between those provider formats.[[switchyard-server README](https://github.com/NVIDIA-NeMo/Switchyard/blob/main/crates/switchyard-server/README.md)]
- The server records routing logs, model and token statistics, latency metrics, retries, classifier failures, and upstream attempts, providing useful telemetry vocabulary for Farseer's analytics.[[switchyard-server README](https://github.com/NVIDIA-NeMo/Switchyard/blob/main/crates/switchyard-server/README.md#session-routing-log)] [[switchyard-server README](https://github.com/NVIDIA-NeMo/Switchyard/blob/main/crates/switchyard-server/README.md#metrics)]
- The documented system routes LLM requests and model targets; it does not describe task decomposition, worktree ownership, harness process supervision, or project-level delegation.[[Core concepts](https://github.com/NVIDIA-NeMo/Switchyard/blob/main/docs/core_concepts.md)]

Supported implication for Farseer: treat Switchyard as an optional model or provider gateway behind a runner adapter, with its route decision and telemetry imported as observations.

Limitation: Switchyard cannot by itself solve Farseer's runner/account routing problem, especially subscription windows and harness availability, because its documented target is an upstream LLM endpoint rather than a local CLI process or provider account.

## Recommendations for the review

1. Keep the current boundary: the core owns durable tasks, event truth, process supervision, contracts, permissions, workspace lifecycle, and recovery.
2. Keep kanban, graph projections, analytics, terminal presentation, remote clients, and automatic routing as removable consumers or policy modules over the record.
3. Define two routing contracts: runner/account selection for harness availability and model selection inside a runner; do not let a token router decide worktree, permission, or delegation ownership.
4. Make every self-improvement proposal an append-only candidate with provenance, evaluation results, operator or reviewer approval, version, rollback pointer, and expiry or demotion policy.
5. Use a capability-based runner adapter contract so a terminal-only harness, an ACP session, and a remote service can all expose the same normalized lifecycle, usage, session, and control events.
6. Make the UI consume the event/API contract and never become the source of task or run truth; preserve Farseer's existing widget sandbox and host-controlled bridge as the extension seam.
7. Record cost with basis and confidence: provider-billed cost, subscription usage, list-price estimate, and unknown must remain distinct so “optimal cost” does not become a false precision score.

## Cost-optimal routing warning

No cited reference proves that a router can always choose the globally cheapest successful execution.

The cheapest route depends on task difficulty, tool continuation behavior, context reuse, provider quotas, retry cost, latency, failure probability, and whether a subscription has marginal or sunk cost.

The implementable contract should therefore optimize a declared policy such as expected cost subject to capability, deadline, quota, and success constraints, then show the chosen route, alternatives, confidence, and observed outcome.

Farseer should queue when the selected account is exhausted unless a standing policy permits a degraded route, and every automatic downgrade should be visible and reversible.

## Source boundary

The primary sources above verify product capabilities and architectural shapes only.

They do not verify that any reference is safe for Farseer's Windows process model, has equivalent failure semantics, or should be copied as a dependency.

Those decisions remain governed by Farseer's local map, tickets, runner probes, and validation evidence.
