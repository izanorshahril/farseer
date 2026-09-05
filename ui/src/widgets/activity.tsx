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

/**
 * How long a tool call took, once its result arrives.
 *
 * The feed used to render a three-minute `bash` and a two-hundred-millisecond
 * `read` as the same row, which is the thing that makes a trajectory unreadable:
 * every step looks equally expensive, so none of them stands out. The record
 * already knew - `tool_call_started` and `tool_result` both carry a `ts` - and
 * nothing was subtracting them.
 *
 * Paired per run and per tool name, most recent start first, because a run may
 * have two `bash` calls in flight and the second result belongs to the second
 * call. Unmatched starts are simply not shown yet, which is correct: a call that
 * has not returned has no duration, and inventing one would be a guess.
 */
function durations(events: RecordEvent[]): Map<string, number> {
  const out = new Map<string, number>();
  const open = new Map<string, RecordEvent[]>();
  // Oldest first, so a start is always seen before the result that closes it.
  for (const event of [...events].reverse()) {
    const name = (event.payload as Record<string, unknown> | null)?.["tool_name"];
    if (typeof name !== "string") continue;
    const key = `${event.run_id}/${name}`;
    if (event.kind === "tool_call_started") {
      open.set(key, [...(open.get(key) ?? []), event]);
    } else if (event.kind === "tool_result") {
      const started = open.get(key)?.shift();
      if (started) out.set(event.event_id, event.ts - started.ts);
    }
  }
  return out;
}

/** Wide enough to notice, bounded so one slow call cannot own the column. */
function Capsule({ ms }: { ms: number }) {
  const label = ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
  // Log scale: the interesting range spans milliseconds to minutes, and a
  // linear bar would render everything under ten seconds as a dot.
  const width = Math.max(6, Math.min(100, (Math.log10(Math.max(ms, 1)) / 5) * 100));
  const band = ms >= 60_000 ? "bad" : ms >= 5_000 ? "warn" : "ok";
  return (
    <span className="capsule" title={`${ms} ms`}>
      <span className="capsule-track">
        <span className={`capsule-bar ${band}`} style={{ width: `${width}%` }} />
      </span>
      <span className="capsule-label mono faint">{label}</span>
    </span>
  );
}

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
  const took = durations(events);

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
              {took.has(event.event_id) && <Capsule ms={took.get(event.event_id)!} />}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
