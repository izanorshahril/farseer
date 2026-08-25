import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";

/**
 * Which harness stands in front of farseer.
 *
 * `01 cell primitive` made cell zero the operator's address, and its manager is
 * whatever runner the definition names. Changing that is the one setting worth
 * having before any other.
 *
 * **This does not go through `/v1`.** `16 local api surface` gave the API read,
 * validate and reload for definitions and **no edit path**, because a definition
 * is a file in git rather than a row. The shell writes the file and asks the
 * runtime to reload it, so the change leaves a diff the operator can read, revert
 * or commit - exactly what `22 cell addressing` relied on when it refused an
 * in-conversation override.
 *
 * Which means this widget is **absent under `bun run dev`**: the endpoints live
 * in the desktop shell. Saying so beats a button that fails.
 */
type Runner = {
  name: string;
  installed: boolean;
  path: string | null;
  note: string;
};

type TopManager = { cell_id: string; runner: string; file: string };

export function SettingsWidget({ bridge: _bridge }: { bridge: Bridge }) {
  const [runners, setRunners] = useState<Runner[] | null>(null);
  const [current, setCurrent] = useState<TopManager | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [available, setAvailable] = useState(true);

  const load = useCallback(async () => {
    try {
      const [list, top] = await Promise.all([
        fetch("/__settings/runners").then((r) => r.json() as Promise<Runner[]>),
        fetch("/__settings/top-manager").then((r) => r.json() as Promise<TopManager>),
      ]);
      setRunners(list);
      setCurrent(top);
    } catch {
      setAvailable(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const choose = async (runner: string) => {
    setBusy(true);
    setNote(null);
    try {
      const response = await fetch("/__settings/top-manager", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ runner }),
      });
      const body = (await response.json()) as {
        top_manager?: TopManager;
        reload_status?: number;
        error?: string;
      };
      if (!response.ok) throw new Error(typeof body === "string" ? body : response.statusText);
      setCurrent(body.top_manager ?? null);
      setNote(
        body.reload_status === 200
          ? `${runner} is the top manager now, and the definition changed on disk`
          : `written, but farseer answered ${body.reload_status} to reload`,
      );
    } catch (error) {
      setNote((error as Error).message);
    } finally {
      setBusy(false);
    }
  };

  if (!available)
    return (
      <p className="empty">
        Settings live in the desktop shell, which owns the filesystem. Under <code>bun run dev</code>{" "}
        there is nothing here to write with.
      </p>
    );
  if (!runners || !current) return <p className="empty">reading the definition...</p>;

  return (
    <>
      <p className="dim small" style={{ margin: "0 0 10px" }}>
        The harness in front of <b>{current.cell_id}</b>. Every request you type goes to it, and it
        decides where the work goes.
      </p>
      <ul className="runners">
        {runners.map((runner) => {
          const chosen = runner.name === current.runner;
          return (
            <li key={runner.name} className={chosen ? "chosen" : ""}>
              <span className={`dot ${runner.installed ? "live" : ""}`} />
              <span className="grow">
                <b className="mono">{runner.name}</b>
                <div className="dim small">{runner.note}</div>
                {!runner.installed && (
                  <div className="faint small">not on PATH here - farseer would fail at spawn</div>
                )}
              </span>
              {chosen ? (
                <span className="badge agent">top manager</span>
              ) : (
                <button
                  className="chip"
                  disabled={busy || !runner.installed}
                  onClick={() => void choose(runner.name)}
                >
                  use
                </button>
              )}
            </li>
          );
        })}
      </ul>
      <p className="dim small" style={{ marginBottom: 0 }}>
        {note ?? (
          <>
            Written to <span className="mono">{current.file}</span> and reloaded. A change here is a
            git diff, not a hidden setting.
          </>
        )}
      </p>
    </>
  );
}
