import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

declare const farseer: {
  read: <T>(path: string) => Promise<T>;
  ask: (text: string) => Promise<string>;
  loadState: <T>(key: string) => Promise<T | null>;
  saveState: (key: string, value: unknown) => Promise<void>;
};

/**
 * A widget that tries the things a widget should not be able to do.
 *
 * `28 operator surface`'s three gates were built and then never driven from the
 * inside: two widgets that read one endpoint each is not an exercise of a
 * sandbox. This one asserts the boundary rather than describing it, so a change
 * that opens the frame up shows as a red line on the canvas instead of as
 * nothing at all.
 */
type Check = { what: string; want: "allowed" | "denied"; got: string; ok: boolean };

async function attempt(
  what: string,
  want: Check["want"],
  run: () => Promise<unknown>,
): Promise<Check> {
  try {
    const value = await run();
    const got = `ok - ${JSON.stringify(value).slice(0, 40)}`;
    return { what, want, got, ok: want === "allowed" };
  } catch (error) {
    return { what, want, got: (error as Error).message.slice(0, 60), ok: want === "denied" };
  }
}

function Probe() {
  const [checks, setChecks] = useState<Check[] | null>(null);

  useEffect(() => {
    void (async () => {
      const results: Check[] = [];

      // The host's own page. An opaque origin has no access to its parent, so
      // this is a TypeError rather than a policy decision - which is the point:
      // the boundary is the browser's, not farseer's.
      results.push(
        await attempt("reach the host page", "denied", async () => {
          return (window.parent as unknown as { location: { href: string } }).location.href;
        }),
      );

      // No cookies, no storage: an opaque origin has none to read.
      results.push(
        await attempt("read localStorage", "denied", async () => localStorage.length),
      );

      // The network, directly. An opaque origin fails CORS everywhere, so the
      // only channel is the port the host handed over.
      results.push(
        await attempt("fetch /v1/runs directly", "denied", async () => {
          const response = await fetch("/v1/runs?limit=1");
          return response.status;
        }),
      );

      // Through the bridge, which is the sanctioned channel.
      results.push(
        await attempt("farseer.read('/runs?limit=1')", "allowed", () =>
          farseer.read("/runs?limit=1"),
        ),
      );

      // The canvas arrangement, and every other widget's private state with it.
      // `saveState` was namespaced per widget and `read` was not, so this went
      // straight past the namespacing until it was closed.
      results.push(
        await attempt("farseer.read('/ui-state/canvas')", "denied", () =>
          farseer.read("/ui-state/canvas"),
        ),
      );

      // Its own slice, which it may keep.
      results.push(
        await attempt("saveState then loadState", "allowed", async () => {
          await farseer.saveState("probe", { seen: true });
          return farseer.loadState("probe");
        }),
      );

      // Reaching another widget's slice by naming it. The host prefixes every
      // key with this widget's id, so the worst a widget can do is make itself
      // a strangely-named key of its own.
      results.push(
        await attempt("loadState('../cost-today.x')", "allowed", () =>
          farseer.loadState("../cost-today.x"),
        ),
      );

      setChecks(results);
      // Out through the one channel a widget has, so the verdict is checkable
      // from outside a frame that is deliberately unreadable from outside.
      // Saving it is itself the last check.
      await farseer.saveState("results", results);
    })();
  }, []);

  if (!checks) return <p style={{ margin: 0, opacity: 0.6 }}>probing...</p>;

  return (
    <ul style={{ listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 4 }}>
      {checks.map((check) => (
        <li key={check.what} style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
          <span style={{ color: check.ok ? "#3fb950" : "#f85149" }}>{check.ok ? "OK" : "!!"}</span>
          <span style={{ flex: 1 }}>{check.what}</span>
          <span style={{ opacity: 0.55, fontSize: 11 }}>
            {check.want} - {check.got}
          </span>
        </li>
      ))}
    </ul>
  );
}

createRoot(document.getElementById("root")!).render(<Probe />);
