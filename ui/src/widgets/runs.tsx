import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow } from "../stream";
import { selectRun } from "../selection";
import { confirmVerb } from "../confirm";
import { meaningOf } from "../meaning";

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
  /** `07 attach semantics`'s third axis: autonomous, observed or taken_over. */
  control: "autonomous" | "observed" | "taken_over";
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

/**
 * What `05 run state model` and `07 attach semantics` permit, given where the
 * run actually is.
 *
 * Two axes, not one. Lifecycle decides whether anything can be sent at all;
 * control decides who is holding the wheel. A verb appears only when the
 * runtime would accept it - the rule this widget exists to keep.
 *
 * `observe` is offered on a **finished** run too. `07` section 1 makes attach
 * target a run rather than a process: with a live worker the subscription
 * continues into live output, without one it is replay, and one address answers
 * either way.
 */
function verbsFor(run: Run, steerable: (runner: string) => boolean): string[] {
  const verbs: string[] = [];
  if (run.control === "autonomous") verbs.push("observe");
  else verbs.push("release");
  if (run.lifecycle !== "running") return verbs;
  // Live input needs a process listening on stdin. The runtime answers `400`
  // when the runner has no steering path, and finding that out by clicking is
  // exactly what this list exists to prevent - but the runner is on the row, so
  // the decision is honest rather than optimistic.
  //
  // Asked of the shell rather than named here: this used to read
  // `runner === "claude-code"`, which was true when it was written and stopped
  // being true the moment ACP and pi arrived, silently. One table, everything
  // else derived.
  if (steerable(run.runner)) {
    // A manager is a conversation, so `steer` is how the operator talks to it.
    // A worker is executing a contract somebody else wrote, and `07` section 3
    // refused typing into one unannounced - so its input verb is behind a
    // takeover, and the takeover is the event the record hangs on.
    if (run.role === "manager") verbs.push("steer");
    else if (run.control === "taken_over") verbs.push("intervene");
    else verbs.push("take over");
  }
  verbs.push("cancel");
  return verbs;
}

/** Which path each verb posts to. `take over` is two words and one route. */
const VERB_PATH: Record<string, string> = {
  observe: "observe",
  "take over": "take-over",
  release: "release",
};

/**
 * The same rows, ordered so a worker sits under the manager that spawned it.
 *
 * `28 operator surface`'s review found this widget's one blind spot: every row
 * looked alike, so a screen of twenty-five runs said nothing about which of
 * them an operator started and which a manager did on its own. The Runners
 * widget beside it has answered that since it was written, by indenting runs
 * that share a `task_id` - so the fix is to speak that idiom here rather than
 * invent a second one.
 *
 * Two things this deliberately does not do:
 *
 * - **Guess a parent.** The indent is `role`, which the record holds, not
 *   position in the list. A worker whose manager fell outside this window is
 *   shown flat, because indenting it under the row above would name a manager
 *   that is not there.
 * - **Re-sort the fleet.** Tasks keep the order the API returned them in -
 *   newest first - and only the runs *within* one task are put oldest first, so
 *   the manager comes before what it handed off.
 */
type Row = { run: Run; under: boolean };

function threaded(runs: Run[]): Row[] {
  const rows: Row[] = [];
  const done = new Set<string>();
  for (const run of runs) {
    if (done.has(run.task_id)) continue;
    done.add(run.task_id);
    const task = runs
      .filter((candidate) => candidate.task_id === run.task_id)
      .sort((a, b) => a.started_ts - b.started_ts);
    const hasManager = task.some((candidate) => candidate.role !== "worker");
    for (const member of task) {
      rows.push({ run: member, under: hasManager && member.role === "worker" });
    }
  }
  return rows;
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
      const short = run.run_id.slice(0, 8);
      if (verb === "steer" || verb === "intervene") {
        const ask =
          verb === "steer"
            ? `Steer run ${short} - same run, same contract:`
            : // Named as what it is. `07 attach semantics` section 6: the
              // manager is told and decides for itself whether the goal
              // changed, and the run carries the flag for good.
              `Send this to run ${short}. The manager is told, and the run is marked as touched:`;
        const message = prompt(ask);
        if (!message) return;
        await bridge.post(`/runs/${run.run_id}/${verb === "steer" ? "steer" : "intervene"}`, {
          message,
        });
        setNote(`${verb === "steer" ? "steered" : "sent to"} ${short}`);
      } else if (verb === "cancel") {
        await bridge.post(`/runs/${run.run_id}/cancel`);
        setNote(`cancelled ${short}`);
      } else {
        // observe, take over, release: no body, and the answer is the control
        // state the run landed in.
        const landed = (await bridge.post(`/runs/${run.run_id}/${VERB_PATH[verb]}`)) as
          | { control?: string }
          | undefined;
        // The runtime answers with the state the run landed in, so the note
        // says what is true rather than what was asked for.
        setNote(`${short} is ${landed?.control ?? "changed"}`);
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
        {threaded(runs).map(({ run, under }) => {
          const verbs = verbsFor(run, steerable);
          return (
            <li key={run.run_id} className={under ? "under" : undefined}>
              {under ? (
                <span className="mono faint small elbow" title={meaningOf("role")}>
                  └
                </span>
              ) : (
                run.role && (
                  <span className="badge role" title={meaningOf("role")}>
                    {run.role}
                  </span>
                )
              )}
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
              {/* `$0.00` is a claim; most runners report no cost at all and
                  this rendered their silence as a number. The rest of this
                  codebase says `not stated` and this line did not. */}
              {run.usd_micros > 0 ? (
                <span className="mono faint small">{usd(run.usd_micros)}</span>
              ) : (
                <span className="faint small" title="this runner reported no cost">
                  -
                </span>
              )}
              <span className="grow" />
              {verbs.map((verb) => (
                <button
                  key={verb}
                  className={
                    verb === "cancel"
                      ? "chip danger"
                      : // The two verbs that put a person between the agent and
                        // its work read as held rather than as neutral.
                        verb === "take over" || verb === "intervene"
                        ? "chip on"
                        : "chip"
                  }
                  disabled={busy !== null}
                  onClick={() => {
                    // Named, not "are you sure": the risk here is having hit the
                    // wrong row in a list of twenty-five, and a dialog that does
                    // not name the row cannot catch that.
                    if (confirmVerb(verb, run.title ?? run.run_id.slice(0, 8))) {
                      void act(run, verb);
                    }
                  }}
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
