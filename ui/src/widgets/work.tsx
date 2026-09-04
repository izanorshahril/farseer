import { useCallback, useEffect, useMemo, useState } from "react";
import type { Bridge } from "../bridge";
import { onSubjectSelection, selectSubject, selectedSubject } from "../selection";
import { follow } from "../stream";

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
type Conversation = {
  conversation_id: string;
  title: string;
  project_path?: string;
  manager_runner?: string;
  updated_ts: number;
  archived_ts?: number;
};
type Run = { run_id: string; runner: string; outcome?: string };
type Session = { run_id: string; identifier_kind: string; identifier: string; log_pointer?: string };
type Attachment = { digest: string; run_id: string; custody: string; source: string };
type TaskDetail = { task: Task; runs: Run[]; sessions: Session[]; attachments: Attachment[]; transitions: { from: TaskState; to: TaskState; actor: string; reason: string; ts: number }[] };
type Graph = {
  projects: string[];
  conversations: Conversation[];
  tasks: Task[];
  runs: { run_id: string; task_id: string; cell_id: string; runner: string }[];
  sessions: Session[];
  parents: { run_id: string; parent_run_id: string; kind: string }[];
  similarities: { left_digest: string; right_digest: string; score: number; projection_version: string }[];
};
type Cell = { manager: { runners: string[] } };
type Face = "board" | "conversations" | "graph" | "completed";

const STATES: TaskState[] = ["inbox", "planned", "in_progress", "blocked", "review", "done", "cancelled"];
const NEXT: Record<TaskState, TaskState[]> = {
  inbox: ["planned", "in_progress", "cancelled"],
  planned: ["in_progress", "blocked", "cancelled"],
  in_progress: ["blocked", "review", "cancelled"],
  blocked: ["planned", "in_progress", "cancelled"],
  review: ["in_progress", "done", "cancelled"],
  done: [],
  cancelled: [],
};

const short = (value: string) => value.slice(0, 8);
const stateLabel = (state: TaskState) => state.replace("_", " ");

