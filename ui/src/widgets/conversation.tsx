import { useEffect, useRef, useState } from "react";
import type { Bridge } from "../bridge";
import { onSubjectSelection, selectedSubject } from "../selection";
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
  sessions: { kind: string; id: string }[];
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
  /**
   * `farseer` is the runtime's own voice, and it exists because the runtime's
   * voice was being put in the manager's mouth. A run that died before the
   * model ever spoke ends with farseer's error as its text - a spawn failure,
   * a workspace that could not be made - and rendering that under "top manager"
   * claims an agent said something no agent was alive to say.
   */
  who: "operator" | "manager" | "farseer";
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
 * Decode immutable `session_started` records written before
 * `40 work model and session explorer` added an explicit `session_kind`.
 *
 * Current events carry the kind and never consult this frozen compatibility
 * table, so adding a new runner does not create another evolving registry.
 */
function legacySessionKind(runner: string): string {
  if (runner === "codex" || runner === "codex-app-server") return "thread";
  if (runner === "agy") return "conversation";
  return "session";
}

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
    const failed = payload["outcome"] === "failed";
    return {
      seq: event.seq,
      run: event.run_id,
      // Kept verbatim rather than replaced with a friendlier sentence: the
      // operator needs the exe and the argv to fix it, and `02 record scope`'s
      // whole argument is that the record says what happened. What changes is
      // **who is credited with saying it**.
      who: failed ? "farseer" : "manager",
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
  const initial = selectedSubject();
  const [subject, setSubject] = useState(initial);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [meta, setMeta] = useState<Meta>({ sessions: [] });
  const [error, setError] = useState<string | null>(null);
  const [showSilent, setShowSilent] = useState(false);
  const thread = useRef<HTMLOListElement>(null);

  useEffect(() => onSubjectSelection(setSubject), []);
  useEffect(() => {
    let live = true;
    setTurns([]);
    setMeta({ sessions: [] });
    setError(null);
    const add = (event: RecordEvent) => {
      const payload = (event.payload ?? {}) as Record<string, unknown>;
      const str = (key: string) =>
        typeof payload[key] === "string" ? (payload[key] as string) : undefined;
      const num = (key: string) =>
        typeof payload[key] === "number" ? (payload[key] as number) : undefined;

      if (event.kind === "session_started") {
        const id = str("session_id");
        const runner = str("runner") ?? "";
        const kind = str("session_kind") ?? legacySessionKind(runner);
        setMeta((current) => ({
          ...current,
          runner: runner || current.runner,
          model: str("model"),
          provider: str("provider"),
          effort: str("configured_effort"),
          effortFrom: str("configured_from"),
          sessions:
            id && !current.sessions.some((session) => session.kind === kind && session.id === id)
              ? [...current.sessions, { kind, id }]
              : current.sessions,
          cell: event.cell_id,
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

    const subscription = follow((event) => {
      if (subject.run === event.run_id) add(event);
    });
    if (!subject.conversation) {
      setTurns([]);
      setMeta({ sessions: [] });
      return () => subscription.close();
    }

    let runIds = new Set<string>();
    bridge
      .read<{ task_id: string }[]>(
        `/tasks?conversation_id=${encodeURIComponent(subject.conversation)}&limit=500`,
      )
      .then((tasks) =>
        Promise.all(
          tasks.map((task) =>
            bridge.read<{ runs: { run_id: string }[] }>(`/tasks/${task.task_id}`),
          ),
        ),
      )
      .then((details) => {
        if (!live) return;
        runIds = new Set(details.flatMap((detail) => detail.runs.map((run) => run.run_id)));
        return bridge.read<RecordEvent[]>("/events?tail=1000");
      })
      .then((events) => events?.filter((event) => runIds.has(event.run_id)).forEach(add))
      .catch((e: Error) => live && setError(e.message));

    const selectedSubscription = follow((event) => {
      if (runIds.has(event.run_id)) add(event);
    });
    return () => {
      live = false;
      subscription.close();
      selectedSubscription.close();
    };
  }, [bridge, subject.conversation, subject.task, subject.run]);

  // The newest turn is the one being waited for, and a thread that keeps its
  // scroll at the top hides exactly the line the operator is here to read.
  //
  // The thread owns its scroll so the runner metadata above it stays visible.
  // Keyed on the newest turn rather than on how many there are. The thread is
  // capped at 40, so **once it fills, the count stops changing** and an effect
  // watching the length never fires again - the widget scrolled itself for the
  // first forty turns and then quietly stopped, which is the worst version of
  // this bug because it works while you are testing it.
  const newest = turns.length > 0 ? turns[turns.length - 1]!.seq : 0;
  useEffect(() => {
    // After the frame, not during it: the thread takes what the flex column
    // leaves it, so its scroll height is not settled at the moment the turns
    // change - scrolling now lands short of the bottom on first paint.
    const frame = requestAnimationFrame(() => {
      const el = thread.current;
      if (el) el.scrollTop = el.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [newest]);

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

  const fields: [string, string | undefined][] = [
        ["cell", meta.cell],
        ["runner", meta.runner],
        ["model", meta.model],
        ["provider", meta.provider],
        // Labelled `configured` rather than `thinking`: the runner states what
        // it will reach for, and farseer never sets it, so calling it the level
        // this turn used would be a claim nobody made.
        ["configured effort", meta.effort],
        ["sessions", meta.sessions.length ? meta.sessions.map((session) => `${session.kind}:${session.id.slice(0, 8)}`).join(", ") : undefined],
        ["context", context(meta)],
        ["tokens", meta.tokens?.toLocaleString()],
        ["cost", typeof meta.cost === "number" ? usd(meta.cost) : undefined],
        ["last run", meta.outcome],
  ];
  const reported = fields.filter(([, value]) => value !== undefined);
  const silent = fields.filter(([, value]) => value === undefined);

  // `10 runner inventory`'s rule is that a blank means the runner declined, and
  // that is information. It is not information worth seven of the ten slots on
  // the tallest element in the widget - most runners report almost nothing, so
  // the strip was mostly a list of things that were never going to be there.
  // Folded, counted, and one click away, which keeps the fact without paying
  // full height for it every render.
  const strip = (
    <div className="meta">
      {reported.map(([label, value]) => (
        <span key={label} className={label === "context" ? pressure : ""}>
          <i>{label}</i>
          <b className="mono">{value}</b>
        </span>
      ))}
      {silent.length > 0 && (
        <button
          type="button"
          className="chip"
          aria-expanded={showSilent}
          onClick={() => setShowSilent((current) => !current)}
          title={silent.map(([label]) => label).join(", ")}
        >
          {showSilent ? "hide" : `${silent.length} not reported`}
        </button>
      )}
      {showSilent &&
        silent.map(([label]) => (
          <span key={label} className="absent">
            <i>{label}</i>
            <b className="mono">not reported</b>
          </span>
        ))}
    </div>
  );

  if (error) return <p className="empty bad">{error}</p>;
  if (!subject.conversation)
    return <p className="empty">Start or select a conversation in Work, then use the canvas composer.</p>;
  if (turns.length === 0)
    return (
      <>
        {strip}
        <p className="empty">Nothing said in this conversation yet.</p>
      </>
    );

  return (
    <>
      {strip}
      <ol className="thread" ref={thread}>
      {turns.map((turn) => (
        <li key={turn.seq} className={turn.who}>
          <div className="row small">
            <b>{turn.who === "operator" ? "you" : turn.who === "farseer" ? "farseer" : "top manager"}</b>
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
    </>
  );
}
