# Farseer core contract

Status: proposed rework baseline for the headless runtime.
Scope: durable domain rules, process boundaries, routing, record evidence, and extension seams.
UI layout, colors, widgets, and interaction polish are outside this document.

## Purpose

Farseer is a local-first orchestration runtime for Windows.
It coordinates cells, runners, workers, peer orchestrators, projects, terminals, and observations through one authenticated local API.
The runtime is the product boundary and every client is replaceable.

## Domain model

- A cell is a stable address, manager policy, roster, workspace strategy, record scope, and budget.
- A manager makes decisions for one cell and may call only declared roster entries.
- A runner is an adapter for one installed harness or provider face.
- A worker is a supervised run with an immutable contract and one owning cell.
- A cell call is a parent-linked request to another cell or peer orchestrator.
- A project is an authorized workspace root with an optional team profile.
- A conversation groups durable operator intent across tasks and runs.
- A task is the durable unit shown in boards and analytics.
- A session is provider-owned identity attached to a run, never a second source of truth.
- An event is runtime-observed evidence; projections are rebuildable views.

## Invariants

1. `farseer-core` stays pure; clocks, filesystem, process, network, and persistence enter through arguments or outer adapters.
2. A run contract is sealed before spawn; changing policy, runner, workspace, or authority creates a new run.
3. Capabilities are observed and versioned; missing, unsupported, and not reported stay distinct.
4. Authority is explicit; roster, tool level, autonomy ceiling, budget, workspace, and record scope are checked before side effects.
5. The record is append-only and runtime-owned; agents and clients cannot forge events or rewrite history.
6. Projections are bounded and disposable; every projection carries a version and source scope.
7. External protocols stop at adapters; ACP, A2A, MCP, terminals, and provider APIs do not become domain types.
8. Optional features degrade to explicit unavailable states without changing core behavior.
9. Core types contain no cosmetic UI concepts; widget ids, panes, CSS, and layout blobs belong to clients.
10. Compatibility is additive and explicit through versioned envelopes or optional fields with a removal plan.

## Rework trigger

`InstructBody` mixes `project`, `conversation_id`, `task_id`, `manager_runner`, and `OperatorAnchor` beside the goal.
`OperatorAnchor` contains widget-facing data, so a changing UI shapes the runtime request.
`manager_runner` is a string while the cell, project team, and future router need a policy and route explanation.
Model and reasoning live in runner configuration but are not part of a sealed request plan.
Operator, A2A, MCP, maintenance, and artifact paths assemble tasks, runs, and events independently.
The current surface works, but every new client or feature crosses API, store, manager, and UI code.

Rework the application boundary around one semantic instruction path.
Keep storage and event vocabulary where they already satisfy the invariants.
Do not rewrite the runtime into a plugin host or replace SQLite without measured failure.

## Semantic instruction

Introduce a domain-level `Instruction` assembled by adapters.
It contains `target_cell`, `goal`, authorized `project`, optional `conversation`, optional `task`, optional `dispatch`, semantic `origin`, and versioned namespaced `extensions`.
`DispatchPreference` contains optional harness, runner, model, reasoning effort, cost policy, and capability requirements.
Absent values mean automatic selection; requested values are constraints, not promises.
Unsupported requested capabilities fail before a task or run is created.

UI anchor data stays in an API-side provenance extension.
A widget or pane name must never enter the worker contract or routing algorithm.
The runtime may record a scrubbed origin label for audit, but core decisions use semantic fields only.

## Route plan

Add a pure `RoutePlan` produced before spawn.
It contains selected cell, runner, model, reasoning effort, workspace strategy, tool level, capabilities, budget allocation, and ordered selection or rejection reasons.
The plan is immutable after sealing.
`run_queued` stores the contract and `routing_sealed` stores the explanation and capability snapshot.
The same planner serves operator instructions, manager delegation, cell calls, maintenance, and future automation.
Cost policy is explicit and deterministic before learned or semantic routing is considered.
Unknown prices remain unknown rather than becoming a fabricated ranking.

## Application services

Create one service per semantic command family.
- `InstructionService` resolves project, conversation, task, dispatch, route plan, and run creation.
- `DelegationService` validates manager identity, roster authority, budget, and parent linkage.
- `RunControlService` owns cancel, steer, rerun, rescope, attach, and intervention provenance.
- `ObservationService` owns quota, resource, terminal, transcript, and capability observations.
- `MaintenanceService` turns drift into an ordinary task and bounded proposal.