export function WorkWidget({ bridge }: { bridge: Bridge }) {
  const [face, setFace] = useState<Face>("board");
  const [expanded, setExpanded] = useState(false);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [graph, setGraph] = useState<Graph | null>(null);
  const [subject, setSubject] = useState(selectedSubject());
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [runners, setRunners] = useState<string[]>([]);
  const [newTitle, setNewTitle] = useState("");
  const [transcriptPath, setTranscriptPath] = useState("");
  const [transcriptMode, setTranscriptMode] = useState("reference");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    const [nextTasks, nextConversations, nextGraph, cell] = await Promise.all([
      bridge.read<Task[]>("/tasks?limit=1000"),
      bridge.read<Conversation[]>("/conversations?limit=500"),
      bridge.read<Graph>("/work/graph"),
      bridge.read<Cell>("/cells/zero"),
    ]);
    setTasks(nextTasks);
    setConversations(nextConversations);
    setGraph(nextGraph);
    setRunners(cell.manager.runners);
    const selectedTask = selectedSubject().task;
    if (selectedTask) {
      setDetail(await bridge.read<TaskDetail>(`/tasks/${selectedTask}`));
    }
    setError(null);
  }, [bridge]);

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

  useEffect(() => onSubjectSelection(setSubject), []);
  useEffect(() => {
    if (!subject.task) {
      setDetail(null);
      return;
    }
    bridge.read<TaskDetail>(`/tasks/${subject.task}`).then(setDetail).catch((failure: Error) => setError(failure.message));
  }, [bridge, subject.task]);

  const grouped = useMemo(
    () => Object.fromEntries(STATES.map((state) => [state, tasks.filter((task) => task.state === state)])) as Record<TaskState, Task[]>,
    [tasks],
  );

  const chooseTask = (task: Task) => {
    selectSubject({ conversation: task.conversation_id, task: task.task_id, project: task.project_path ?? null, run: null });
  };

  const transition = async (state: TaskState) => {
    if (!detail) return;
    await bridge.post(`/tasks/${detail.task.task_id}/transition`, {
      state,
      reason: `Moved from ${stateLabel(detail.task.state)} to ${stateLabel(state)} in Work`,
    });
    await load();
    setDetail(await bridge.read<TaskDetail>(`/tasks/${detail.task.task_id}`));
  };

  const addTranscript = async () => {
    const run = detail?.runs.at(-1);
    if (!run || !transcriptPath.trim()) return;
    await bridge.post(`/runs/${run.run_id}/transcripts`, { mode: transcriptMode, path: transcriptPath.trim() });
    setTranscriptPath("");
    setDetail(await bridge.read<TaskDetail>(`/tasks/${detail!.task.task_id}`));
    await load();
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
          {(["board", "conversations", "graph", "completed"] as Face[]).map((name) => (
            <button key={name} className={face === name ? "chip on" : "chip"} role="tab" aria-selected={face === name} onClick={() => setFace(name)}>
              {name}
            </button>
          ))}
        </div>
        <button className="chip" aria-pressed={expanded} onClick={() => setExpanded((current) => !current)}>{expanded ? "restore" : "expand"}</button>
      </div>
      {error && <p className="empty bad" role="alert">{error}</p>}

      {face === "board" && (
        <div className="work-board">
          {STATES.filter((state) => state !== "done" && state !== "cancelled").map((state) => (
            <section key={state} className="work-column" aria-label={stateLabel(state)}>
              <h4>{stateLabel(state)} <span>{grouped[state].length}</span></h4>
              {grouped[state].map((task) => (
                <button key={task.task_id} className={subject.task === task.task_id ? "work-card selected" : "work-card"} onClick={() => chooseTask(task)}>
                  <b>{task.title}</b><small>{task.project_path ?? "fleet"}</small>
                </button>
              ))}
            </section>
          ))}
        </div>
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
                  <b>{conversation.title}</b><small>{conversation.project_path ?? "fleet"}</small><span className="mono">{short(conversation.conversation_id)}</span>
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

      {face === "completed" && (
        <div className="completed-work">
          {[...grouped.done, ...grouped.cancelled].map((task) => (
            <button key={task.task_id} className="row-button" onClick={() => chooseTask(task)}><b>{task.title}</b><span className={`badge ${task.state === "cancelled" ? "bad" : ""}`}>{task.state}</span></button>
          ))}
          {grouped.done.length + grouped.cancelled.length === 0 && <p className="empty">No completed work yet.</p>}
        </div>
      )}

      {face === "graph" && graph && <WorkGraph graph={graph} />}

      {detail && (
        <aside className="task-detail" aria-label="Selected task detail">
          <div className="row"><b>{detail.task.title}</b><span className="badge">{stateLabel(detail.task.state)}</span><button className="chip" onClick={() => selectSubject({ task: null, run: null })}>close</button></div>
          <p>{detail.task.goal}</p>
          <div className="task-actions">{NEXT[detail.task.state].map((state) => <button key={state} className="chip" onClick={() => transition(state).catch((failure: Error) => setError(failure.message))}>{stateLabel(state)}</button>)}</div>
          <div className="task-runs">{detail.runs.map((run) => <button key={run.run_id} className="chip" onClick={() => selectSubject({ run: run.run_id })}>{short(run.run_id)} · {run.runner} · {run.outcome ?? "running"}</button>)}</div>
          {detail.sessions.map((session) => <p key={`${session.identifier_kind}:${session.identifier}`} className="mono small">{session.identifier_kind} {session.identifier}{session.log_pointer ? ` · ${session.log_pointer}` : ""}</p>)}
          <form className="transcript-form" onSubmit={(event) => { event.preventDefault(); addTranscript().catch((failure: Error) => setError(failure.message)); }}>
            <select aria-label="transcript custody" value={transcriptMode} onChange={(event) => setTranscriptMode(event.currentTarget.value)}><option>reference</option><option>copy</option><option>copy-plus-index</option></select>
            <input aria-label="transcript file path" value={transcriptPath} onChange={(event) => setTranscriptPath(event.currentTarget.value)} placeholder="harness transcript path" />
            <button className="chip" disabled={!detail.runs.length || !transcriptPath.trim()}>attach</button>
          </form>
          {detail.attachments.map((attachment) => <p key={attachment.digest} className="mono small">{attachment.custody} · {short(attachment.digest)} · {attachment.source}</p>)}
        </aside>
      )}
    </div>
  );
}

function WorkGraph({ graph }: { graph: Graph }) {
  const nodes = [
    ...graph.projects.map((project) => ({
      id: `project:${project}`,
      label: project.split(/[\\/]/).at(-1) ?? project,
      kind: "project",
    })),
    ...graph.conversations.map((conversation) => ({ id: conversation.conversation_id, label: conversation.title, kind: "conversation" })),
    ...graph.tasks.map((task) => ({ id: task.task_id, label: task.title, kind: "task" })),
    ...graph.runs.map((run) => ({ id: run.run_id, label: run.runner, kind: "run" })),
    ...graph.sessions.map((session) => ({ id: `${session.identifier_kind}:${session.identifier}`, label: `${session.identifier_kind} ${short(session.identifier)}`, kind: "session" })),
  ].slice(0, 48);
  const at = new Map(nodes.map((node, index) => [node.id, { x: 90 + (index % 6) * 150, y: 50 + Math.floor(index / 6) * 90 }]));
  const observed = [
    ...graph.conversations
      .filter((conversation) => conversation.project_path)
      .map((conversation) => ({
        from: `project:${conversation.project_path}`,
        to: conversation.conversation_id,
        label: "conversation",
      })),
    ...graph.tasks.map((task) => ({ from: task.conversation_id, to: task.task_id, label: "task" })),
    ...graph.runs.map((run) => ({ from: run.task_id, to: run.run_id, label: "run" })),
    ...graph.sessions.map((session) => ({ from: session.run_id, to: `${session.identifier_kind}:${session.identifier}`, label: "session" })),
    ...graph.parents.map((parent) => ({ from: parent.parent_run_id, to: parent.run_id, label: parent.kind })),
  ];
  return (
    <div className="work-graph">
      <div className="graph-legend"><span>observed topology</span><span className="derived">derived similarity</span></div>
      <svg viewBox="0 0 950 760" role="img" aria-label="Conversation, task, run, harness session, delegation, cell call, rescope, continuation, and similarity graph">
        {observed.map((edge, index) => { const from = at.get(edge.from); const to = at.get(edge.to); return from && to ? <line key={`${edge.from}:${edge.to}:${index}`} x1={from.x} y1={from.y} x2={to.x} y2={to.y} className="observed-edge"><title>{edge.label}</title></line> : null; })}
        {nodes.map((node) => { const point = at.get(node.id)!; return <g key={node.id} transform={`translate(${point.x},${point.y})`} className={`graph-node ${node.kind}`}><circle r="25"/><text y="42" textAnchor="middle">{node.label.slice(0, 18)}</text></g>; })}
      </svg>
      <ul className="similarity-list">{graph.similarities.map((edge) => <li key={`${edge.left_digest}:${edge.right_digest}`}><span className="derived">derived</span> {short(edge.left_digest)} ↔ {short(edge.right_digest)} · {edge.score.toFixed(2)} · {edge.projection_version}</li>)}</ul>
    </div>
  );
}
