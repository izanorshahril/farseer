import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

/**
 * An agent-authored widget, written by hand first so the contract is proven
 * before a manager is asked to write one.
 *
 * It runs in an opaque-origin frame: no token, no storage, no network. The only
 * thing it has is `farseer`, handed to it by the host.
 */
declare const farseer: {
  read: <T>(path: string) => Promise<T>;
  ask: (text: string) => Promise<string>;
  loadState: <T>(key: string) => Promise<T | null>;
  saveState: (key: string, value: unknown) => Promise<void>;
};

type Row = { cell_id: string; runs: number; touched: number };

function RunTally() {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    farseer
      .read<Row[]>("/analytics/intervention")
      .then(setRows)
      .catch((e: Error) => setNote(e.message));
  }, []);

  if (note) return <p style={{ color: "#f85149", margin: 0 }}>{note}</p>;
  if (!rows) return <p style={{ color: "#5c6673", margin: 0 }}>reading the record...</p>;
  if (rows.length === 0)
    return <p style={{ color: "#5c6673", margin: 0 }}>No runs recorded yet.</p>;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {rows.map((row) => (
        <div key={row.cell_id} style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
          <b style={{ fontFamily: "ui-monospace, Consolas, monospace" }}>{row.cell_id}</b>
          <span style={{ flex: 1 }} />
          <span style={{ fontSize: 18, fontWeight: 600 }}>{row.runs}</span>
          <span style={{ color: "#8b97a6", fontSize: 11 }}>
            runs, {row.touched} operator-touched
          </span>
        </div>
      ))}
      <button
        onClick={() => {
          // A widget may ask, and the top manager decides what to do about it.
          // It cannot address a cell - there is no such call on the bridge.
          void farseer
            .ask("Summarise which cell needed the most operator intervention this week, and why.")
            .then((run) => setNote(`asked - run ${run.slice(0, 8)}`));
        }}
        style={{
          alignSelf: "flex-start",
          padding: "5px 10px",
          borderRadius: 5,
          border: "1px solid #232b35",
          background: "#171c23",
          color: "#e6edf3",
          font: "inherit",
          fontSize: 11,
          cursor: "pointer",
        }}
      >
        ask about this
      </button>
    </div>
  );
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<RunTally />);
