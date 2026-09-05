# Work model and session explorer

Type: decision
Status: closed
Blocked by: none

## Question

How does farseer represent durable operator conversations, task boards, harness sessions, and a visual graph without turning transcripts or a graph engine into the source of truth?

## Resolution

Resolved 2026-09-04 after the operator UX review and session-graph research.

### Work

A **conversation** is the durable operator-visible thread that groups tasks.
A **task** remains the operator's whole request and belongs to exactly one conversation.
A task carries an optional canonical `project_path` snapshot because a project is a discovered directory, not a durable entity.
A run belongs to one task and may name one parent run when delegation created it.

Task board states are `inbox`, `planned`, `in_progress`, `blocked`, `review`, `done`, and `cancelled`.
The global and project kanbans are filtered projections over the same tasks, never separate stores.
Every transition records the actor and reason through a validated runtime command.

### Harness sessions

A harness session is observed and never coined by farseer.
A run may reference several harness sessions and each reference preserves the provider's identifier kind.
Harness-native subagents remain nested observations unless farseer created and supervises a distinct worker run.

### Transcript custody

`reference` records a harness log pointer and copies no transcript bytes.
`copy` adds an operator-authorized content-addressed attachment outside the event log.
`copy-plus-index` additionally creates scrubbed, versioned search and similarity projections.
The canonical record never contains a raw transcript.

### Explorer

The explorer is a Work widget face with a full-canvas expansion.
Observed topology and derived similarity are separate layers.
Topology contains project path, conversation, task, run, harness session, delegation, cell-call, rescope, and continuation edges.
Similarity edges carry score, embedding model, dimensions, distance metric, redaction version, projection version, source digest, and evidence pointers.
Embeddings and similarity edges are rebuildable projections rather than evidence.
SQLite edge tables and recursive CTEs remain the graph store until measured production queries exceed the existing store decision's limits.

### Manager selection

The top manager remains the only recipient of operator requests.
Cell zero declares an ordered candidate set and each conversation pins the candidate used for new manager runs.
Changing the selected candidate starts a new manager run and harness session without claiming session migration.

### Extension seam

The runtime loads no plugin ABI.
Runner adapters report observations and attachment pointers.
OTLP and OpenInference are import and export adapters.
First-party widgets own operational views and agent-authored widgets remain sandboxed presentation only.

## Verification consequences

- One conversation can contain tasks and runs from several harnesses.
- One task appears identically on global and project boards.
- A dangling harness-log pointer never breaks record replay.
- Reference mode never copies bytes and copy mode never indexes them.
- Every similarity edge identifies the projection that produced it.
- No visual or authored widget can append raw events or address a manager other than the top manager.

## Sources

- [Operator UX and orchestration review](../operator-ux-orchestration-review.md)
- [Session and graph exploration research](../research/session-graph-exploration.md)
- [Record scope](02-record-scope.md)
- [Store decision](09-store-decision.md)
- [Vocabulary lock](14-vocabulary-lock.md)
- [Operator surface](28-operator-surface.md)
