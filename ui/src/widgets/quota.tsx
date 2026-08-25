import { useEffect, useState } from "react";
import type { Bridge } from "../bridge";

/**
 * `27 quota accounting`'s utilisation surface, as a widget.
 *
 * **There is no progress bar here, and that is the whole point.** Farseer's own
 * spend is a lower bound on a window drained by sessions it cannot see, so a bar
 * would be wrong in a way the operator could not detect - and most wrong exactly
 * near exhaustion, when they would trust it most. What it shows instead is what
 * is actually true: allowed or exhausted, a countdown, and what the fleet spent.
 */
type Window = {
  account: string;
  status: "allowed" | "exhausted_until" | "unknown";
  resets_at: number | null;
  rate_limit_type: string;
  is_using_overage: boolean;
  since_ts: number;
  farseer_usd_micros: number;
  farseer_tokens: number;
  runners: string[];
};

const usd = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;

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
        <li key={window.account}>
          <div className="row">
            <span className={`badge ${window.status}`}>{window.status.replace("_", " ")}</span>
            <b className="mono">{window.account}</b>
            {window.is_using_overage && <span className="badge overage">overage</span>}
          </div>
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
