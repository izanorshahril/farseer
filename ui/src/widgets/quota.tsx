import { useEffect, useState } from "react";
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

export function QuotaWidget({ bridge }: { bridge: Bridge }) {
  const [windows, setWindows] = useState<Window[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    let live = true;
    const load = () =>
      bridge
        .read<{ windows: Window[] }>("/quota")
        .then((body) => live && setWindows(body.windows))
        .catch((e: Error) => live && setError(e.message));
    load();
    const tick = setInterval(() => {
      setNow(Date.now());
      load();
    }, 30_000);
    return () => {
      live = false;
      clearInterval(tick);
    };
  }, [bridge]);

  if (error) return <p className="empty">quota unavailable - {error}</p>;
  if (!windows) return <p className="empty">reading windows...</p>;
  if (windows.length === 0)
    return (
      <p className="empty">
        No window observed yet. A window appears the first time a runner reports one, which is
        after its first successful run.
      </p>
    );

  // Grouped in first-seen order rather than sorted, so the list does not
  // reshuffle under the operator every thirty seconds.
  const accounts: { account: string; runners: string[]; windows: Window[] }[] = [];
  for (const w of windows) {
    const found = accounts.find((a) => a.account === w.account);
    if (found) found.windows.push(w);
    else accounts.push({ account: w.account, runners: w.runners, windows: [w] });
  }

  return (
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
  );
}
