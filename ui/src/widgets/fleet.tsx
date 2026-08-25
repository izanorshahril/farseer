import { useEffect, useState } from "react";
import type { Bridge } from "../bridge";

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

export function FleetWidget({ bridge }: { bridge: Bridge }) {
  const [cells, setCells] = useState<Cell[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    bridge
      .read<Cell[]>("/cells")
      .then((body) => live && setCells(body))
      .catch((e: Error) => live && setError(e.message));
    return () => {
      live = false;
    };
  }, [bridge]);

  if (error) return <p className="empty">cells unavailable - {error}</p>;
  if (!cells) return <p className="empty">reading cells...</p>;
  if (cells.length === 0) return <p className="empty">No cell definitions loaded.</p>;

  return (
    <ul className="cells">
      {cells.map((cell) => (
        <li key={cell.cell_id}>
          <div className="row">
            <b>{cell.name}</b>
            <span className="grow" />
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