HTTP, MCP, A2A, CLI, and future clients translate into these services.
They do not create tasks, runs, events, or budgets directly.
This removes duplication between operator instruct, artifact jobs, A2A, and manager transports.

## Capability and feature seams

Core-critical modules are cell validation, policy, routing, contract sealing, supervision, record writing, authentication, and recovery.
An unavailable core-critical module blocks the operation with a typed error.
Everything else is an adapter or observer.

Each optional feature exposes a versioned descriptor with stable id and version, commands and projections, required capabilities, authority and data scope, unavailable states, and migration/removal behavior.
Registration is in-process and explicit for now.
Do not add a dynamic plugin ABI, marketplace, or arbitrary code loading until two real integrations require it.
A future plugin host may implement the same descriptor contract without changing the domain model.
UI widgets remain clients of read projections and named bridge verbs.

## Projects and multi-harness teams

A project profile declares its coordinating cell, specialist cells, allowed runner candidates, and policy ceilings.
The profile is resolved and snapshotted into each task and route plan.
A project may use one harness or many without changing task identity.
Specialist calls remain parent-linked runs with their own cell authority and budgets.
The router chooses only nominated candidates and never infers equivalence from names or price.

## Terminal and observations

Terminal profiles are named capabilities such as PowerShell, cmd, Git Bash, and an open shell.
A terminal adapter owns process creation, input, resize, output, and teardown.
The core sees a profile id and observation stream, never a PTY implementation.
Resource and quota data carry source, units, timestamp, freshness, and scope.
Unavailable data is returned as unavailable rather than zero.

## Self-maintenance

Farseer may monitor its binaries, cells, runners, schemas, projections, and evidence checks.
Monitoring produces observations and ordinary maintenance tasks.
A maintenance proposal includes candidate, reproducer, branch or worktree, validation evidence, and rollback metadata.
Promotion is a separate explicit command.
An agent cannot silently rewrite the running core, install a plugin, or promote its own change.
The maintenance cell is a regular cell with narrower authority.

## Record and projections

Every side effect has actor, cell, run or task linkage, timestamp, event kind, payload version, and redaction policy.
Adapter event kinds remain open, while core lifecycle events stay named and tested.
Payloads are versioned independently so one feature can evolve without a global schema rewrite.
SQLite remains canonical until a measured workload disproves it.
Analytics, similarity, graph, kanban, session search, usage, and telemetry are projections over the same record.
Similarity must preserve source, model, version, and confidence.

## Rework sequence

1. `C1 instruction envelope` - add semantic instruction and compatibility decoding for the current API body.
2. `C2 route plan` - centralize runner, model, reasoning, cost, capability, and project-team selection.
3. `C3 application services` - route all ingress paths through shared services.
4. `C4 provenance split` - keep UI origin outside contracts and routing while retaining audit records.
5. `C5 feature descriptors` - define optional registration and unavailable behavior without dynamic loading.
6. `C6 maintenance loop` - make self-observation and proposals use the same services as other projects.
7. `C7 client migration` - migrate the desktop surface and future clients to semantic requests and projections.
8. `C8 removal and proof` - remove deprecated fields after compatibility tests and full workspace gates.

## Acceptance

- Core tests cover route selection, capability narrowing, budget composition, and contract immutability without I/O.
- Operator, MCP, A2A, manager, artifact, and maintenance instructions use the same semantic command path.
- A request may omit project, runner, model, reasoning, and UI origin when policy permits automatic selection.
- Unsupported capabilities fail before task or run creation.
- Every run records the sealed route plan and capability snapshot.
- Removing an optional feature leaves core startup and core commands usable.
- A maintenance proposal cannot promote or mutate the running binary without an explicit operator command.
- Deprecated fields have compatibility tests and a removal ticket.

## Non-goals

- No GUI layout contract in core.
- No learned router or embeddings before route and outcome data are reliable.
- No general third-party MCP gateway.
- No cloud execution or remote project authority.
- No dynamic plugin ABI until concrete integrations justify it.
- No second canonical store for sessions, tasks, analytics, or UI state.