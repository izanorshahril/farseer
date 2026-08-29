import { useEffect, useRef, useState } from "react";
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
/**
 * What the runner said about itself, plus what the run cost.
 *
 * Every field is optional because **every field is a claim some runner chose to
 * make**: Claude Code names a model and a session, Codex names a thread and no
 * model at all, and `10 runner inventory`'s rule is that farseer reports what it
 * observed rather than what was configured. A blank here means the runner
 * declined, which is information rather than a gap to fill in.
 */
type Meta = {
  runner?: string;
  model?: string;
  /** The account the runner says it is using, which is not what was configured. */
  provider?: string;
  /** A **hint**: what the runner is configured to reach for, not what this turn used. */
  effort?: string;
  effortFrom?: string;
  session_id?: string;
  cell?: string;
  cost?: number;
  tokens?: number;
  outcome?: string;
  /** Context tokens in use, and the window they are used out of. */
  used?: number;
  size?: number;
};

/**
 * `used / size`, with the percentage that makes it actionable.
 *
 * Absent unless the runner said both. A `used` with no `size` is a token count
 * without a denominator, which is what every non-ACP runner reports and is not
 * a context reading.
 */
function context(meta: Meta): string | undefined {
  if (meta.used === undefined || !meta.size) return undefined;
  const pct = Math.round((meta.used / meta.size) * 100);
  return `${meta.used.toLocaleString()} / ${meta.size.toLocaleString()} (${pct}%)`;
}

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

  if (event.kind === "session_started") {
    return null;
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

/**
 * Type to the top manager from inside the conversation.
 *
 * The canvas has always had one composer, in the footer, and `28 operator
 * surface`'s rule is why: **the widget you type from is the anchor, never the
 * destination.** That rule is about where the work goes, not about where the
 * box is - so a second composer here breaks nothing, and answers the thing the
 * footer could not: a reply belongs under the thing it replies to.
 *
 * It calls the same `bridge.ask`, anchored to this widget, and gets back a run
 * id rather than an answer. The answer arrives on the stream and lands in the
 * thread above, which is what makes this a conversation rather than a form.
 */
function Composer({ bridge }: { bridge: Bridge }) {
  const [sending, setSending] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);

  const send = async () => {
    const text = field.current?.value.trim();
    if (!text || sending) return;
    setSending(true);
    setFailed(null);
    try {
      await bridge.ask({ widget: "Conversation" }, text);
      if (field.current) field.current.value = "";
    } catch (e) {
      setFailed((e as Error).message);
    } finally {
      setSending(false);
    }
  };

  return (
    <form
      className="composer"
      onSubmit={(event) => {
        event.preventDefault();
        void send();
      }}
    >
      <textarea
        ref={field}
        rows={2}
        disabled={sending}
        placeholder="Say something to the top manager - Enter sends, Shift+Enter for a new line"
        // A textarea rather than an input, because a goal is usually a
        // paragraph. Enter still sends, since the common case is one line.
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            void send();
          }
        }}
      />
      <button className="chip on" disabled={sending}>
        {sending ? "sending" : "send"}
      </button>
      {failed && <span className="bad small">{failed}</span>}
    </form>
  );
}

