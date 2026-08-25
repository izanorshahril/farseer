import { useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow, type RecordEvent } from "../stream";

/**
 * What the fleet is doing, live.
 *
 * This is the widget that stops the canvas being write-only: the composer
 * accepts a run and `16 local api surface` made that fire-and-forget, so without
 * a stream the answer has nowhere to land.
 *
 * `05 run state model` split activity from progress, and this shows **progress**
 * - the kinds that belong to the record. Token chunks are activity, they drive
 * the watchdog, and they are deliberately not here: a feed that scrolls on every
 * token is a feed nobody reads.
 */
const NOISE = new Set(["agent_message_chunk", "stream_event"]);

/** Kinds worth a colour, because they mean something happened rather than progressed. */
const TONE: Record<string, string> = {
  run_finished: "ok",
  run_queued: "dim",
  operator_intervened: "accent",
  manager_steered: "accent",
  cell_called: "accent",
  rate_limit_event: "warn",
};

const time = (ts: number) =>
  new Date(ts).toLocaleTimeString(undefined, { hour12: false });

function summarise(event: RecordEvent): string {
  const payload = event.payload as Record<string, unknown> | null;
  const text = payload?.["text"] ?? payload?.["result"] ?? payload?.["tool_name"];
  if (typeof text === "string" && text.trim()) return text.slice(0, 160);
  if (event.kind === "cell_called") {
    const call = payload?.["call"] as { to_cell?: string } | undefined;
    return `to ${call?.to_cell ?? "a cell"}`;
  }
  return "";
}

export function ActivityWidget({ bridge: _bridge }: { bridge: Bridge }) {
  const [events, setEvents] = useState<RecordEvent[]>([]);
  const [live, setLive] = useState(false);

  useEffect(() => {
    const subscription = follow((event) => {
      if (NOISE.has(event.kind)) return;
      setLive(true);
      // Newest first, and bounded: an operator reads the top of a feed, and an
      // unbounded one is a memory leak with a scrollbar.
      setEvents((current) => [event, ...current].slice(0, 60));
    });
    return subscription.close;
  }, []);

  return (
    <>
      <div className="row dim small" style={{ marginBottom: 8 }}>
        <span className={live ? "pulse on" : "pulse"} aria-hidden />
        <span>{live ? "following the record" : "waiting for the first event"}</span>
      </div>
      {events.length === 0 ? (
        <p className="empty">
          Nothing yet. Anything the fleet does appears here as it happens - this is the same
          call that replays a finished run, with a different cursor.
        </p>
      ) : (
        <ul className="feed">
          {events.map((event) => (
            <li key={event.event_id}>
              <span className="mono faint">{time(event.ts)}</span>
              <span className={`kind ${TONE[event.kind] ?? ""}`}>{event.kind}</span>
              <span className="dim">{event.cell_id}</span>
              <span className="mono faint">{event.run_id.slice(0, 8)}</span>
              <span className="summary">{summarise(event)}</span>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
