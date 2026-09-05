import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow, type RecordEvent } from "../stream";
import { onSelection, selectRun, selectedRun } from "../selection";
import { confirmVerb } from "../confirm";
import { meaningOf } from "../meaning";

/**
 * One run, whole: what it was told to do, everything it did, and how it ended.
 *
 * `28 operator surface` named this and refused to fake it. Its verb table put
 * `steer` and `cancel` inline on the fleet line and held `re-run` and `re-scope`
 * back, because **both start a new run and re-scope changes a contract field**,
 * so the contract has to be on screen before either is offered. The Runs widget
 * said so in its own doc comment and then had nowhere to send the operator.
 *
 * Every field here comes out of the record rather than out of a second table.
 * `run_queued` carries the whole sealed contract - `05 run state model` made it
 * immutable precisely so that "what was this worker allowed to do" has one
 * answer - so the contract is read from the run's own first event, and the
 * trajectory is the rest of them. Nothing is stored twice and nothing can drift.
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
  operator_touched: boolean;
  started_ts: number;
  finished_ts: number | null;
  liveness: "live" | "stalled" | "likely_hung" | null;
  title: string | null;
  role: string | null;
  finished_reason: string | null;
};

/**
 * The sealed contract, as `run_queued` carries it.
 *
 * Loosely typed on purpose: this is a snapshot of a struct the runtime owns,
 * and a field added there should appear here rather than being silently dropped
 * by a type that was written before it existed. The named fields are the ones
 * this view lays out; the rest are listed as they come.
 */
type Contract = Record<string, unknown> & {
  goal?: string;
  runner?: string;
  tool_level?: string;
  tool_grants?: string[];
  autonomy_ceiling?: string;
  definition_of_done?: string;
  workspace?: unknown;
  budget?: { tokens?: number | null; usd_micros?: number | null; wall_secs?: number | null };
  skills?: string[];
};

const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(4)}`;
const time = (ts: number) => new Date(ts).toLocaleTimeString(undefined, { hour12: false });

/** Wall time from queue to exit, or to now while it is still going. */
function elapsed(run: Run, now: number): string {
  const ms = (run.finished_ts ?? now) - run.started_ts;
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1000)}s`;
}

/**
 * How long each tool call took, paired within this run.
 *
 * The Activity widget does this across the whole feed and loses a pair whose
 * start has scrolled out of its 60-event window. Scoped to one run there is no
 * window: every event this run produced is here, so every call that returned
 * has its duration.
 */
function durations(events: RecordEvent[]): Map<string, number> {
  const out = new Map<string, number>();
  const open = new Map<string, RecordEvent[]>();
  for (const event of events) {
    const name = (event.payload as Record<string, unknown> | null)?.["tool_name"];
    if (typeof name !== "string") continue;
    if (event.kind === "tool_call_started") {
      open.set(name, [...(open.get(name) ?? []), event]);
    } else if (event.kind === "tool_result") {
      const started = open.get(name)?.shift();
      if (started) out.set(event.event_id, event.ts - started.ts);
    }
  }
  return out;
}

/** One line of the trajectory, in the record's own words. */
function summarise(event: RecordEvent): string {
  const payload = event.payload as Record<string, unknown> | null;
  if (!payload) return "";
  for (const key of ["text", "tool_name", "result", "reason", "status", "outcome"]) {
    const value = payload[key];
    if (typeof value === "string" && value.trim()) return value.slice(0, 400);
  }
  return "";
}

const TONE: Record<string, string> = {
  run_finished: "ok",
  run_queued: "dim",
  operator_intervened: "accent",
  manager_steered: "accent",
  cell_called: "accent",
  rate_limit_event: "warn",
};

/**
 * What the words on this screen mean, for somebody reading them for the first
 * time.
 *
 * The vocabulary here is load-bearing and invented - `cell`, `ceiling`, `tool
 * level`, `sealed contract` - and it was explained only in source comments,
 * which the operator never sees. The same discipline this codebase applies to
 * numbers it did not observe applies to words it did not teach. The dictionary
 * itself now lives in `meaning.ts`, because the words are used on widgets this
 * one does not own.
 */

