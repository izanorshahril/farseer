import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow } from "../stream";
import { selectRun } from "../selection";

/**
 * The fleet, with `05 run state model`'s verbs on the line.
 *
 * The rule this widget exists to keep: **a surface never offers a verb the
 * runtime would refuse.** The verb list is derived from lifecycle and control
 * the same way liveness is derived from a timestamp - never stored, never
 * guessed, and never a button that fails when clicked.
 *
 * `28 operator surface`'s table put `steer`, `cancel`, `observe`, `take over`
 * and `release` inline here, and `re-run` and `re-scope` only where the contract
 * is on screen - both start a **new** run, so they belong to a detail view this
 * widget deliberately does not fake.
 */
type Run = {
  run_id: string;
  task_id: string;
  cell_id: string;
  runner: string;
  lifecycle: "running" | "finished";
  outcome: string | null;
  usd_micros: number;
  tokens: number;
  operator_touched: boolean;
  started_ts: number;
  liveness: "live" | "stalled" | "likely_hung" | null;
  title: string | null;
  role: string | null;
  /** Why it ended, when the record says. See `RunView::finished_reason`. */
  finished_reason: string | null;
};

/**
 * The outcome word, and the short form of why.
 *
 * `17 cell lifecycle` reaps a run whose farseer process is gone and marks it
 * `failed`, which is right - it started, and what it did is unknown. A screen of
 * bare `failed` after a restart still reads as a fleet that is broken, so the
 * reason the record already holds is shown beside the word rather than folded
 * into it.
 */
function why(run: Run): string | null {
  if (!run.finished_reason) return null;
  return run.finished_reason.includes("process that started this run is gone")
    ? "farseer restarted"
    : run.finished_reason.slice(0, 40);
}

/** What `05 run state model` permits, given where the run actually is. */
function verbsFor(run: Run, steerable: (runner: string) => boolean): string[] {
  if (run.lifecycle !== "running") return [];
  // Steer needs a live process listening on stdin. The runtime answers `400`
  // when the runner has no steering path, and finding that out by clicking is
  // exactly what this list exists to prevent - but the runner is on the row, so
  // the decision is honest rather than optimistic.
  //
  // Asked of the shell rather than named here: this used to read
  // `runner === "claude-code"`, which was true when it was written and stopped
  // being true the moment ACP and pi arrived, silently. One table, everything
  // else derived.
  return steerable(run.runner) ? ["steer", "cancel"] : ["cancel"];
}

const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;

const TONE: Record<string, string> = {
  ok: "ok",
  failed: "bad",
  cancelled: "dim",
  abandoned: "dim",
};

export function RunsWidget({ bridge }: { bridge: Bridge }) {
  const [runs, setRuns] = useState<Run[] | null>(null);
  /** Runner name to what farseer cannot do with it, from the settings surface. */
  const [cannot, setCannot] = useState<Record<string, string[]>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const load = useCallback(
    () =>
      bridge
        .read<Run[]>("/runs?limit=25")
        .then(setRuns)
        .catch((e: Error) => setNote(e.message)),
    [bridge],
  );

  useEffect(() => {
    void load();
    // The stream is the trigger, not a timer: a run changes state because
    // something happened, and something happening is an event.
    const subscription = follow(() => void load());
    return subscription.close;
  }, [load]);

  useEffect(() => {
    fetch("/__settings/runners")
      .then((r) => r.json() as Promise<{ name: string; cannot: string[] }[]>)
      .then((list) => setCannot(Object.fromEntries(list.map((f) => [f.name, f.cannot]))))
      // An unreachable settings surface offers no steer rather than a steer
      // that fails: under-promising is the safe direction here.
      .catch(() => setCannot({}));
  }, []);

  const steerable = (runner: string) =>
    (cannot[runner] ?? ["cannot be steered once a run starts"]).every(
      (warning) => !warning.includes("steered"),
    );

  const act = async (run: Run, verb: string) => {
    setBusy(`${run.run_id}:${verb}`);
    setNote(null);
    try {
      if (verb === "steer") {
        const message = prompt(`Steer run ${run.run_id.slice(0, 8)} - same run, same contract:`);
        if (!message) return;
        await bridge.post(`/runs/${run.run_id}/steer`, { message });
        setNote(`steered ${run.run_id.slice(0, 8)}`);
      } else {
        await bridge.post(`/runs/${run.run_id}/cancel`);
        setNote(`cancelled ${run.run_id.slice(0, 8)}`);
      }
      await load();
    } catch (e) {
      setNote((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  if (note && !runs) return <p className="empty bad">{note}</p>;
  if (!runs) return <p className="empty">reading runs...</p>;
  if (runs.length === 0) return <p className="empty">No runs yet.</p>;

  return (
    <>
      <ul className="runs">
        {runs.map((run) => {
          const verbs = verbsFor(run, steerable);
          return (
            <li key={run.run_id}>
              <span
                className={`dot ${run.liveness ?? (run.lifecycle === "running" ? "live" : "done")}`}
                title={run.liveness ?? run.lifecycle}
              />
              {/* The way into the detail view. `28 operator surface` held
                  `re-run` and `re-scope` back until the contract is on screen;
                  this is the click that puts it there. */}
              <button
                className="run-title link"
                title={`open ${run.run_id}`}
                onClick={() => selectRun(run.run_id)}
              >
                {run.title ?? run.run_id.slice(0, 8)}
              </button>
              <span className="badge">{run.cell_id}</span>
              <span className="dim mono small">{run.runner}</span>
              <span className={`kind ${TONE[run.outcome ?? ""] ?? ""}`}>
                {run.outcome ?? run.lifecycle}
              </span>
              {why(run) && (
                <span className="dim small" title={run.finished_reason ?? undefined}>
                  {why(run)}
                </span>
              )}
              {run.operator_touched && (
                <span className="badge touched" title="a human intervened, per 07">
                  touched
                </span>
              )}
              <span className="mono faint small">{usd(run.usd_micros)}</span>
              <span className="grow" />
              {verbs.map((verb) => (
                <button
                  key={verb}
                  className={verb === "cancel" ? "chip danger" : "chip"}
                  disabled={busy !== null}
                  onClick={() => void act(run, verb)}
                >
                  {busy === `${run.run_id}:${verb}` ? "..." : verb}
                </button>
              ))}
            </li>
          );
        })}
      </ul>
      {note && <p className="dim small">{note}</p>}
    </>
  );
}
