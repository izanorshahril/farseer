import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Bridge } from "../bridge";
import { onSubjectSelection, selectSubject, selectedSubject } from "../selection";
import { follow } from "../stream";
import { mask, usePrivacy } from "../privacy";
import { ReadFailure } from "../ReadFailure";

type TaskState = "inbox" | "planned" | "in_progress" | "blocked" | "review" | "done" | "cancelled";
type Task = {
  task_id: string;
  conversation_id: string;
  goal: string;
  title: string;
  project_path?: string;
  state: TaskState;
  priority: number;
  updated_ts: number;
};
type TaskPage = {
  tasks: Task[];
  next_cursor?: string;
  has_more: boolean;
  freshness: "eventual";
  generated_ts: number;
};
type Conversation = {
  conversation_id: string;
  title: string;
  project_path?: string;
  manager_runner?: string;
  updated_ts: number;
  archived_ts?: number;
};
type Run = {
  run_id: string;
  runner: string;
  outcome?: string;
  model?: string;
  usd_micros?: number;
  tokens?: number;
  duration_ms?: number;
  cost_basis?: "reported" | "estimated" | "unknown";
};
type Session = { run_id: string; identifier_kind: string; identifier: string; log_pointer?: string };
type SessionRow = { session: Session & { observed_ts: number }; task_id: string; runner: string; model: string; project_path?: string; log_available: boolean };
type SessionPage = { rows: SessionRow[]; next_offset?: number };
type SearchHit = { digest: string; excerpt: string; coverage: string; projection_version?: string };
type SearchPage = { rows: SearchHit[]; next_offset?: number };
type Projection = { status: "pending" | "complete" | "truncated" | "failed" | "cancelled"; error?: string; coverage: string; updated_ts: number };
type Attachment = { digest: string; run_id: string; custody: string; source: string; projection?: Projection };
type Artifact = { artifact_id: string; run_id: string; kind: string; status: string; input_path: string; staged_path: string; final_path?: string; error?: string; created_ts: number; finished_ts?: number };
type TaskUsage = {
  scope: "task";
  runs: number;
  successful_runs: number;
  failed_runs: number;
  tokens: number;
  usd_micros: number;
  reported_usd_micros: number;
  estimated_usd_micros: number;
  duration_ms: number;
  cost_basis: "reported" | "estimated" | "mixed" | "unknown";
};
type TaskDetail = { task: Task; usage?: TaskUsage; allowed_transitions: TaskState[]; runs: Run[]; sessions: Session[]; attachments: Attachment[]; artifacts?: Artifact[]; transitions: { from: TaskState; to: TaskState; actor: string; reason: string; ts: number }[] };
type GraphNode = { id: string; kind: string; label: string; project_path?: string; runner?: string; target?: string; parent?: string };
type GraphEdge = { from: string; to: string; kind: string; source?: string; projection?: string; score?: number; evidence: string[] };
type Graph = {
  nodes: GraphNode[];
  observed_edges: GraphEdge[];
  derived_edges: GraphEdge[];
  next_cursor?: string;
  has_more: boolean;
  freshness: "eventual";
  generated_ts: number;
};
type Cell = { manager: { runners: string[] } };
type Face = "board" | "conversations" | "sessions" | "search" | "graph" | "completed";

const STATES: TaskState[] = ["inbox", "planned", "in_progress", "blocked", "review", "done", "cancelled"];

const short = (value: string) => value.slice(0, 8);
const stateLabel = (state: TaskState) => state.replace("_", " ");

