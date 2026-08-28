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
 * One account can now have several windows: Codex reports a five-hour and a
 * weekly, so the list is keyed by account **and** limit.
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
};

const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;

/**
 * A meter, for a number the provider stated.
 *
 * **Only ever rendered from `used_percent`.** `27 quota accounting` refused a
 * bar built from farseer's own spend, because that spend is a *lower bound* on a
 * window drained by sessions farseer cannot see - it would be most wrong exactly
 * near exhaustion, which is when an operator would trust it most. A bar reads as
 * a measurement whatever the caption says, so the rule is enforced by this
 * component taking a percentage and nothing else.
 *
 * Bands rather than a gradient: an operator reads position, and three states
 * they can name beat a hue they have to interpret.
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

function countdown(resetsAt: number | null, now: number): string {
  if (resetsAt === null) return "no reset reported";
  const seconds = resetsAt - Math.floor(now / 1000);
  if (seconds <= 0) return "reset due";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return hours > 0 ? `resets in ${hours}h ${minutes}m` : `resets in ${minutes}m`;
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

  return (
    <ul className="windows">
      {windows.map((window) => (
        <li key={`${window.account}/${window.rate_limit_type}`}>
          <div className="row">
            <span className={`badge ${window.status}`}>{window.status.replace("_", " ")}</span>
            <b className="mono">{window.account}</b>
            {window.rate_limit_type && (
              <span className="dim small">{window.rate_limit_type.replace("_", " ")}</span>
            )}
            {window.is_using_overage && <span className="badge overage">overage</span>}
            <span className="grow" />
            {/* Stated, never derived. The wording says whose number it is,
                because the one farseer could calculate is the one `27 quota
                accounting` refused. */}
            {window.used_percent !== undefined && (
              <span className="figure mono" title="reported by the provider">
                {window.used_percent}% used
              </span>
            )}
          </div>
          {/* Absent for every runner that states nothing, which is most of them.
              No bar at all is the honest rendering of "nobody said". */}
          {window.used_percent !== undefined && (
            <Meter
              percent={window.used_percent}
              exhausted={window.status === "exhausted_until"}
            />
          )}
          <div className="row dim">
            <span>{window.runners.join(", ")}</span>
            <span className="grow" />
            <span>{countdown(window.resets_at, now)}</span>
          </div>
          <div className="row">
            <span className="figure mono">{usd(window.farseer_usd_micros)}</span>
            <span className="dim">{window.farseer_tokens.toLocaleString()} tokens</span>
            <span className="grow" />
            {/* The label is load-bearing, not decoration. Two things it has to
                get right: this is not a share of the window, and it is counted
                from when farseer **first saw** the window rather than from when
                the provider opened it - a run already in flight at that moment
                is not in this number. */}
            <span className="dim small">farseer's spend since it first saw this window</span>
          </div>
        </li>
      ))}
    </ul>
  );
}
