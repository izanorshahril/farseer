import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow, type RecordEvent } from "../stream";

/**
 * Every runner process farseer has alive right now.
 *
 * The Runs widget beside this one is a **history** with `05 run state model`'s
 * verbs on it. This one answers a different question, and the question an
 * operator actually asks while watching: *what is spawned?* A run that finished
 * an hour ago is the answer to neither, so it is not here.
 *
 * Three things this shows that a run row cannot:
 *
 * - **What each process is**, not just which runner: the model it is on, since
 *   `runners.toml` can pin one and a fleet where every runner is on the same
 *   model is a fleet whose differences are farseer's doing.
 * - **A tree.** Runs sharing a `task_id` are one manager and the workers it
 *   delegated to. Grouping by it is the only view in which "the manager spawned
 *   nothing" is visible as a fact rather than as an absence.
 * - **What farseer cannot do with it**, read from the same `control_of` the
 *   settings menu reads. `28 operator surface`'s design review called out that
 *   a caveat shown *before* the operator picks a runner and never again leaves
 *   absent-because-unreportable looking identical to
 *   absent-because-nothing-happened. This is where it stops looking identical.
 */
type Run = {
  run_id: string;
  task_id: string;
  cell_id: string;
  runner: string;
  model: string;
  lifecycle: "running" | "finished";
  outcome: string | null;
  usd_micros: number;
  tokens: number;
  started_ts: number;
  /** One line saying what this run is for, derived from its goal. */
  title: string | null;
  /** `manager` or `worker`, as recorded when the run was queued. */
  role: string | null;
  /** `null` for a run this farseer process did not start - see `17 cell lifecycle`. */
  liveness: "live" | "stalled" | "likely_hung" | null;
};

/** What the settings menu already knows about each runner, joined by name. */
type RunnerFacts = {
  name: string;
  note: string;
  cannot: string[];
  /** How to read this runner's money, or null when it reports none. */
  cost_basis: string | null;
};

