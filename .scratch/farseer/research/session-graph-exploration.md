# Session and graph exploration research

Observed 2026-09-04, with recent-activity checks covering 2026-08-04 through 2026-09-04.

## Scope and evaluation criteria

Farseer needs a visual explorer for harness-owned sessions without redefining `session`, because the locked vocabulary reserves `session` for a runner's conversation with its model and the record is not session history.

The explorer should answer five separate questions without pretending they are one graph: what happened in one run, how runs group into a task, how runs group into a cell and project, which runs are related by shared entities or meaning, and where the source transcript or other attachment can be inspected.

The evaluation criteria are trace fidelity, cross-run grouping, project and cell scoping, semantic search and relationship rendering, self-hostability on a developer machine, license permissiveness, ingestion and export interoperability, raw-transcript custody controls, secret-safety defaults, schema stability, deduplication behavior, embedding controls, and storage cost.

A trace topology is a causal or parent-child structure of observations, usually a tree or DAG, and it is not a knowledge graph or semantic-similarity graph.

A service graph aggregates communication between services and is not sufficient to explain an individual agent run.

A knowledge graph links entities, facts, and provenance, and it is useful for cross-session context but does not prove causal execution order.

A semantic-similarity graph links content vectors by a distance or nearest-neighbor rule, and it must retain the embedding model, dimensions, redaction policy, and evidence pointers to be reproducible.

## Decision matrix

