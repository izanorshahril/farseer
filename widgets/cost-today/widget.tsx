import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

declare const farseer: {
  read: <T>(path: string) => Promise<T>;
  ask: (text: string) => Promise<string>;
  loadState: <T>(key: string) => Promise<T | null>;
  saveState: (key: string, value: unknown) => Promise<void>;
};

type Cost = { total_usd?: number; total?: number; usd?: number };

function Widget() {
  const [total, setTotal] = useState<number | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    farseer
      .read<Cost>("/analytics/cost")
      .then((c) => setTotal(c.total_usd ?? c.total ?? c.usd ?? 0))
      .catch(() => setError(true));
  }, []);

  return (
    <div
      style={{
        background: "#0d1117",
        color: "#e6edf3",
        font: "13px system-ui",
        padding: 16,
      }}
    >
      <div style={{ color: "#8b97a6" }}>total farseer spend</div>
      <div style={{ color: "#58a6ff", fontSize: 32, marginTop: 8 }}>
        {error ? "-" : total === null ? "..." : `$${total.toFixed(2)}`}
      </div>
    </div>
  );
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<Widget />);
