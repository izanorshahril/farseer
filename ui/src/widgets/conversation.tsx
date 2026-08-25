import { useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow, type RecordEvent } from "../stream";

/**
 * What the top manager said, as a conversation.
 *
 * `16 local api surface` made an instruction fire-and-forget and promised the
 * answer arrives on the event stream. This is the surface that promise was for -
 * without it the operator can start a manager and never hear back, which makes
 * the composer a shout into a hole.
 *
 * It is **built from the record**, not from a socket the composer holds open.
 * A conversation reconstructed from events survives a reload, a restart and a
 * second window, and `07 attach semantics` already made replay and live the same
 * call with a different cursor. Nothing here is a chat session; it is a view of
 * what happened, which is the only thing farseer keeps.
 */
type Turn = {
  seq: number;
  run: string;
  who: "operator" | "manager";
  text: string;
  ts: number;
  outcome?: string;
  cost?: number | null;
  /** Set on the run's exit, which a live manager reaches long after it last spoke. */
  final?: boolean;
};

const time = (ts: number) => new Date(ts).toLocaleTimeString(undefined, { hour12: false });
const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;

/**
 * The goal an operator typed, out of the queued event's contract snapshot.
 *
 * The anchor the canvas prepends rides in the goal as prose, so it is shown as
 * typed rather than parsed back out - farseer never claimed that text had a
 * structure, and pretending it does here would invent one.
 */
function turnFrom(event: RecordEvent): Turn | null {
  const payload = event.payload as Record<string, unknown> | null;
  if (!payload) return null;

  if (event.kind === "run_queued") {
    const goal = payload["goal"];
    if (typeof goal !== "string" || !goal.trim()) return null;
    // Workers get goals too, and they are not talking to the operator.
    if (payload["_farseer_role"] === "worker") return null;
    return { seq: event.seq, run: event.run_id, who: "operator", text: goal, ts: event.ts };
  }

  // A manager on live stdin answers per turn and stays alive, so this is the
  // event a conversation is actually made of. `run_finished` still counts,
  // because a one-shot runner has only that.
  if (event.kind === "manager_answered") {
    const text = payload["text"];
    if (typeof text !== "string" || !text.trim()) return null;
    return { seq: event.seq, run: event.run_id, who: "manager", text, ts: event.ts };
  }

  if (event.kind === "run_finished") {
    const text = payload["text"];
    if (typeof text !== "string" || !text.trim()) return null;
    // A run whose last answer is already in the thread should not repeat it
    // when it finally exits; the outcome is what is new by then.
    return {
      seq: event.seq,
      run: event.run_id,
      who: "manager",
      text,
      ts: event.ts,
      final: true,
      outcome: typeof payload["outcome"] === "string" ? payload["outcome"] : undefined,
      cost: typeof payload["cost_usd_micros"] === "number" ? payload["cost_usd_micros"] : null,
    };
  }
  return null;
}

export function ConversationWidget({ bridge }: { bridge: Bridge }) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const add = (event: RecordEvent) => {
      const turn = turnFrom(event);
      if (!turn) return;
      setTurns((current) => {
        if (current.some((t) => t.seq === turn.seq)) return current;
        // The exit repeats the last thing said, so replace rather than echo.
        const echoes =
          turn.final &&
          current.some((t) => t.run === turn.run && t.who === "manager" && t.text === turn.text);
        const kept = echoes
          ? current.filter((t) => !(t.run === turn.run && t.who === "manager" && t.text === turn.text))
          : current;
        return [...kept, turn].sort((a, b) => a.seq - b.seq).slice(-40);
      });
    };

    // Replay what was already said, then follow. Same call, different cursor.
    bridge
      .read<RecordEvent[]>("/events?limit=200")
      .then((events) => live && events.forEach(add))
      .catch((e: Error) => live && setError(e.message));
    const subscription = follow(add);
    return () => {
      live = false;
      subscription.close();
    };
  }, [bridge]);

  if (error) return <p className="empty bad">{error}</p>;
  if (turns.length === 0)
    return (
      <p className="empty">
        Nothing said yet. Type below - it goes to the top manager, and its answer lands here when
        the run finishes.
      </p>
    );

  return (
    <ol className="thread">
      {turns.map((turn) => (
        <li key={turn.seq} className={turn.who}>
          <div className="row small">
            <b>{turn.who === "operator" ? "you" : "top manager"}</b>
            <span className="faint mono">{time(turn.ts)}</span>
            <span className="faint mono">{turn.run.slice(0, 8)}</span>
            {turn.outcome && turn.outcome !== "ok" && (
              <span className={`badge ${turn.outcome === "failed" ? "bad" : ""}`}>
                {turn.outcome}
              </span>
            )}
            <span className="grow" />
            {typeof turn.cost === "number" && turn.cost > 0 && (
              <span className="faint mono">{usd(turn.cost)}</span>
            )}
          </div>
          <p>{turn.text}</p>
        </li>
      ))}
    </ol>
  );
}