export function WorkWidget({ bridge }: { bridge: Bridge }) {
  const privacy = usePrivacy();
  const [face, setFace] = useState<Face>("board");
  const [expanded, setExpanded] = useState(false);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [nextCursor, setNextCursor] = useState<string | undefined>();
  const [loadingMore, setLoadingMore] = useState(false);
  const loadVersion = useRef(0);
  const [projectScope, setProjectScope] = useState("");
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [sessionOffset, setSessionOffset] = useState<number | undefined>();
  const [sessionsLoading, setSessionsLoading] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchRows, setSearchRows] = useState<SearchHit[]>([]);
  const [searchOffset, setSearchOffset] = useState<number | undefined>();
  const [searchLoading, setSearchLoading] = useState(false);
  const [graph, setGraph] = useState<Graph | null>(null);
  const [graphProject, setGraphProject] = useState("");
  const [graphRunner, setGraphRunner] = useState("");
  const [graphLoading, setGraphLoading] = useState(false);
  const [subject, setSubject] = useState(selectedSubject());
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [runners, setRunners] = useState<string[]>([]);
  const [newTitle, setNewTitle] = useState("");
  const [transcriptPath, setTranscriptPath] = useState("");
  const [transcriptMode, setTranscriptMode] = useState("reference");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    const version = ++loadVersion.current;
    const params = new URLSearchParams({ limit: "100" });
    if (projectScope) params.set("project", projectScope);
    const [nextTasks, nextConversations, cell] = await Promise.all([
      bridge.read<TaskPage>(`/tasks/page?${params}`),
      bridge.read<Conversation[]>("/conversations?limit=500"),
      bridge.read<Cell>("/cells/zero"),
    ]);
    if (version !== loadVersion.current) return;
    setTasks(nextTasks.tasks);
    setNextCursor(nextTasks.next_cursor);
    setConversations(nextConversations);
    setRunners(cell.manager.runners);
    const selectedTask = selectedSubject().task;
    if (selectedTask) {
      const selected = await bridge.read<TaskDetail>(`/tasks/${selectedTask}`);
      if (version === loadVersion.current && selectedSubject().task === selectedTask) setDetail(selected);
    }
    setError(null);
  }, [bridge, projectScope]);

  const loadMore = async () => {
    if (!nextCursor || loadingMore) return;
    const version = loadVersion.current;
    setLoadingMore(true);
    try {
      const params = new URLSearchParams({ limit: "100", cursor: nextCursor });
      if (projectScope) params.set("project", projectScope);
      const page = await bridge.read<TaskPage>(`/tasks/page?${params}`);
      if (version !== loadVersion.current) return;
      setTasks((current) => [...current, ...page.tasks]);
      setNextCursor(page.next_cursor);
    } finally {
      if (version === loadVersion.current) setLoadingMore(false);
    }
  };

  useEffect(() => {
    load().catch((failure: Error) => setError(failure.message));
    let timer: ReturnType<typeof setTimeout> | undefined;
    const subscription = follow(() => {
      clearTimeout(timer);
      timer = setTimeout(() => load().catch((failure: Error) => setError(failure.message)), 250);
    });
    return () => {
      clearTimeout(timer);
      subscription.close();
    };
  }, [load]);

  const loadGraph = useCallback(async (cursor?: string) => {
    setGraphLoading(true);
    try {
      const params = new URLSearchParams({ limit: "100", edge_limit: "300" });
      if (graphProject) params.set("project", graphProject);
      if (graphRunner) params.set("runner", graphRunner);
      if (cursor) params.set("cursor", cursor);
      const next = await bridge.read<Graph>(`/work/graph?${params}`);
      setGraph((current) => cursor && current ? { ...next, nodes: [...current.nodes, ...next.nodes], observed_edges: [...current.observed_edges, ...next.observed_edges], derived_edges: [...current.derived_edges, ...next.derived_edges] } : next);
      setError(null);
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setGraphLoading(false);
    }
  }, [bridge, graphProject, graphRunner]);

  const loadSessions = useCallback(async (offset = 0) => {
    setSessionsLoading(true);
    try {
      const params = new URLSearchParams({ limit: "100", offset: String(offset) });
      if (projectScope) params.set("project", projectScope);
      const page = await bridge.read<SessionPage>(`/work/sessions?${params}`);
      setSessions((current) => offset ? [...current, ...page.rows] : page.rows);
      setSessionOffset(page.next_offset);
      setError(null);
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setSessionsLoading(false);
    }
  }, [bridge, projectScope]);

  const loadSearch = useCallback(async (offset = 0) => {
    if (!searchQuery.trim()) {
      setSearchRows([]);
      setSearchOffset(undefined);
      return;
    }
    setSearchLoading(true);
    try {
      const params = new URLSearchParams({ q: searchQuery.trim(), limit: "50", offset: String(offset) });
      const page = await bridge.read<SearchPage>(`/work/search/page?${params}`);
      setSearchRows((current) => offset ? [...current, ...page.rows] : page.rows);
      setSearchOffset(page.next_offset);
      setError(null);
    } catch (failure) {
      setError((failure as Error).message);
    } finally {
      setSearchLoading(false);
    }
  }, [bridge, searchQuery]);

  useEffect(() => {
    if (face !== "graph") return;
    setGraph(null);
    loadGraph().catch(() => undefined);
  }, [face, graphProject, graphRunner, loadGraph]);

  useEffect(() => {
    if (face !== "sessions") return;
    setSessions([]);
    setSessionOffset(undefined);
    void loadSessions();
  }, [face, projectScope, loadSessions]);

  useEffect(() => {
    if (face !== "search") return;
    setSearchRows([]);
    setSearchOffset(undefined);
    void loadSearch();
  }, [face, loadSearch]);

  useEffect(() => onSubjectSelection(setSubject), []);
  useEffect(() => {
    if (!subject.task) {
      setDetail(null);
      return;
    }
    const taskId = subject.task;
    bridge.read<TaskDetail>(`/tasks/${taskId}`).then((next) => {
      if (selectedSubject().task === taskId) setDetail(next);
    }).catch((failure: Error) => setError(failure.message));
  }, [bridge, subject.task]);

  const projectPaths = useMemo(
    () => [...new Set(tasks.flatMap((task) => task.project_path ? [task.project_path] : []))].sort(),
    [tasks],
  );
  const visibleTasks = useMemo(
    () => projectScope ? tasks.filter((task) => task.project_path === projectScope) : tasks,
    [projectScope, tasks],
  );
  const grouped = useMemo(
    () => Object.fromEntries(STATES.map((state) => [state, visibleTasks.filter((task) => task.state === state)])) as Record<TaskState, Task[]>,
    [visibleTasks],
  );

  const chooseTask = (task: Task) => {
    selectSubject({ conversation: task.conversation_id, task: task.task_id, project: task.project_path ?? null, run: null });
  };

  const transition = async (state: TaskState) => {
    if (!detail) return;
    const taskId = detail.task.task_id;
    await bridge.post(`/tasks/${taskId}/transition`, {
      state,
      reason: `Moved from ${stateLabel(detail.task.state)} to ${stateLabel(state)} in Work`,
    });
    await load();
    if (selectedSubject().task === taskId) setDetail(await bridge.read<TaskDetail>(`/tasks/${taskId}`));
  };

  const addTranscript = async () => {
    const run = detail?.runs.at(-1);
    if (!run || !transcriptPath.trim()) return;
    const taskId = detail!.task.task_id;
    await bridge.post(`/runs/${run.run_id}/transcripts`, { mode: transcriptMode, path: transcriptPath.trim() });
    setTranscriptPath("");
    if (selectedSubject().task === taskId) setDetail(await bridge.read<TaskDetail>(`/tasks/${taskId}`));
    await load();
  };

  const updateTranscript = async (attachment: Attachment, action: "retry" | "cancel") => {
    await bridge.post(`/runs/${attachment.run_id}/transcripts/${attachment.digest}/${action}`, {});
    const taskId = detail?.task.task_id;
    if (taskId && selectedSubject().task === taskId) setDetail(await bridge.read<TaskDetail>(`/tasks/${taskId}`));
  };

  const createConversation = async () => {
    if (!newTitle.trim()) return;
    const conversation = (await bridge.post("/conversations", {
      title: newTitle.trim(),
      project: subject.project,
      manager_runner: subject.managerRunner,
    })) as Conversation;
    selectSubject({ conversation: conversation.conversation_id, task: null, run: null, project: conversation.project_path ?? null, managerRunner: conversation.manager_runner ?? null });
    setNewTitle("");
    await load();
  };

  return (
    <div className={`work-panel${expanded ? " expanded" : ""}`}>
      <div className="work-toolbar">
        <div role="tablist" aria-label="Work faces">
          {(["board", "conversations", "sessions", "search", "graph", "completed"] as Face[]).map((name) => (
            <button key={name} className={face === name ? "chip on" : "chip"} role="tab" aria-selected={face === name} onClick={() => setFace(name)}>
              {name}
            </button>
          ))}
        </div>
        {face === "board" && (
          <select
            aria-label="project board"
            value={projectScope}
            onChange={(event) => setProjectScope(event.currentTarget.value)}
          >
            <option value="">all projects</option>
            {projectPaths.map((project) => <option key={project} value={project}>{project}</option>)}
          </select>
        )}
        {face === "graph" && (
          <>
            <select aria-label="graph project" value={graphProject} onChange={(event) => setGraphProject(event.currentTarget.value)}>
              <option value="">all projects</option>
              {projectPaths.map((project) => <option key={project} value={project}>{mask(project, "path", privacy)}</option>)}
            </select>
            <select aria-label="graph runner" value={graphRunner} onChange={(event) => setGraphRunner(event.currentTarget.value)}>
              <option value="">all runners</option>
              {runners.map((runner) => <option key={runner}>{runner}</option>)}
            </select>
          </>
        )}
        {face === "search" && (
          <form onSubmit={(event) => { event.preventDefault(); void loadSearch(); }} className="row">
            <input aria-label="search indexed transcripts" value={searchQuery} onChange={(event) => setSearchQuery(event.currentTarget.value)} placeholder="search scrubbed transcripts" />
            <button className="chip on" disabled={searchLoading || !searchQuery.trim()}>search</button>
          </form>
        )}
        <button className="chip" aria-pressed={expanded} onClick={() => setExpanded((current) => !current)}>{expanded ? "restore" : "expand"}</button>
      </div>
      {error && <ReadFailure
        capability={`work ${face}`}
        error={error}
        stale
        onRetry={() => {
          if (face === "graph") void loadGraph().catch(() => undefined);
          else if (face === "sessions") void loadSessions().catch(() => undefined);
          else void load().catch(() => undefined);
        }}
        onReduceScope={projectScope ? () => setProjectScope("") : undefined}
      />}

      {face === "board" && (
        <>
          <div className="work-board">
            {STATES.filter((state) => state !== "done" && state !== "cancelled").map((state) => (
              <section key={state} className="work-column" aria-label={stateLabel(state)}>
                <h4>{stateLabel(state)} <span>{grouped[state].length}</span></h4>
                {grouped[state].map((task) => (
                  <button key={task.task_id} className={subject.task === task.task_id ? "work-card selected" : "work-card"} onClick={() => chooseTask(task)}>
                    <b>{task.title}</b><small>{task.project_path ? mask(task.project_path, "path", privacy) : "fleet"}</small>
                  </button>
                ))}
              </section>
            ))}
          </div>
          {nextCursor && <button className="chip" onClick={() => loadMore().catch((failure: Error) => setError(failure.message))} disabled={loadingMore}>{loadingMore ? "loading..." : "load more"}</button>}
        </>
      )}

      {face === "conversations" && (
        <div className="work-conversations">
          <form onSubmit={(event) => { event.preventDefault(); createConversation().catch((failure: Error) => setError(failure.message)); }}>
            <input aria-label="new conversation title" value={newTitle} onChange={(event) => setNewTitle(event.currentTarget.value)} placeholder="new conversation" />
            <button className="chip on" disabled={!newTitle.trim()}>create</button>
          </form>
          <ul className="plain-list">
            {conversations.map((conversation) => (
              <li key={conversation.conversation_id}>
                <button className={subject.conversation === conversation.conversation_id ? "row-button selected" : "row-button"} onClick={() => selectSubject({ conversation: conversation.conversation_id, task: null, run: null, project: conversation.project_path ?? null, managerRunner: conversation.manager_runner ?? null })}>
                  <b>{conversation.title}</b><small>{conversation.project_path ? mask(conversation.project_path, "path", privacy) : "fleet"}</small><span className="mono">{mask(short(conversation.conversation_id), "session", privacy)}</span>
                </button>
              </li>
            ))}
          </ul>
          {subject.conversation && runners.length > 0 && (
            <label className="runner-picker">manager for next request
              <select value={subject.managerRunner ?? conversations.find((conversation) => conversation.conversation_id === subject.conversation)?.manager_runner ?? runners[0]} onChange={(event) => selectSubject({ managerRunner: event.currentTarget.value })}>
                {runners.map((runner) => <option key={runner}>{runner}</option>)}
              </select>
            </label>
          )}
        </div>
      )}

      {face === "sessions" && (
        <div className="work-sessions">
          {sessions.length === 0 && !sessionsLoading && <p className="empty">No harness sessions observed.</p>}
          <ul className="plain-list">
            {sessions.map((row) => (
              <li key={`${row.session.identifier_kind}:${row.session.identifier}:${row.session.run_id}`}>
                <button className="row-button" onClick={() => selectSubject({ task: row.task_id, run: row.session.run_id, project: row.project_path ?? null })}>
                  <b>{mask(row.session.identifier, "session", privacy)}</b>
                  <small>{row.runner} · {row.model || "model unavailable"} · {row.log_available ? "log available" : "log unavailable"}</small>
                  <span className="mono">{row.session.identifier_kind} · {new Date(row.session.observed_ts).toLocaleString()}</span>
                </button>
              </li>
            ))}
          </ul>
          {sessionOffset !== undefined && <button className="chip" onClick={() => void loadSessions(sessionOffset)} disabled={sessionsLoading}>{sessionsLoading ? "loading..." : "load more sessions"}</button>}
        </div>
      )}

      {face === "search" && (
        <div className="work-search">
          {searchRows.length === 0 && !searchLoading && <p className="empty">Search indexed transcript excerpts.</p>}
          <ul className="plain-list">
            {searchRows.map((hit) => (
              <li key={hit.digest} className="row-button">
                <b className="mono">{mask(short(hit.digest), "session", privacy)}</b>
                <small>{hit.coverage} · projection {hit.projection_version ?? "not stated"}</small>
                <span>{hit.excerpt}</span>
              </li>
            ))}
          </ul>
          {searchOffset !== undefined && <button className="chip" onClick={() => void loadSearch(searchOffset)} disabled={searchLoading}>{searchLoading ? "searching..." : "load more excerpts"}</button>}
        </div>
      )}

      {face === "completed" && (
        <div className="completed-work">
          {[...grouped.done, ...grouped.cancelled].map((task) => (
            <button key={task.task_id} className="row-button" onClick={() => chooseTask(task)}><b>{task.title}</b><span className={`badge ${task.state === "cancelled" ? "bad" : ""}`}>{task.state}</span></button>
          ))}
          {grouped.done.length + grouped.cancelled.length === 0 && <p className="empty">No completed work yet.</p>}
        </div>
      )}

      {face === "graph" && graphLoading && !graph && <p className="empty">Loading bounded graph...</p>}
      {face === "graph" && graph && <WorkGraph graph={graph} privacy={privacy} onSelect={(node) => {
        if (!node.target) return;
        if (node.kind === "conversation") selectSubject({ conversation: node.target, task: null, run: null });
        if (node.kind === "task") selectSubject({ task: node.target });
        if (node.kind === "run" || node.kind === "session" || node.kind === "attachment") selectSubject({ run: node.target });
      }} />}
      {face === "graph" && graph?.next_cursor && <button className="chip" onClick={() => loadGraph(graph.next_cursor)} disabled={graphLoading}>{graphLoading ? "loading..." : "load more graph"}</button>}

      {detail && (
        <aside className="task-detail" aria-label="Selected task detail">
          <div className="row"><b>{detail.task.title}</b><span className="badge">{stateLabel(detail.task.state)}</span><button className="chip" onClick={() => selectSubject({ task: null, run: null })}>close</button></div>
          <p>{detail.task.goal}</p>
          {detail.usage && <div className="meta" aria-label="task usage">
            <span><i>usage scope</i><b>task</b></span>
            <span><i>runs</i><b>{detail.usage.runs}</b></span>
            <span><i>tokens</i><b>{detail.usage.tokens.toLocaleString()}</b></span>
            <span><i>cost</i><b>${(detail.usage.usd_micros / 1_000_000).toFixed(4)}</b></span>
            <span><i>cost basis</i><b>{detail.usage.cost_basis}</b></span>
            <span><i>duration</i><b>{(detail.usage.duration_ms / 1000).toFixed(1)}s</b></span>
          </div>}
          <div className="task-actions">{detail.allowed_transitions.map((state) => <button key={state} className="chip" onClick={() => transition(state).catch((failure: Error) => setError(failure.message))}>{stateLabel(state)}</button>)}</div>
          <div className="task-runs">{detail.runs.map((run) => <button key={run.run_id} className="chip" onClick={() => selectSubject({ run: run.run_id })}>{short(run.run_id)} · {run.runner} · {run.model ?? "model not reported"} · {run.outcome ?? "running"}</button>)}</div>
          {detail.artifacts?.map((artifact) => <p key={artifact.artifact_id} className="mono small">{artifact.kind} · {artifact.status} · {mask(artifact.input_path, "path", privacy)}{artifact.error ? ` · ${artifact.error}` : ""}</p>)}
          {detail.sessions.map((session) => <p key={`${session.identifier_kind}:${session.identifier}`} className="mono small">{session.identifier_kind} {mask(session.identifier, "session", privacy)}{session.log_pointer ? ` · ${mask(session.log_pointer, "path", privacy)}` : ""}</p>)}
          <form className="transcript-form" onSubmit={(event) => { event.preventDefault(); addTranscript().catch((failure: Error) => setError(failure.message)); }}>
            <select aria-label="transcript custody" value={transcriptMode} onChange={(event) => setTranscriptMode(event.currentTarget.value)}><option>reference</option><option>copy</option><option>copy-plus-index</option></select>
            <input aria-label="transcript file path" value={transcriptPath} onChange={(event) => setTranscriptPath(event.currentTarget.value)} placeholder="harness transcript path" />
            <button className="chip" disabled={!detail.runs.length || !transcriptPath.trim()}>attach</button>
          </form>
          {detail.attachments.map((attachment) => <p key={attachment.digest} className="mono small">
            {attachment.custody} · {mask(short(attachment.digest), "session", privacy)} · {mask(attachment.source, "path", privacy)}
            {attachment.projection && <> · {attachment.projection.status}{attachment.projection.error ? `: ${attachment.projection.error}` : ""}
              {attachment.projection.status === "pending" && <button className="chip" onClick={() => updateTranscript(attachment, "cancel").catch((failure: Error) => setError(failure.message))}>cancel analysis</button>}
              {(attachment.projection.status === "failed" || attachment.projection.status === "cancelled") && <button className="chip" onClick={() => updateTranscript(attachment, "retry").catch((failure: Error) => setError(failure.message))}>retry analysis</button>}
            </>}
          </p>)}
        </aside>
      )}
    </div>
  );
}

