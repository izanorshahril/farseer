import { useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { meaningOf } from "../meaning";
import { follow, type RecordEvent } from "../stream";

/**
 * What the top manager asked a worker for, and what came back.
 *
 * The Runs widget already lists these, one truncated row each, sorted by time.
 * That answers *how many* and *did they pass*. It cannot answer the question an
 * operator actually has when a run goes wrong - **what was this worker told, and
 * what did it say** - because the goal is the column that gets cut.
 *
 * So this is the same runs, read as an exchange: the goal out, the answer back,
 * and the four facts that decide whether the pair makes sense together - runner,
 * outcome, what it cost, how long it took.
 *
 * `05 run state model` split manager runs from worker runs, and `run_queued`
 * pins which a run was in `_farseer_role`. Only workers appear here. The
 * operator's own exchange with the manager is the Conversation widget, and
 * keeping the two apart is the point: one is a conversation the operator is in.
 */
type Exchange = {
  run: string;
  cell: string;
  goal: string;
  runner?: string;
  /** What the worker said on the way out. Absent while it is still running. */
  answer?: string;
  outcome?: string;
  cost?: number;
  tokens?: number;
  queued: number;
  finished?: number;
};

const time = (ts: number) => new Date(ts).toLocaleTimeString(undefined, { hour12: false });
const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;

/** Wall time from queue to exit, which is the number an operator waits through. */
function took(exchange: Exchange): string | undefined {
  if (exchange.finished === undefined) return undefined;
  const ms = exchange.finished - exchange.queued;
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1000)}s`;
}

/**
 * Fold one event into the exchange it belongs to.
 *
 * Returns the map unchanged for anything that is not a worker run, which is most
 * of the stream. A manager's own `run_queued` carries the operator's words and
 * belongs in the Conversation widget; showing it here too would make one
 * instruction look like two.
 */
function fold(current: Map<string, Exchange>, event: RecordEvent): Map<string, Exchange> {
  const payload = (event.payload ?? {}) as Record<string, unknown>;
  const str = (key: string) => (typeof payload[key] === "string" ? (payload[key] as string) : undefined);
  const num = (key: string) => (typeof payload[key] === "number" ? (payload[key] as number) : undefined);

  if (event.kind === "run_queued") {
    if (payload["_farseer_role"] !== "worker") return current;
    const goal = str("goal");
    if (!goal) return current;
    const next = new Map(current);
    next.set(event.run_id, {
      run: event.run_id,
      cell: event.cell_id,
      goal,
      runner: str("runner"),
      queued: event.ts,
      ...next.get(event.run_id),
    });
    return next;
  }

  // Every later kind only decorates a worker this widget already knows about,
  // so an event for a manager run falls through without a role check.
  const known = current.get(event.run_id);
  if (!known) return current;

  if (event.kind === "run_finished") {
    const next = new Map(current);
    next.set(event.run_id, {
      ...known,
      answer: str("text") ?? known.answer,
      outcome: str("outcome"),
      cost: num("cost_usd_micros"),
      tokens: num("tokens"),
      finished: event.ts,
    });
    return next;
  }
  if (event.kind === "session_started" && str("runner")) {
    const next = new Map(current);
    next.set(event.run_id, { ...known, runner: str("runner") });
    return next;
  }
  return current;
}

export function DelegationWidget({ bridge }: { bridge: Bridge }) {
  const [byRun, setByRun] = useState<Map<string, Exchange>>(new Map());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const add = (event: RecordEvent) => live && setByRun((current) => fold(current, event));
    // Replay then follow, per `07 attach semantics` - the same call, a different
    // cursor. Without the replay a widget mounted after a run is a blank panel
    // about work that already happened.
    bridge
      .read<RecordEvent[]>("/events?tail=600")
      .then((events) => live && events.forEach(add))
      .catch((e: Error) => live && setError(e.message));
    const subscription = follow(add);
    return () => {
      live = false;
      subscription.close();
    };
  }, [bridge]);

  if (error) return <p className="empty bad">{error}</p>;

  // Newest first: a delegation from an hour ago is history, and the one that
  // just started is the one being watched.
  const exchanges = [...byRun.values()].sort((a, b) => b.queued - a.queued).slice(0, 20);

  if (exchanges.length === 0)
    return (
      <p className="empty">
        No worker has been given anything yet. When the top manager delegates, what it asked for
        and what came back appear here as a pair.
      </p>
    );

  return (
    <ol className="exchanges">
      {exchanges.map((exchange) => (
        <li key={exchange.run}>
          <div className="row small">
            <b title={meaningOf("worker")}>to a worker</b>
            <span className="faint mono">{time(exchange.queued)}</span>
            <span className="faint mono">{exchange.run.slice(0, 8)}</span>
            {exchange.runner && <span className="badge">{exchange.runner}</span>}
            <span className="grow" />
            {took(exchange) && <span className="faint mono">{took(exchange)}</span>}
            {typeof exchange.cost === "number" && exchange.cost > 0 && (
              <span className="faint mono">{usd(exchange.cost)}</span>
            )}
            {exchange.outcome ? (
              <span className={`badge ${exchange.outcome === "ok" ? "allowed" : "bad"}`}>
                {exchange.outcome}
              </span>
            ) : (
              <span className="badge" title="queued or running; no outcome yet">
                running
              </span>
            )}
          </div>
          <p className="asked">{exchange.goal}</p>
          {exchange.answer ? (
            <p className="answered">{exchange.answer}</p>
          ) : (
            /* Absent rather than blank: a worker that has not answered is not a
               worker that answered nothing, and `10 runner inventory`'s rule
               about silence applies to a run as much as to a runner. */
            <p className="answered dim">no answer yet</p>
          )}
        </li>
      ))}
    </ol>
  );
}