export function ConversationWidget({ bridge }: { bridge: Bridge }) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [meta, setMeta] = useState<Meta>({});
  const [error, setError] = useState<string | null>(null);
  const thread = useRef<HTMLOListElement>(null);

  useEffect(() => {
    let live = true;
    const add = (event: RecordEvent) => {
      const payload = (event.payload ?? {}) as Record<string, unknown>;
      const str = (key: string) =>
        typeof payload[key] === "string" ? (payload[key] as string) : undefined;
      const num = (key: string) =>
        typeof payload[key] === "number" ? (payload[key] as number) : undefined;

      // The meta describes the latest session, so later events win. A run that
      // never named a model leaves the field absent rather than stale.
      if (event.kind === "session_started") {
        setMeta((current) => ({
          ...current,
          runner: str("runner") ?? current.runner,
          model: str("model"),
          provider: str("provider"),
          effort: str("configured_effort"),
          effortFrom: str("configured_from"),
          session_id: str("session_id"),
          cell: event.cell_id,
          // Cleared, not carried. A new session has spent nothing yet, and a
          // runner that reports no context window must not inherit the last
          // one's - `pi` reporting neither was showing a `codex-app-server`
          // context reading under a pi session id, which is precisely the
          // absent-because-unreportable / absent-because-nothing-happened
          // confusion `10 runner inventory`'s rule exists to prevent.
          used: undefined,
          size: undefined,
          tokens: undefined,
          cost: undefined,
          outcome: undefined,
        }));
        return;
      }
      // Cumulative, so the latest reading is the answer. Only an ACP runner
      // sends a `size`; the rest never say how big the window is, so the field
      // stays absent rather than showing a fraction with a made-up denominator.
      if (event.kind === "usage_updated") {
        setMeta((current) => ({
          ...current,
          cell: event.cell_id,
          used: num("used") ?? current.used,
          size: num("size") ?? current.size,
          cost: num("cost_usd_micros") ?? current.cost,
        }));
        return;
      }
      if (event.kind === "run_finished") {
        setMeta((current) => ({
          ...current,
          cell: event.cell_id,
          outcome: str("outcome"),
          cost: num("cost_usd_micros"),
          tokens: num("tokens"),
        }));
      }

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

    // Replay what was already said, then follow. `tail` rather than `limit`: a
    // surface opening cold wants what just happened, and reading forward from
    // zero gives it the oldest 400 events instead - which looked right while
    // the log was shorter than the limit and would have frozen silently.
    bridge
      .read<RecordEvent[]>("/events?tail=400")
      .then((events) => live && events.forEach(add))
      .catch((e: Error) => live && setError(e.message));
    const subscription = follow(add);
    return () => {
      live = false;
      subscription.close();
    };
  }, [bridge]);

  // The newest turn is the one being waited for, and a thread that keeps its
  // scroll at the top hides exactly the line the operator is here to read.
  //
  // The thread scrolls, not the widget body: a composer that scrolls away with
  // the messages is a composer the operator has to go looking for, and it is
  // the one control on this widget.
  useEffect(() => {
    // After the frame, not during it: the thread takes what the flex column
    // leaves it, so its scroll height is not settled at the moment the turns
    // change - scrolling now lands short of the bottom on first paint.
    const frame = requestAnimationFrame(() => {
      const el = thread.current;
      if (el) el.scrollTop = el.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [turns.length]);

  // 75-90% is worth noticing and above 95% the next prompt may not fit, which
  // is the convention ACP's clients already settled on - farseer does not need
  // a second one.
  const pressure =
    meta.used !== undefined && meta.size
      ? meta.used / meta.size >= 0.95
        ? "bad"
        : meta.used / meta.size >= 0.75
          ? "warn"
          : ""
      : "";

  const strip = (
    <div className="meta">
      {[
        ["cell", meta.cell],
        ["runner", meta.runner],
        ["model", meta.model],
        ["provider", meta.provider],
        // Labelled `configured` rather than `thinking`: the runner states what
        // it will reach for, and farseer never sets it, so calling it the level
        // this turn used would be a claim nobody made.
        ["configured effort", meta.effort],
        ["session", meta.session_id?.slice(0, 8)],
        ["context", context(meta)],
        ["tokens", meta.tokens?.toLocaleString()],
        ["cost", typeof meta.cost === "number" ? usd(meta.cost) : undefined],
        ["last run", meta.outcome],
      ].map(([label, value]) => (
        <span
          key={label}
          className={[value ? "" : "absent", label === "context" ? pressure : ""]
            .filter(Boolean)
            .join(" ")}
        >
          <i>{label}</i>
          <b className="mono">{value ?? "not reported"}</b>
        </span>
      ))}
    </div>
  );

  if (error) return <p className="empty bad">{error}</p>;
  if (turns.length === 0)
    return (
      <>
        {strip}
        <p className="empty">
          Nothing said yet. Type below - it goes to the top manager, and its answer lands here
          when the run finishes.
        </p>
        <Composer bridge={bridge} />
      </>
    );

  return (
    <>
      {strip}
      <ol className="thread" ref={thread}>
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
      <Composer bridge={bridge} />
    </>
  );
}
