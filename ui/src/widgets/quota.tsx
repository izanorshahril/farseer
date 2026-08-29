import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";

/**
 * `27 quota accounting`'s utilisation surface, as a widget.
 *
 * **Farseer's own spend never becomes a progress bar, and that is the whole
 * point.** It is a lower bound on a window drained by sessions farseer cannot
 * see, so a bar would be wrong in a way the operator could not detect - and most
 * wrong exactly near exhaustion, when they would trust it most.
 *
 * `30 codex app server` found a runner that states **its own** percentage, which
 * is a different number reached a different way. That one is shown, labelled as
 * the provider's, beside farseer's spend rather than instead of it - and a
 * window whose runner says nothing still shows no percentage at all.
 *
 * **Grouped by account, tiled by window.** One account has several windows at
 * once - Codex reports a five-hour and a weekly - and listing them as separate
 * rows made four readings of two subscriptions look like four subscriptions.
 * The account is the thing an operator thinks in; the windows are its shape.
 */
type Window = {
  account: string;
  status: "allowed" | "exhausted_until" | "unknown";
  resets_at: number | null;
  rate_limit_type: string;
  is_using_overage: boolean;
  /** The provider's own reading. Absent for every runner that does not state one. */
  used_percent?: number;
  window_duration_mins?: number;
  since_ts: number;
  farseer_usd_micros: number;
  farseer_tokens: number;
  runners: string[];
  /** `record` for a window a run reported, or the poll that read it. */
  source?: string;
};

const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;

/**
 * The window's own name for itself, shortened to what an operator reads.
 *
 * `10 runner inventory` transcribed `rateLimitType` rather than renaming it, so
 * these are provider words - `primary`, `secondary`, `5h`, `7d`. A duration is
 * more use than a rank when two sit side by side, so it wins when reported.
 */
function windowName(w: Window): string {
  const mins = w.window_duration_mins;
  if (mins) {
    if (mins % (60 * 24) === 0) return `${mins / (60 * 24)} day`;
    if (mins % 60 === 0) return `${mins / 60} hour`;
  }
  return w.rate_limit_type.replace(/[_:]/g, " ") || "window";
}

