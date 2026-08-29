import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { follow } from "../stream";

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
};

/** Only the two fields this widget needs off a run row. */
type Run = { cell_id: string; lifecycle: "running" | "finished" };

export function FleetWidget({ bridge }: { bridge: Bridge }) {
  const [cells, setCells] = useState<Cell[] | null>(null);
  /** How many runs each cell has in flight, keyed by cell id. */
  const [busy, setBusy] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);

  // A definition list that never changes cannot answer the question an operator
  // has while watching: **is anything happening in this one?** Runs and Runners
  // both know, and neither is grouped by cell - so a cell with three workers
  // running looked exactly like a cell nobody has ever instructed.
  const count = useCallback(
    () =>
      bridge
        .read<Run[]>("/runs?limit=100")
        .then((runs) => {
          const live: Record<string, number> = {};
          for (const run of runs) {
            if (run.lifecycle === "running") live[run.cell_id] = (live[run.cell_id] ?? 0) + 1;
          }
          setBusy(live);
        })
        .catch(() => setBusy({})),
    [bridge],
  );

  useEffect(() => {
    let live = true;
    bridge
      .read<Cell[]>("/cells")
      .then((body) => live && setCells(body))
      .catch((e: Error) => live && setError(e.message));
    void count();
    // The stream is the trigger rather than a timer, the same choice the Runs
    // widget makes: a run changes state because something happened.
    const subscription = follow(() => void count());
    return () => {
      live = false;
      subscription.close();
    };
  }, [bridge, count]);

  if (error) return <p className="empty">cells unavailable - {error}</p>;
  if (!cells) return <p className="empty">reading cells...</p>;
  if (cells.length === 0) return <p className="empty">No cell definitions loaded.</p>;

  return (
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
        </li>
      ))}
    </ul>
  );
}
