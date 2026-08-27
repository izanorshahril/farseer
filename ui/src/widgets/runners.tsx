import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow } from "../stream";

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
  /** `null` for a run this farseer process did not start - see `17 cell lifecycle`. */
  liveness: "live" | "stalled" | "likely_hung" | null;
};

/** What the settings menu already knows about each runner, joined by name. */
type RunnerFacts = { name: string; note: string; cannot: string[] };

const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(4)}`;

function elapsed(since: number): string {
  const secs = Math.max(0, Math.round((Date.now() - since) / 1000));
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

export function RunnersWidget({ bridge }: { bridge: Bridge }) {
  const [runs, setRuns] = useState<Run[] | null>(null);
  const [facts, setFacts] = useState<Record<string, RunnerFacts>>({});
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
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
              <span className="mono faint">task {task.task_id.slice(0, 8)}</span>
              <span className="grow" />
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
                      <b className="mono">{run.runner}</b>
                      <span className="dim small">{run.model || "model not reported"}</span>
                      <span className="badge">{run.cell_id}</span>
                      <span className="grow" />
                      <span className="mono faint small">{elapsed(run.started_ts)}</span>
                      <span className="mono faint small">
                        {run.tokens.toLocaleString()} tok
                      </span>
                      <span className="mono faint small">{usd(run.usd_micros)}</span>
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
