import type { ReactNode } from "react";

type Props = {
  capability: string;
  error: unknown;
  onRetry: () => void;
  onReduceScope?: () => void;
  stale?: boolean;
  children?: ReactNode;
};

/** Keep backend diagnostics useful without copying paths, query values, or tokens into the UI. */
export function readFailureStatus(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error ?? "");
  const match = message.match(/\b(?:HTTP|status:?)[\s/:]*(\d{3})\b|:\s*(\d{3})\b/i);
  return match?.[1] ?? match?.[2] ?? "unknown";
}

/** A stable correlation label for one capability and one failure class. */
export function readFailureIncident(capability: string, error: unknown): string {
  const input = `${capability}:${readFailureStatus(error)}`;
  let hash = 0;
  for (const char of input) hash = (hash * 31 + char.charCodeAt(0)) | 0;
  return `read-${capability.replace(/[^a-z0-9]+/gi, "-").toLowerCase()}-${Math.abs(hash).toString(36)}`;
}

/** Localized read failure with retry and bounded diagnostics. */
export function ReadFailure({ capability, error, onRetry, onReduceScope, stale, children }: Props) {
  const status = readFailureStatus(error);
  const incident = readFailureIncident(capability, error);
  return (
    <div className="read-failure" role="alert">
      <div className="row small">
        <b>{capability} unavailable</b>
        <span className="grow" />
        <span className="badge bad">{stale ? "stale projection" : "read failed"}</span>
      </div>
      <p className="dim small">
        {stale ? "Showing the last successful data while farseer reconnects." : "This read did not complete."}
      </p>
      {children}
      <div className="row small">
        <button className="chip" onClick={onRetry}>retry</button>
        {onReduceScope && <button className="chip" onClick={onReduceScope}>reduce scope</button>}
        <details className="read-diagnostics">
          <summary className="chip">diagnostics</summary>
          <span className="dim small mono">incident {incident} · status {status}</span>
        </details>
      </div>
    </div>
  );
}