/** A labelled fact, absent-aware, because a blank and a zero are not the same. */
function Fact({ label, value }: { label: string; value: string | undefined }) {
  return (
    <span className={value ? "" : "absent"}>
      <i title={meaningOf(label)}>{label}</i>
      <b className="mono">{value ?? "not stated"}</b>
    </span>
  );
}

export function RunWidget({ bridge }: { bridge: Bridge }) {
  const [runId, setRunId] = useState<string | null>(selectedRun());
  const [run, setRun] = useState<Run | null>(null);
  const [events, setEvents] = useState<RecordEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  /** The re-scope draft. `null` means the editor is closed. */
  const [draft, setDraft] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => onSelection(setRunId), []);

  const load = useCallback(
    async (id: string) => {
      try {
        const [row, trajectory] = await Promise.all([
          bridge.read<Run>(`/runs/${id}`),
          bridge.read<RecordEvent[]>(`/events?run=${id}`),
        ]);
        setRun(row);
        setEvents(trajectory);
        setError(null);
      } catch (e) {
        setError((e as Error).message);
      }
    },
    [bridge],
  );

  useEffect(() => {
    if (!runId) {
      setRun(null);
      setEvents([]);
      setNote(null);
      setDraft(null);
      return;
    }
    void load(runId);
    // Only this run's events re-read it. The stream is shared, so the filter is
    // here rather than in a second connection - see `stream.ts`.
    const subscription = follow((event) => {
      if (event.run_id === runId) void load(runId);
    });
    const clock = setInterval(() => setNow(Date.now()), 1_000);
    return () => {
      subscription.close();
      clearInterval(clock);
    };
  }, [runId, load]);

  const act = async (verb: "cancel" | "rerun" | "rescope", body?: unknown) => {
    if (!runId) return;
    setBusy(verb);
    setNote(null);
    try {
      const answer = (await bridge.post(`/runs/${runId}/${verb}`, body)) as
        | { run_id?: string }
        | undefined;
      // A re-run and a re-scope are **new runs**, so the view follows the new
      // one rather than leaving the operator looking at the old contract and
      // wondering whether the button worked.
      if (answer?.run_id) {
        selectRun(null);
        selectRun(answer.run_id);
        setNote(null);
      } else {
        setNote(`${verb} accepted`);
      }
      setDraft(null);
    } catch (e) {
      setNote((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  if (!runId)
    return (
      <p className="empty">
        No run selected. Click a run&apos;s name in the Runs widget to open it here - its
        contract, everything it did, and the verbs that need both on screen.
      </p>
    );
  if (error) return <p className="empty bad">{error}</p>;
  if (!run) return <p className="empty">reading the run...</p>;

  const queued = events.find((event) => event.kind === "run_queued");
  const contract = (queued?.payload ?? {}) as Contract;
  const took = durations(events);
  const running = run.lifecycle === "running";

  return (
    <>
      <div className="row" style={{ marginBottom: 8 }}>
        <b>{run.title ?? run.run_id.slice(0, 8)}</b>
        <span className="grow" />
        <span className="faint mono small">{run.run_id.slice(0, 8)}</span>
        <button className="chip" onClick={() => selectRun(null)} title="close this run">
          close
        </button>
      </div>

      <div className="meta">
        <Fact label="cell" value={run.cell_id} />
        <Fact label="role" value={run.role ?? undefined} />
        <Fact label="runner" value={run.runner} />
        <Fact label="model" value={run.model || undefined} />
        <Fact label="state" value={running ? (run.liveness ?? "running") : (run.outcome ?? "")} />
        <Fact label="took" value={elapsed(run, now)} />
        <Fact label="cost" value={run.usd_micros > 0 ? usd(run.usd_micros) : undefined} />
        <Fact label="tokens" value={run.tokens > 0 ? run.tokens.toLocaleString() : undefined} />
      </div>

      {/* Why it ended that way, when the record says. A screen of `failed`
          after a restart reads as breakage until this line names the reaper. */}
      {run.finished_reason && <p className="dim small">{run.finished_reason}</p>}

      {/* **The contract, on screen.** `28`'s condition for offering re-scope at
          all, and `05`'s reason for sealing it: one answer to what this worker
          was allowed to do, not a timeline of answers. */}
      <h4
        className="section"
        title="Sealed when the run started and immutable for its life, so `what was this allowed to do` has one answer rather than a timeline of them"
      >
        the contract it was sealed with
      </h4>
      {queued ? (
        <>
          <p className="asked">{contract.goal ?? "no goal recorded"}</p>
          <div className="meta">
            <Fact label="tool level" value={contract.tool_level} />
            <Fact label="ceiling" value={contract.autonomy_ceiling} />
            <Fact
              label="tool grants"
              value={contract.tool_grants?.length ? contract.tool_grants.join(", ") : undefined}
            />
            {/* Paths, because that is what reached the argv - see `32`. */}
            <Fact
              label="skills"
              value={contract.skills?.length ? contract.skills.join(", ") : undefined}
            />
            <Fact
              label="budget"
              value={
                contract.budget && Object.values(contract.budget).some((v) => v != null)
                  ? Object.entries(contract.budget)
                      .filter(([, v]) => v != null)
                      .map(([k, v]) => `${k} ${v}`)
                      .join(", ")
                  : undefined
              }
            />
            <Fact label="done when" value={contract.definition_of_done || undefined} />
          </div>
        </>
      ) : (
        // `run_queued` missing is a corrupt run rather than an untitled one, and
        // saying so beats rendering an empty contract as though it were real.
        <p className="empty">
          This run has no <span className="mono">run_queued</span> event, so farseer has no record
          of what it was sealed with.
        </p>
      )}

      <h4 className="section">what it did</h4>
      <ul className="feed">
        {events.map((event) => (
          <li key={event.event_id}>
            <span className="mono faint">{time(event.ts)}</span>
            <span className={`kind ${TONE[event.kind] ?? ""}`}>{event.kind}</span>
            <span className="summary">{summarise(event)}</span>
            {took.has(event.event_id) && (
              <span className="mono faint">{(took.get(event.event_id)! / 1000).toFixed(1)}s</span>
            )}
          </li>
        ))}
      </ul>

      <div className="row" style={{ marginTop: 10 }}>
        {running && (
          <button
            className="chip danger"
            disabled={busy !== null}
            onClick={() => {
              if (confirmVerb("cancel", run.title ?? run.run_id.slice(0, 8))) void act("cancel");
            }}
          >
            {busy === "cancel" ? "..." : "cancel"}
          </button>
        )}
        {/* Offered only with the contract above them, which is the whole reason
            this widget exists rather than these sitting on the fleet line. */}
        <button className="chip" disabled={busy !== null} onClick={() => void act("rerun")}>
          {busy === "rerun" ? "..." : "re-run"}
        </button>
        <button
          className="chip"
          disabled={busy !== null}
          onClick={() => setDraft(draft === null ? (contract.goal ?? "") : null)}
        >
          re-scope
        </button>
        <span className="grow" />
        {note && <span className="dim small">{note}</span>}
      </div>

      {draft !== null && (
        <form
          className="composer"
          onSubmit={(event) => {
            event.preventDefault();
            void act("rescope", { goal: draft });
          }}
        >
          <textarea
            rows={3}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="The changed goal. Everything else on the contract carries over."
          />
          <button className="chip on" disabled={busy !== null || draft.trim() === ""}>
            {busy === "rescope" ? "..." : "start"}
          </button>
        </form>
      )}
    </>
  );
}
