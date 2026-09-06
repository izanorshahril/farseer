import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow } from "../stream";
import { ReadFailure } from "../ReadFailure";

/**
 * The cells farseer has loaded, and what each one is.
 *
 * A second widget exists so the canvas has something to arrange - one tile is a
 * page, not a canvas - and so the bridge is exercised by more than the widget it
 * was written for.
 */
/** `GET /v1/cells` answers a summary, not a definition: id, name, description,
 *  version and roster size. The definition itself is one level down at
 *  `/v1/cells/{id}`, and a list widget has no business fetching every one. */
type Cell = {
  cell_id: string;
  name: string;
  description: string;
  version: string;
  roster_size: number;
  authority: {
    tool_level: "read" | "edit" | "shell";
    shell_grant: boolean;
    runners: { runner: string; tool_level: string; shell_reach: string }[];
    tools: { name: string; grants_shell: boolean; serving_path: boolean; authority: string }[];
  };
};

/** Only the two fields this widget needs off a run row. */
type Run = { cell_id: string; lifecycle: "running" | "finished" };

export function FleetWidget({ bridge }: { bridge: Bridge }) {
  const [cells, setCells] = useState<Cell[] | null>(null);
  /** How many runs each cell has in flight, keyed by cell id. */
  const [busy, setBusy] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);
  const [countError, setCountError] = useState<string | null>(null);

  // A definition list that never changes cannot answer the question an operator
  // has while watching: **is anything happening in this one?** Runs and Runners
  // both know, and neither is grouped by cell - so a cell with three workers
  // running looked exactly like a cell nobody has ever instructed.
  const count = useCallback(async () => {
    const runs = await bridge.read<Run[]>("/runs?limit=100");
    const live: Record<string, number> = {};
    for (const run of runs) {
      if (run.lifecycle === "running") live[run.cell_id] = (live[run.cell_id] ?? 0) + 1;
    }
    setBusy(live);
    setCountError(null);
  }, [bridge]);
  const load = useCallback(async () => {
    const body = await bridge.read<Cell[]>("/cells");
    setCells(body);
    setError(null);
  }, [bridge]);

  useEffect(() => {
    let live = true;
    void load().catch((e: Error) => live && setError(e.message));
    void count().catch((e: Error) => live && setCountError(e.message));
    // The stream is the trigger rather than a timer, the same choice the Runs
    // widget makes: a run changes state because something happened.
    const subscription = follow(() => void count().catch((e: Error) => live && setCountError(e.message)));
    return () => {
      live = false;
      subscription.close();
    };
  }, [count, load]);

  if (error && !cells) return <ReadFailure capability="cell fleet" error={error} onRetry={() => void load().catch((e: Error) => setError(e.message))} />;
  if (!cells) return <p className="empty">reading cells...</p>;
  // **Not just "none".** An installed farseer that finds no `cells/` opens a
  // console with an empty fleet and a composer whose every message will fail,
  // and the old wording described that as though it were a state the operator
  // had chosen. `01 cell primitive` makes a definition a plain file, so the
  // answer is a path - and the operator can act on a path.
  if (cells.length === 0)
    return (
      <p className="empty">
        No cell definitions loaded, so there is no top manager to talk to. farseer looks for a{" "}
        <span className="mono">cells/</span> directory in the working directory, then beside the
        executable, then in its own data directory. Put a <span className="mono">.toml</span>{" "}
        definition in one of those and use <b>reload</b> on the Settings widget.
      </p>
    );

  return (
    <>
      {error && <ReadFailure capability="cell fleet" error={error} stale onRetry={() => void load().catch((e: Error) => setError(e.message))} />}
      {countError && <ReadFailure capability="running count" error={countError} stale onRetry={() => void count().catch((e: Error) => setCountError(e.message))} />}
      <ul className="cells">
      {cells.map((cell) => (
        <li key={cell.cell_id}>
          <div className="row">
            <span className={`dot ${busy[cell.cell_id] ? "live" : "done"}`} aria-hidden />
            <b>{cell.name}</b>
            <span className="grow" />
            {busy[cell.cell_id] ? (
              <span className="badge allowed">
                {busy[cell.cell_id]} running
              </span>
            ) : (
              /* Said, not left blank: an idle cell and a cell whose runs this
                 widget failed to read must not look the same. */
              <span className="dim small">idle</span>
            )}
            <span className="badge">{cell.roster_size} in roster</span>
          </div>
          <p className="dim small">
            {cell.description || cell.cell_id} v{cell.version}
          </p>
          <details className="fleet-detail">
            <summary>authority detail</summary>
            <p className="dim small">
              Runner level <code>{cell.authority.tool_level}</code>:{" "}
              {cell.authority.runners
                .map((runner) => runner.runner + " " + runner.tool_level + "/" + runner.shell_reach)
                .join(", ") || "none"}.
              Shell reach is {cell.authority.shell_grant ? "explicitly granted" : "not granted"}.
            </p>
            {cell.authority.tools.length > 0 ? (
              <ul className="fleet-tools">
                {cell.authority.tools.map((tool) => (
                  <li key={tool.name}>
                    <code>{tool.name}</code> <span className="dim small">recorded only - no Farseer call</span>
                    {tool.grants_shell ? <span className="dim small">; grants shell reach, not per-tool containment</span> : null}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="dim small">No declared tools.</p>
            )}
          </details>
        </li>
      ))}
      </ul>
    </>
  );
}