function countdown(resetsAt: number | null, now: number): string {
  if (resetsAt === null) return "no reset reported";
  const seconds = resetsAt - Math.floor(now / 1000);
  if (seconds <= 0) return "reset due";
  const days = Math.floor(seconds / 86_400);
  if (days > 0) return `${days}d ${Math.floor((seconds % 86_400) / 3600)}h`;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m`;
}

/**
 * A meter, for a number the provider stated.
 *
 * **Only ever rendered from `used_percent`.** `27 quota accounting` refused a
 * bar built from farseer's own spend, because that spend is a *lower bound* on a
 * window drained by sessions farseer cannot see - it would be most wrong exactly
 * near exhaustion, which is when an operator would trust it most. A bar reads as
 * a measurement whatever the caption says, so the rule is enforced by this
 * component taking a percentage and nothing else.
 */
function Meter({ percent, exhausted }: { percent: number; exhausted: boolean }) {
  const clamped = Math.max(0, Math.min(100, percent));
  const band = exhausted || clamped >= 90 ? "bad" : clamped >= 70 ? "warn" : "ok";
  return (
    <div
      className="meter"
      role="meter"
      aria-valuenow={clamped}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label="reported by the provider"
      title={`${clamped}% used, reported by the provider`}
    >
      <span className={`meter-fill ${band}`} style={{ width: `${clamped}%` }} />
    </div>
  );
}

/** One window, as a tile: name, the provider's reading, and when it turns over. */
function WindowTile({ window: w, now }: { window: Window; now: number }) {
  return (
    <div className="tile">
      <div className="row">
        <b className="small">{windowName(w)}</b>
        <span className="grow" />
        {w.is_using_overage && <span className="badge overage">overage</span>}
        {w.status === "exhausted_until" ? (
          <span className="badge exhausted_until">spent</span>
        ) : w.used_percent !== undefined ? (
          <span className="figure mono" title="reported by the provider">
            {w.used_percent}%
          </span>
        ) : (
          <span className="dim small" title="this runner states no percentage">
            not stated
          </span>
        )}
      </div>
      {/* Absent for every runner that states nothing, which is most of them.
          No bar at all is the honest rendering of "nobody said". */}
      {w.used_percent !== undefined && (
        <Meter percent={w.used_percent} exhausted={w.status === "exhausted_until"} />
      )}
      <div className="row dim small">
        <span>{countdown(w.resets_at, now)}</span>
        <span className="grow" />
        {/* Per window rather than per account, because that is what the number
            is: farseer's spend since **this** window was first seen. Two windows
            on one account began at different moments, so adding them would
            invent a total nothing measured. */}
        <span title="farseer's spend since it first saw this window">
          {usd(w.farseer_usd_micros)}
        </span>
      </div>
    </div>
  );
}

/**
 * How often the widget re-reads, in seconds. `0` is off.
 *
 * A window that a run reports moves when a run happens; the polled one moves
 * every five minutes at most, because it costs a process launch. So the useful
 * range is "faster than I can hit the button" to "leave it alone" - and the
 * default stays what it has always been.
 */
const INTERVALS: [string, number][] = [
  ["every 10s", 10],
  ["every 30s", 30],
  ["every 1m", 60],
  ["every 5m", 300],
  // The whole phrase, not a suffix: prefixing "every" made this read "every
  // off", which is the cost of building a label out of a template.
  ["no auto-refresh", 0],
];

const DEFAULT_INTERVAL = 30;

/** What this widget keeps in `24 ui state persistence`'s blob, under its own key. */
type Prefs = { intervalSecs: number };

export function QuotaWidget({ bridge }: { bridge: Bridge }) {
  const [windows, setWindows] = useState<Window[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const [intervalSecs, setIntervalSecs] = useState(DEFAULT_INTERVAL);
  /** When the numbers on screen were last fetched, so "live" is checkable. */
  const [readAt, setReadAt] = useState<number | null>(null);
  const [polling, setPolling] = useState(false);

  const load = useCallback(
    () =>
      bridge
        .read<{ windows: Window[] }>("/quota")
        .then((body) => {
          setWindows(body.windows);
          setReadAt(Date.now());
          setError(null);
        })
        .catch((e: Error) => setError(e.message)),
    [bridge],
  );

  /**
   * Ask the runner **now**, rather than waiting for farseer's own five-minute
   * poll. Refused by the runtime when no runner is configured to poll, and the
   * refusal names the line to add - which is the whole reason the button says
   * something rather than doing nothing.
   */
  const refresh = useCallback(async () => {
    setPolling(true);
    setError(null);
    try {
      await bridge.post("/quota/refresh");
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setPolling(false);
    }
  }, [bridge, load]);

  useEffect(() => {
    bridge
      .loadState<Prefs>("quota")
      .then((stored) => stored && setIntervalSecs(stored.intervalSecs))
      .catch(() => undefined);
  }, [bridge]);

  useEffect(() => {
    void load();
    // The countdowns tick every second whatever the reload interval is: they
    // are computed from `resets_at` and need no request to stay right, and a
    // clock that only moves every thirty seconds looks stopped.
    const clock = setInterval(() => setNow(Date.now()), 1_000);
    if (intervalSecs === 0) return () => clearInterval(clock);
    const tick = setInterval(() => void load(), intervalSecs * 1_000);
    return () => {
      clearInterval(clock);
      clearInterval(tick);
    };
  }, [load, intervalSecs]);

  const chooseInterval = (secs: number) => {
    setIntervalSecs(secs);
    void bridge.saveState("quota", { intervalSecs: secs } satisfies Prefs).catch(() => undefined);
  };

  const controls = (
    <div className="row small quota-controls">
      <button className="chip" disabled={polling} onClick={() => void refresh()}>
        {polling ? "asking the runner" : "refresh now"}
      </button>
      <span className="grow" />
      <span className="faint">
        {readAt === null
          ? "not read yet"
          : `read ${Math.max(0, Math.round((now - readAt) / 1000))}s ago`}
      </span>
      <select
        className="chip"
        aria-label="how often to re-read"
        value={intervalSecs}
        onChange={(event) => chooseInterval(Number(event.target.value))}
      >
        {INTERVALS.map(([label, secs]) => (
          <option key={secs} value={secs}>
            {label}
          </option>
        ))}
      </select>
    </div>
  );

  // The controls stay on screen in every state, including the failing one: a
  // widget that hides its refresh button exactly when the read is broken hides
  // the control the operator came for, and shows them a message about a setting
  // with nothing to click afterwards.
  if (!windows && !error) return <p className="empty">reading windows...</p>;

  // Grouped in first-seen order rather than sorted, so the list does not
  // reshuffle under the operator every thirty seconds.
  const accounts: { account: string; runners: string[]; windows: Window[] }[] = [];
  for (const w of windows ?? []) {
    const found = accounts.find((a) => a.account === w.account);
    if (found) found.windows.push(w);
    else accounts.push({ account: w.account, runners: w.runners, windows: [w] });
  }

  return (
    <>
      {controls}
      {error && <p className="empty bad">{error}</p>}
      {accounts.length === 0 && !error && (
        <p className="empty">
          No window observed yet. A window appears the first time a runner reports one, which is
          after its first successful run.
        </p>
      )}
      <ul className="accounts">
      {accounts.map((account) => (
        <li key={account.account}>
          <div className="row">
            <b className="mono">{account.account}</b>
            <span className="grow" />
            <span className="dim small">{account.runners.join(", ")}</span>
          </div>
          <div className="tiles">
            {account.windows.map((w) => (
              <WindowTile key={w.rate_limit_type} window={w} now={now} />
            ))}
          </div>
        </li>
      ))}
      </ul>
    </>
  );
}