const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(4)}`;

function elapsed(since: number): string {
  const secs = Math.max(0, Math.round((Date.now() - since) / 1000));
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

/** Kinds that are farseer talking to itself rather than the run doing anything. */
const PLUMBING = new Set(["run_queued", "agent_message_chunk", "stream_event"]);

const time = (ts: number) => new Date(ts).toLocaleTimeString(undefined, { hour12: false });

/**
 * One runner's thread: what went in, what came back, in order.
 *
 * Read from the record rather than from the process, which is why it works for
 * a run that has already finished and for one this farseer process did not
 * start. `02 record scope` is the whole reason there is anything to show - the
 * runner is not asked to remember, the record already did.
 *
 * Deliberately not a rendered conversation. The Conversation widget is where an
 * operator *talks*; this is where they check what actually crossed the wire, so
 * a payload with no text shows its shape instead of being dropped.
 */
function Thread({ bridge, run, onBack }: { bridge: Bridge; run: Run; onBack: () => void }) {
  const [events, setEvents] = useState<RecordEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    () =>
      bridge
        // `run`, not `run_id`: an unparseable id is a `400` rather than a
        // silently unfiltered read of every other run in the log.
        .read<RecordEvent[]>(`/events?run=${run.run_id}&limit=500`)
        .then(setEvents)
        .catch((e: Error) => setError(e.message)),
    [bridge, run.run_id],
  );

  useEffect(() => {
    void load();
    // A finished run will never fire again; a live one still might.
    if (run.lifecycle !== "running") return;
    const subscription = follow(() => void load());
    return subscription.close;
  }, [load, run.lifecycle]);

  const shown = (events ?? []).filter((event) => !PLUMBING.has(event.kind));

  return (
    <div className="thread-view">
      <div className="row">
        <button className="chip" onClick={onBack}>
          back
        </button>
        <b>{run.title ?? run.runner}</b>
        <span className="dim small mono">{run.runner}</span>
        <span className="dim small">{run.model || "model not reported"}</span>
        {run.role && <span className="badge">{run.role}</span>}
        <span className="grow" />
        <span className="mono faint small">{run.run_id.slice(0, 8)}</span>
      </div>
      {error && <p className="empty bad">{error}</p>}
      {!events && !error && <p className="empty">reading the thread...</p>}
      {events && shown.length === 0 && (
        <p className="empty">
          Nothing in the record for this run yet
          {events.length > 0 && ` - ${events.length} setup event${events.length === 1 ? "" : "s"} hidden`}.
        </p>
      )}
      <ol className="thread">
        {shown.map((event) => {
          const payload = (event.payload ?? {}) as Record<string, unknown>;
          const text = payload["text"] ?? payload["result"] ?? payload["message"];
          return (
            <li key={event.seq}>
              <div className="row">
                <span className="mono faint small">{time(event.ts)}</span>
                <span className="kind">{event.kind}</span>
                <span className="dim small">{event.actor}</span>
              </div>
              {typeof text === "string" && text.trim() ? (
                <p className="thread-text">{text}</p>
              ) : (
                // Shape rather than nothing: this view exists to show what
                // crossed the wire, and an event with no prose still did.
                Object.keys(payload).length > 0 && (
                  <pre className="thread-payload">{JSON.stringify(payload, null, 1)}</pre>
                )
              )}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

export function RunnersWidget({ bridge }: { bridge: Bridge }) {
  const [runs, setRuns] = useState<Run[] | null>(null);
  const [facts, setFacts] = useState<Record<string, RunnerFacts>>({});
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  /** The run whose thread is showing, or `null` for the list. */
  const [open, setOpen] = useState<string | null>(null);
  // Only so elapsed times move. Nothing is fetched on this tick: `05 run state
  // model` says a run changes state because something happened, and the stream
  // below is what says so.
  const [, tick] = useState(0);

  const load = useCallback(
    () =>
      bridge
        .read<Run[]>("/runs?limit=50")
        .then(setRuns)
        .catch((e: Error) => setNote(e.message)),
    [bridge],
  );

  useEffect(() => {
    void load();
    const subscription = follow(() => void load());
    const timer = setInterval(() => tick((n) => n + 1), 1000);
    return () => {
      subscription.close();
      clearInterval(timer);
    };
  }, [load]);

  useEffect(() => {
    // The shell's own surface, not the runtime's: what farseer can do with a
    // runner is a property of this build, and reading it from one place keeps
    // this widget from growing a second opinion.
    fetch("/__settings/runners")
      .then((r) => r.json() as Promise<RunnerFacts[]>)
      .then((list) => setFacts(Object.fromEntries(list.map((f) => [f.name, f]))))
      .catch(() => setFacts({}));
  }, []);

  const cancel = async (run: Run) => {
    setBusy(run.run_id);
    setNote(null);
    try {
      await bridge.post(`/runs/${run.run_id}/cancel`);
      await load();
    } catch (e) {
      setNote((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  if (note && !runs) return <p className="empty bad">{note}</p>;
  if (!runs) return <p className="empty">reading runners...</p>;

  const opened = runs.find((run) => run.run_id === open);
  if (open && !opened) {
    // The run left the window this widget reads. Say so rather than silently
    // dropping back to the list, which would look like the click did nothing.
    return (
      <p className="empty">
        That run is no longer in the last 50.{" "}
        <button className="chip" onClick={() => setOpen(null)}>
          back
        </button>
      </p>
    );
  }
  if (opened) return <Thread bridge={bridge} run={opened} onBack={() => setOpen(null)} />;

  const live = runs.filter((run) => run.lifecycle === "running");
  if (live.length === 0) {
    return (
      <p className="empty">
        Nothing spawned.
        {runs.length > 0 && ` ${runs.length} run${runs.length === 1 ? "" : "s"} have finished.`}
      </p>
    );
  }

  // Oldest first inside a task: the manager started before the workers it
  // delegated to, so the tree reads top-down without storing a parent.
  const tasks = [...new Set(live.map((run) => run.task_id))].map((task_id) => ({
    task_id,
    runs: live
      .filter((run) => run.task_id === task_id)
      .sort((a, b) => a.started_ts - b.started_ts),
  }));

  return (
    <>
      <ul className="runners-live">
        {tasks.map((task) => (
          <li key={task.task_id}>
            <div className="row dim small">
              {/* The task's name is its first process's goal - the operator
                  asked for one thing, and everything under it is how. */}
              <b className="task-title">{task.runs[0]?.title ?? "untitled"}</b>
              <span className="grow" />
              <span className="mono faint">{task.task_id.slice(0, 8)}</span>
              <span>
                {task.runs.length} process{task.runs.length === 1 ? "" : "es"}
              </span>
            </div>
            <ul className="runners-tree">
              {task.runs.map((run, index) => {
                const cannot = facts[run.runner]?.cannot ?? [];
                return (
                  <li key={run.run_id}>
                    <div className="row">
                      {/* The first process of a task is the one nothing
                          delegated to; everything after it was spawned by
                          something already on this list. Derived from order
                          rather than stored, because farseer does not record a
                          parent and inventing one here would be a claim. */}
                      <span className="mono faint small">{index === 0 ? "" : "└"}</span>
                      <span
                        className={`dot ${run.liveness ?? "done"}`}
                        title={run.liveness ?? "started before this farseer process"}
                      />
                      <button
                        className="linkish"
                        onClick={() => setOpen(run.run_id)}
                        title="show this runner's thread"
                      >
                        <b className="mono">{run.runner}</b>
                      </button>
                      <span className="dim small">{run.model || "model not reported"}</span>
                      {run.role && <span className="badge">{run.role}</span>}
                      <span className="badge">{run.cell_id}</span>
                      <span className="grow" />
                      <span className="mono faint small">{elapsed(run.started_ts)}</span>
                      <span className="mono faint small">
                        {run.tokens.toLocaleString()} tok
                      </span>
                      {/* Never a bare dollar figure. `27 quota accounting`
                          refused a derived percentage where a reported one
                          existed; pi's cost is an API list price and not what
                          a subscription was billed, and the two must never be
                          read as the same number. */}
                      <span
                        className={`mono faint small${facts[run.runner]?.cost_basis === "at list price, not billed" ? " notional" : ""}`}
                        title={facts[run.runner]?.cost_basis ?? "this runner reports no cost"}
                      >
                        {facts[run.runner]?.cost_basis ? usd(run.usd_micros) : "-"}
                      </span>
                      <button
                        className="chip danger"
                        disabled={busy !== null}
                        onClick={() => void cancel(run)}
                      >
                        {busy === run.run_id ? "..." : "cancel"}
                      </button>
                    </div>
                    {/* Said here as well as in the menu, because this is where
                        an empty column is about to be read as "nothing
                        happened". */}
                    {cannot.map((warning) => (
                      <div key={warning} className="dim small caveat">
                        {warning}
                      </div>
                    ))}
                    {run.liveness === null && (
                      <div className="dim small caveat">
                        started before this farseer process, so there is no liveness to ask for
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          </li>
        ))}
      </ul>
      {note && <p className="dim small">{note}</p>}
    </>
  );
}