function WorkGraph({ graph, privacy, onSelect }: { graph: Graph; privacy: boolean; onSelect: (node: GraphNode) => void }) {
  const [zoom, setZoom] = useState(1);
  const nodes = graph.nodes;
  const rows = Math.max(2, Math.ceil(nodes.length / 6));
  const height = 100 + rows * 90;
  const at = new Map(nodes.map((node, index) => [node.id, { x: 90 + (index % 6) * 150, y: 50 + Math.floor(index / 6) * 90 }]));
  const observed = graph.observed_edges;
  const derived = graph.derived_edges;
  return (
    <div className="work-graph">
      <div className="graph-legend"><span>observed topology</span><span className="derived">derived similarity</span><span><button className="chip" onClick={() => setZoom((value) => Math.max(0.6, value - 0.2))}>-</button><button className="chip" onClick={() => setZoom((value) => Math.min(2, value + 0.2))}>+</button><button className="chip" onClick={() => setZoom(1)}>reset</button></span></div>
      <svg viewBox={`0 0 950 ${height}`} style={{ width: `${zoom * 100}%` }} role="img" aria-label="Conversation, task, run, harness session, transcript, delegation, cell call, rescope, continuation, and similarity graph">
        {derived.map((edge, index) => { const from = at.get(edge.from); const to = at.get(edge.to); return from && to ? <line key={`${edge.from}:${edge.to}:${index}`} x1={from.x} y1={from.y} x2={to.x} y2={to.y} className="derived-edge"><title>{`${edge.score?.toFixed(2) ?? ""} ${edge.projection ?? ""}`}</title></line> : null; })}
        {observed.map((edge, index) => { const from = at.get(edge.from); const to = at.get(edge.to); return from && to ? <line key={`${edge.from}:${edge.to}:${index}`} x1={from.x} y1={from.y} x2={to.x} y2={to.y} className="observed-edge"><title>{edge.kind}</title></line> : null; })}
        {nodes.map((node) => { const point = at.get(node.id)!; const label = node.kind === "project" ? mask(node.label, "path", privacy) : node.kind === "session" || node.kind === "attachment" ? mask(node.label, "session", privacy) : node.label; return <g key={node.id} transform={`translate(${point.x},${point.y})`} className={`graph-node ${node.kind}`} onClick={() => onSelect(node)} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") onSelect(node); }} role={node.target ? "button" : undefined} tabIndex={node.target ? 0 : undefined}><circle r="25"/><text y="42" textAnchor="middle">{label.slice(0, 18)}</text></g>; })}
      </svg>
      <ul className="similarity-list">{derived.map((edge) => <li key={`${edge.from}:${edge.to}`}><span className="derived">derived</span> {short(edge.from)} ↔ {short(edge.to)} · {edge.score?.toFixed(2) ?? ""} · {edge.projection ?? ""}</li>)}</ul>
    </div>
  );
}