- Langfuse is a trace tree and timeline with session replay rather than a cross-session graph, per its [observability model](https://langfuse.com/docs/observability/data-model.md) and [sessions page](https://langfuse.com/docs/observability/features/sessions.md).
- Langfuse self-hosted Docker, VM, and Kubernetes deployment is documented in the [repository](https://github.com/langfuse/langfuse) and [self-hosting docs](https://langfuse.com/self-hosting).
- Langfuse is MIT outside explicitly separated enterprise directories, per the [license](https://raw.githubusercontent.com/langfuse/langfuse/main/LICENSE).
- Langfuse ingests normalized observations, traces, and sessions through SDKs, OpenTelemetry, and API, with asynchronous batching documented in the [data model](https://langfuse.com/docs/observability/data-model.md).
- Langfuse uses `session_id` to group multiple traces for one interaction, but no documented cross-project session graph exists, per [sessions](https://langfuse.com/docs/observability/features/sessions.md).
- No built-in cross-session semantic graph was found in Langfuse's official data-model or sessions documentation.
- Langfuse session replay can show captured interaction data, but full custody of every harness message or raw PTY stream is undocumented.
- [Langfuse v4.28.1](https://github.com/langfuse/langfuse/releases/tag/v4.28.1) was published 2026-09-03 and includes timeline-row and ClickHouse read-path work.
- Farseer should prototype Langfuse as an export target and UI benchmark, not as its canonical store.

- Arize Phoenix provides an OpenTelemetry span tree and session conversation view rather than a knowledge or semantic graph, per [tracing](https://arize.com/docs/phoenix/tracing/llm-traces.md) and [sessions](https://arize.com/docs/phoenix/tracing/llm-traces/sessions.md).
- Phoenix documents local `pip install arize-phoenix; phoenix serve`, Docker, and Helm deployment in its [repository](https://github.com/Arize-ai/phoenix).
- Phoenix is Elastic License 2.0 with hosted-service and license-key restrictions, per its [license](https://raw.githubusercontent.com/Arize-ai/phoenix/main/LICENSE).
- OpenInference is Apache 2.0 and provides conventions and instrumentations complementary to OpenTelemetry, per its [repository](https://github.com/Arize-ai/openinference).
- Phoenix ingests OTLP, while OpenInference is transport- and file-format agnostic, per [Phoenix tracing](https://arize.com/docs/phoenix/tracing/llm-traces.md) and the [OpenInference README](https://github.com/Arize-ai/openinference).
- Phoenix projects organize traces and a session ID groups related traces into a thread, per [sessions](https://arize.com/docs/phoenix/tracing/llm-traces/sessions.md), but no cross-project semantic session explorer is documented.
- Phoenix supports embedding and retrieval spans, but no official source found a built-in graph linking similar sessions.
- Phoenix can capture inputs and outputs as telemetry, but complete harness transcript custody is unstated and should be treated as unknown.
- [Phoenix v20.7.0](https://github.com/Arize-ai/phoenix/releases/tag/arize-phoenix-v20.7.0) was published 2026-09-03 and [client v3.4.0](https://github.com/Arize-ai/phoenix/releases/tag/arize-phoenix-client-v3.4.0) was published 2026-09-04.
- Farseer should prototype Phoenix or OpenInference OTLP mapping but reject Phoenix as an embedded dependency because ELv2 is a poor fit for a permissive local core.

- AgentOps provides session drilldown, chat history, event charts, and a Session Waterfall that is a timeline rather than a graph of semantically related sessions, per [dashboard docs](https://docs.agentops.ai/v1/usage/dashboard-info.md).
- AgentOps documents a self-hosted dashboard and API, while its app architecture requires ClickHouse and Supabase, per [app README](https://raw.githubusercontent.com/AgentOps-AI/agentops/main/app/README.md).
- AgentOps SDK and repository code are MIT, per the [license](https://raw.githubusercontent.com/AgentOps-AI/agentops/main/LICENSE), while the app README labels the app ELv2, so the app license boundary requires review, per [app README](https://raw.githubusercontent.com/AgentOps-AI/agentops/main/app/README.md).
- AgentOps SDK events and an OTLP exporter feed a backend whose local app stores traces in ClickHouse, and session export REST endpoints are documented in [sessions](https://docs.agentops.ai/v1/concepts/sessions.md) and [app README](https://raw.githubusercontent.com/AgentOps-AI/agentops/main/app/README.md).
- An AgentOps session has a project ID, tags, start and end state, and can be inherited across processes, per [session concepts](https://docs.agentops.ai/v1/concepts/sessions.md).
- AgentOps does not document project-wide or cross-project graph traversal or semantic-similarity edges.
- AgentOps can show exact prompts and completions for recorded LLM events, but complete harness transcript custody and raw-stream policy are unknown.
- The GitHub API exposed no AgentOps commits after 2026-06-25 and no release after 2025-08-29 as checked 2026-09-04, so no qualifying recent activity evidence was found.
- Farseer should use AgentOps only as a UX reference for waterfall and replay, not as a dependency or backend.

- OpenLIT provides a dashboard for normalized OpenTelemetry traces, metrics, costs, and interactions, with no documented cross-session graph, per its [README](https://github.com/openlit/openlit).
- OpenLIT Docker Compose runs ClickHouse and OpenLIT locally with OTLP gRPC and HTTP receiver ports, per the checked-in [compose file](https://raw.githubusercontent.com/openlit/openlit/main/docker-compose.yml).
- OpenLIT is Apache 2.0, per its [license](https://raw.githubusercontent.com/openlit/openlit/main/LICENSE).
- OpenLIT SDKs emit OTLP to an OpenTelemetry Collector and ClickHouse, per the [README architecture](https://github.com/openlit/openlit) and [compose](https://raw.githubusercontent.com/openlit/openlit/main/docker-compose.yml).
- OpenLIT exposes user and session metadata as dashboard dimensions, but no first-party source found project-scoped session replay or a cross-project session graph.
- OpenLIT does not document a semantic-similarity graph, although embeddings and vector databases can be instrumented.
- OpenLIT can render captured trace content, but raw harness-log custody is unspecified and should be treated as unknown.
- [OpenLIT 2.0.0](https://github.com/openlit/openlit/releases/tag/openlit-2.0.0) was published 2026-08-28, and commits on 2026-09-02 and 2026-09-03 added memory-connector and telemetry-destination documentation.
- Farseer should prototype OpenLIT's OTLP receiver shape but not adopt its ClickHouse dashboard as the core.

- OpenLLMetry is instrumentation and semantic telemetry rather than a UI graph, per its [repository](https://github.com/traceloop/openllmetry).
- OpenLLMetry SDKs export to an OpenTelemetry Collector or supported destinations, but OpenLLMetry itself is not a session explorer, per [supported destinations](https://github.com/traceloop/openllmetry).
- OpenLLMetry is Apache 2.0, per the [license](https://raw.githubusercontent.com/traceloop/openllmetry/main/LICENSE).
- OpenLLMetry Python instrumentation emits spans and content events controlled by `TRACELOOP_TRACE_CONTENT`, per [OpenAI utilities](https://raw.githubusercontent.com/traceloop/openllmetry/main/packages/opentelemetry-instrumentation-openai/opentelemetry/instrumentation/openai/utils.py).
- OpenLLMetry does not include a first-party session browser, project graph, or similarity graph.
- Content tracing may include prompts and responses, but custody remains with the selected backend, per [instrumentation source](https://raw.githubusercontent.com/traceloop/openllmetry/main/packages/opentelemetry-instrumentation-openai/opentelemetry/instrumentation/openai/utils.py).
- [OpenLLMetry 0.62.3](https://github.com/traceloop/openllmetry/releases/tag/0.62.3) was published 2026-08-10 with fixes for LiteLLM and OpenAI Agents instrumentation.
- Farseer should adopt only OpenLLMetry's semconv-aware instrumentation ideas through an OTLP boundary.

- Graphiti is a temporal context or knowledge graph of entities, facts, episodes, and provenance rather than a trace DAG, per the [README](https://raw.githubusercontent.com/getzep/graphiti/main/README.md).
- Graphiti self-hosts with Neo4j, FalkorDB, Neptune, or embedded FalkorDB Lite and offers HTTP or stdio MCP, per [installation](https://github.com/getzep/graphiti) and [MCP README](https://raw.githubusercontent.com/getzep/graphiti/main/mcp_server/README.md).
- Graphiti is Apache 2.0, per its [repository](https://github.com/getzep/graphiti) and [license](https://raw.githubusercontent.com/getzep/graphiti/main/LICENSE).
- Graphiti `add_episode` ingests text or JSON, extracts entities and relationships, keeps episode provenance, and supports hybrid semantic, keyword, and graph retrieval, per the [quickstart](https://raw.githubusercontent.com/getzep/graphiti/main/examples/quickstart/quickstart_neo4j.py) and [README](https://github.com/getzep/graphiti/blob/main/README.md).
- Graphiti `group_id` isolates context graphs, but Graphiti leaves user and conversation management to the integrator, per the [README](https://raw.githubusercontent.com/getzep/graphiti/main/README.md).
- Graphiti's hybrid retrieval uses embeddings and graph distance, but it is knowledge retrieval rather than a UI graph of similar harness sessions.
- Graphiti episodes retain raw ingested data as provenance, so callers must scrub before ingestion if they do not want secrets retained, per the [context graph description](https://raw.githubusercontent.com/getzep/graphiti/main/README.md).
- [Graphiti v0.30.0](https://github.com/getzep/graphiti/releases/tag/v0.30.0) was published 2026-09-01 with configured Neo4j database routing and fact-result provenance changes.
- Farseer should prototype Graphiti as an optional derived knowledge projection fed from scrubbed summaries and evidence pointers.

- Mem0 Graph Memory is a native entity and memory graph where shared entities connect memories, with a dashboard graph view on Pro and Enterprise, rather than a trace DAG, per [Graph Memory docs](https://docs.mem0.ai/platform/features/graph-memory.md).
- Mem0 OSS runs as a library or self-hosted Docker server with local Qdrant and SQLite defaults for the library, per [OSS overview](https://docs.mem0.ai/open-source/overview.md).
- Mem0 is Apache 2.0, per its [repository](https://github.com/mem0ai/mem0) and [license](https://raw.githubusercontent.com/mem0ai/mem0/main/LICENSE).
- Mem0 `add` and `search` normalize memories, use vector and keyword retrieval, and the native graph links entities to memories, per [Graph Memory](https://docs.mem0.ai/platform/features/graph-memory.md) and [OSS overview](https://docs.mem0.ai/open-source/overview.md).
- Mem0 user, session, agent, app, and run IDs scope memories, but graph entities are distinct from those IDs, per [Graph Memory](https://docs.mem0.ai/platform/features/graph-memory.md).
- Mem0 provides semantic vector scoring and entity boosts but does not document a graph of similar harness sessions.
- Mem0 stores user-provided content by design, while OSS docs do not establish complete harness-transcript capture, so raw transcript custody is unknown and must not be assumed.
- [Mem0 v2.0.20](https://github.com/mem0ai/mem0/releases/tag/v2.0.20) and [TypeScript v3.1.8](https://github.com/mem0ai/mem0/releases/tag/ts-v3.1.8) were published 2026-09-02.
- Farseer should prototype Mem0 only for an entity-centric memory projection with strict scrubbing and opt-in custody.

- Laminar is the strongest direct UX match found, with transcript, tree, timeline, collapsed subagent cards, metadata, and trace replay, per [trace viewing docs](https://laminar.sh/docs/platform/viewing-traces.md).
- Laminar full OSS self-hosting uses Docker Compose or Helm with frontend, app server, Postgres, ClickHouse, and Quickwit, per [self-hosting](https://laminar.sh/docs/self-hosting/overview.md).
- Laminar is Apache 2.0, per the [license](https://raw.githubusercontent.com/lmnr-ai/lmnr/main/LICENSE.md).
- Laminar documents OTLP ingestion, gRPC export, ClickHouse and Quickwit storage, and trace-level session IDs in the [repository](https://github.com/lmnr-ai/lmnr), [self-hosting docs](https://laminar.sh/docs/self-hosting/overview.md), and [structure docs](https://laminar.sh/docs/tracing/structure/overview.md).
- Laminar sessions group traces and projects are separate, per [structure](https://laminar.sh/docs/tracing/structure/overview.md), but no cross-project semantic session graph exists.
- Laminar signals and full-text search are documented, but no official source describes a vector similarity graph across sessions.
- Laminar's transcript view extracts agent input, turns, tool calls, and subagent previews from captured spans, but complete raw harness custody and secret-scrubbing behavior are deployment-dependent and not fully specified.
- [Laminar v0.2.3](https://github.com/lmnr-ai/lmnr/releases/tag/v0.2.3) was published 2026-08-30, and [v0.2.2](https://github.com/lmnr-ai/lmnr/releases/tag/v0.2.2) added lazy debugger timeline loading and session-table sorting.
- Farseer should prototype Laminar's transcript and collapsed-subagent UX and OTLP export without importing Laminar's storage stack.

## OpenTelemetry and OpenInference boundary

The [OpenTelemetry GenAI events specification](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md) is Development status and says `gen_ai.client.inference.operation.details` can carry chat history and parameters as an opt-in event.

The same specification defines `gen_ai.conversation.id` for a readily available session or thread identifier and explicitly says instrumentations should not invent a UUID, trace ID, or request-content hash when no conversation identifier exists, per the [conversation-id guidance](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md#gen_aiconversationid).

The specification marks `gen_ai.input.messages`, `gen_ai.output.messages`, `gen_ai.system_instructions`, and tool definitions as opt-in and warns that input, output, prompt variables, and system instructions may contain sensitive information, per [content attributes](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md).

The OpenInference specification is complementary to OpenTelemetry, supports LLM invocation, retrieval, and external tools, and is transport and file-format agnostic, per the [OpenInference README](https://github.com/Arize-ai/openinference).

Farseer should version the imported semconv snapshot because the GenAI repository is explicitly in development and its [commit history](https://github.com/open-telemetry/semantic-conventions-genai/commits/main/) shows active changes through 2026-09-03.

## Trace topology versus semantic-similarity graph

The trace projection should preserve causal edges exactly as observed: `parent_id`, `trace_id`, start and end timestamps, actor, operation kind, runner, model, tool, status, usage, and evidence pointers.

A trace may be a tree in the common case and a DAG when links cross process or service boundaries, so the UI should render parent-child edges separately from links introduced by semantic analysis.

A service graph is useful only as an optional aggregate view over many traces and should not replace the per-run trace topology.

The semantic projection should create edges only from an explicit similarity or entity rule and should store `projection_version`, `embedding_model`, `embedding_dimensions`, `distance_metric`, `source_content_digest`, and an evidence pointer back to one or more normalized records.

Graphiti and Mem0 demonstrate that knowledge graphs can connect facts or memories across interactions, but neither provides Farseer's required causal run topology plus cross-project session explorer end to end, and their graph stores are not append-only evidence logs.

No surveyed project delivers cross-project semantic session graph exploration end to end.

Laminar is the best direct visual session debugger, Langfuse and Phoenix are mature trace/session references, Graphiti is the strongest temporal knowledge projection, and Mem0 is the simplest entity-linked memory projection, but they remain separate capabilities.

## Full raw logs versus normalized events and opt-in custody

- Full raw harness logs provide perfect replay of provider-specific frames and context mutations.
- Full raw logs also capture system prompts, secrets, PII, tool payloads, and token streams, couple the record to harness format churn, and inflate storage through retries and streaming fragments.
- Normalized events plus transcript pointers provide small, queryable, stable evidence while allowing adapters to evolve independently, although provider-specific details can be lost and pointers can dangle.
- Normalized events plus an opt-in transcript copy preserve forensic replay while keeping attachment bytes outside the canonical event row, but custody is potentially permanent and unscrubbed.
- Ticket 02 already separates pointer-only reference from opt-in attachment custody.
- This review adds a third, stricter `copy-plus-index` mode because semantic indexing reads transcript content and creates new derived data.
- Farseer should reject full raw logs as the canonical default because [record scope](../issues/02-record-scope.md) says the record is not session history and attachments are out of band.
- Farseer should adopt reference mode by default, permit copy mode per cell or run, and permit copy-plus-index only with explicit operator consent and a documented embedding policy.

Write-time secret scrubbing must apply to normalized event payloads because the repository decision says secrets should never reach disk.
The original copied attachment remains intentionally unscrubbed evidence, while any indexed derivative requires explicit `copy-plus-index` consent and its own redaction policy, per [record scope](../issues/02-record-scope.md) and [scrub implementation](../../../crates/farseer-core/src/scrub.rs).

Normalize only low-cardinality, operationally useful fields into the record, and retain provider-specific payloads behind versioned attachment pointers instead of adding unbounded JSON to every event.

Use a stable source identity composed of `source_system`, `source_event_id`, and canonical-payload digest for idempotent import, while preserving Farseer's own `event_id` and `seq` as the authoritative append order.

Deduplicate only repeated imports with the same source identity and digest, and append a correction or conflicting observation when the source identity repeats with a different digest rather than overwriting history.

Treat embeddings as derived data, not evidence, because changing an embedding model or redaction policy changes nearest neighbors without changing what happened.

Local float32 vectors cost approximately `4 * dimensions` bytes each before ANN index overhead, so a 1536-dimensional vector is about 6 KiB and one million vectors are about 5.7 GiB before metadata and index overhead.

Keep embeddings local or allow an operator-selected endpoint, record the model and redaction version, and never send transcript content to a remote embedder without explicit custody consent.

## Recommended Farseer model and strategy

Keep one physical append-only SQLite log with cell-scoped visibility, UUIDv7 `event_id`, monotonic `seq`, and the existing event shape `{ seq, event_id, ts, cell_id, run_id, kind, actor, payload }`, per [store decision](../issues/09-store-decision.md) and [record scope](../issues/02-record-scope.md).

Add normalized trace linkage as payload fields rather than a new runtime plugin ABI: `trace_id`, `span_id`, `parent_span_id`, `session_ref`, `task_ref`, `canonical_project_path`, `source_system`, `source_event_id`, `source_schema`, and `content_digest`.

Keep `session_ref` a harness-reported pointer and never coin a Farseer session when the runner does not report one, following the [OTel conversation-id rule](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-events.md).

Represent each run as a compact record entry and append progress events for meaningful tool starts, tool results, status changes, steering, intervention, compaction, and terminal result, while leaving token streams and heartbeats as activity only, per [record scope](../issues/02-record-scope.md).

Record transcript pointers by default and permit an operator-selected attachment copy for a run or cell, storing attachment metadata and digest in the normalized event while keeping bytes out of SQLite.

Expose a local OTLP HTTP and gRPC ingestion boundary as an import/export adapter, map OTLP spans and GenAI attributes into normalized events, and preserve unknown attributes in a versioned attachment or bounded metadata field instead of silently dropping evidence.

Export Farseer events as OTLP spans and events for Phoenix, Langfuse, Laminar, OpenLIT, and any collector, and provide a deterministic newline-delimited JSON export that preserves `seq`, Farseer IDs, source IDs, digests, and attachment pointers.

Build the trace UI first as an operator widget over the existing record API, with a topology tab for parent-child edges, a session tab for harness-reported session references, and a project or cell tab for Farseer scope.

Build semantic similarity as a separate, rebuildable projection over scrubbed summaries or explicitly authorized attachments, with local ANN storage and append-only projection metadata that can be discarded and regenerated.

Use Graphiti only as an optional knowledge projection for entity and fact exploration, with scrubbed summaries and evidence pointers, and use Mem0 only if entity-centric memory retrieval is later required; neither should own Farseer's evidence or session identity.

Prototype three surfaces before committing to a backend: Laminar's transcript and collapsed-subagent reading order, Phoenix or OpenInference OTLP interoperability, and a tiny local similarity projection over Farseer summaries with explicit model/version metadata.

Reject AgentOps as a core integration until recent maintenance and self-hosted dependency requirements are clarified, and reject Phoenix as an embedded core because ELv2 conflicts with a permissive local distribution even though its instrumentation remains valuable.

## Explicit unknowns and follow-up checks

No surveyed project documents complete custody semantics for every harness frame, system prompt, encrypted context, raw PTY stream, or secret-bearing tool payload, so Farseer must not infer custody from a UI that displays a prompt or completion.

No surveyed project documents an end-to-end cross-project semantic graph that joins harness sessions, Farseer-like project scope, causal trace topology, and similarity edges in one local open-source product.

The exact retention, deletion, encryption-at-rest, and access-control behavior of self-hosted deployments varies by version and deployment configuration and requires a deployment-specific audit before production use.

The exact deduplication guarantees of OTLP exporters and each backend's retry path are not uniform, so Farseer's source identity and digest must remain authoritative at import.

OpenTelemetry GenAI event and attribute stability is still Development, so Farseer should pin a schema snapshot and maintain an explicit mapping table rather than treating `gen_ai.*` as immutable.

AgentOps has MIT source and a self-host app, but the checked repository metadata shows its latest push in June 2026 and no qualifying activity after 2026-08-04, which is a maintenance risk rather than proof of abandonment.

Mem0's graph view is documented as a Platform feature gated to Pro and Enterprise, while OSS provides the memory engine and self-hosted server, so managed graph screenshots must not be counted as OSS capability, per [Graph Memory availability](https://docs.mem0.ai/platform/features/graph-memory.md).

Graphiti requires a graph database and LLM or embedding provider in the documented setup, so local storage cost and offline operation depend on the selected backend and model, per [installation](https://github.com/getzep/graphiti) and [MCP configuration](https://raw.githubusercontent.com/getzep/graphiti/main/mcp_server/README.md).

## Bottom line

Adopt the normalized append-only event plus pointer model already decided for Farseer, use OTLP and OpenInference as interoperability boundaries, and make transcript custody and embeddings explicit opt-in derived projections.

Prototype Laminar's transcript UX, Phoenix or OpenInference ingestion, and a local similarity projection, while keeping Graphiti and Mem0 optional knowledge and memory projections rather than canonical session stores.

Do not adopt any surveyed product wholesale, and do not claim that an existing project already provides cross-project semantic session graph exploration end to end.