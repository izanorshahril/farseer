import { useEffect, useRef, useState } from "react";
import type { Bridge } from "./bridge";

/**
 * `28 operator surface` gate 3: a sandboxed render with a host bridge.
 *
 * The widget runs in an iframe with `sandbox="allow-scripts"` and no
 * `allow-same-origin`, which gives it an **opaque origin**. That is the whole
 * mechanism, and it is worth being precise about what it buys:
 *
 * - It cannot read the host page, its DOM, or its variables.
 * - It has no cookies, no `localStorage`, and no `sessionStorage` to read.
 * - It cannot `fetch` farseer, or anything else - an opaque origin fails CORS
 *   everywhere, so the network is simply not available to it.
 *
 * So the widget's only channel is a `MessagePort` handed to it once, and the
 * host is the only thing holding `/v1` access. `01 cell primitive` recorded
 * Docker Desktop's **CVE-2025-9074** as proof that "it is only local" is not a
 * boundary; a widget the operator did not write is exactly that case.
 *
* The bundle rides in an inert element and the frame imports it from a blob it
* makes itself, because an opaque origin cannot fetch a URL at all.
 *
 * **The anchor is stamped by the host, never supplied by the widget.** A widget
 * can ask the top manager for something; it cannot claim to be a different
 * widget while doing it.
 */
type Props = {
  id: string;
  title: string;
  bridge: Bridge;
  /** What this widget fronts, if it declared one. `28`: a widget displays a cell. */
  cell?: string;
};

type Call = { id: number; method: "read" | "ask" | "loadState" | "saveState"; args: unknown[] };

const RUNTIME = String.raw`
  const pending = new Map();
  let nextId = 1;
  let port;
  let lastHeight = 0;
  const call = (method, ...args) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      port.postMessage({ id, method, args });
    });
  globalThis.farseer = {
    read: (path) => call("read", path),
    ask: (text) => call("ask", text),
    loadState: (key) => call("loadState", key),
    saveState: (key, value) => call("saveState", key, value),
  };
  // Only report a height that actually changed. Reporting every time resizes
  // the frame, which resizes the document, which reports again - the loop that
  // took the dev server's heap with it the first time this ran.
  const report = () => {
    const height = Math.ceil(document.documentElement.getBoundingClientRect().height);
    if (height === lastHeight) return;
    lastHeight = height;
    port.postMessage({ height });
  };
  const fail = (message) => {
    document.body.innerHTML =
      '<p style="color:#f85149;font:12px system-ui;margin:0"></p>';
    document.body.firstChild.textContent = message;
    report();
  };
  addEventListener("message", (event) => {
    if (!event.ports.length || port) return;
    port = event.ports[0];
    port.onmessage = ({ data }) => {
      const entry = pending.get(data.id);
      if (!entry) return;
      pending.delete(data.id);
      data.error ? entry.reject(new Error(data.error)) : entry.resolve(data.value);
    };
    // The source rides in an inert element rather than a data: URL: base64 of a
    // megabyte is a megabyte of memory for nothing, and the frame's opaque
    // origin cannot fetch a URL anyway. A blob it makes itself, it may import.
    const source = document.getElementById("widget-source").textContent;
    const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    import(/* @vite-ignore */ url)
      .then(() => {
        new ResizeObserver(report).observe(document.documentElement);
        report();
      })
      .catch((error) => fail(error.message))
      .finally(() => URL.revokeObjectURL(url));
  });
`;

function frameHtml(code: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
    :root { color-scheme: dark; }
    body { margin: 0; background: transparent; color: #e6edf3;
           font: 13px/1.5 system-ui, "Segoe UI", sans-serif; }
    a { color: #58a6ff; }
  </style></head><body><div id="root"></div>
  <script id="widget-source" type="text/plain">${code.replace(/<\/script/gi, "<\\/script")}</script>
  <script type="module">${RUNTIME}</script></body></html>`;
}

export function SandboxWidget({ id, title, bridge, cell }: Props) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [html, setHtml] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [height, setHeight] = useState(120);

  useEffect(() => {
    let live = true;
    fetch(`/__widgets/${id}/bundle`)
      .then(async (response) => {
        const body = await response.text();
        if (!response.ok) throw new Error(JSON.parse(body).error ?? body);
        return body;
      })
      .then((code) => live && setHtml(frameHtml(code)))
      .catch((e: Error) => live && setError(e.message));
    return () => {
      live = false;
    };
  }, [id]);

  useEffect(() => {
    const element = frame.current;
    if (!element || !html) return;
    const channel = new MessageChannel();
    channel.port1.onmessage = async ({ data }) => {
      if (typeof data.height === "number") {
        setHeight(Math.max(60, Math.min(600, data.height)));
        return;
      }
      const { id: callId, method, args } = data as Call;
      try {
        const value = await serve(bridge, { widget: title, subject: cell }, method, args);
        channel.port1.postMessage({ id: callId, value });
      } catch (e) {
        channel.port1.postMessage({ id: callId, error: (e as Error).message });
      }
    };
    let handed = false;
    const send = () => {
      if (handed) return;
      handed = true;
      element.contentWindow?.postMessage("farseer-bridge", "*", [channel.port2]);
    };
    // A `srcDoc` frame can finish loading before this effect runs, and a `load`
    // listener attached after the fact never fires - the widget would then sit
    // there forever with no port and no way to say so.
    if (element.contentDocument?.readyState === "complete") send();
    element.addEventListener("load", send);
    return () => {
      element.removeEventListener("load", send);
      channel.port1.close();
    };
  }, [html, bridge, title, cell]);

  if (error) return <p className="empty bad">this widget did not compile - {error}</p>;
  if (!html) return <p className="empty">compiling...</p>;

  return (
    <iframe
      ref={frame}
      className="sandbox"
      title={title}
      // No `allow-same-origin`. Adding it would hand the widget the host's
      // origin and undo every property this gate exists for.
      sandbox="allow-scripts"
      srcDoc={html}
      style={{ height }}
    />
  );
}

/**
 * The host half of the bridge.
 *
 * A widget names a method and arguments; the host decides what that means. The
 * narrowing here is the point: `read` is GET-only, `ask` reaches the top manager
 * with a host-stamped anchor, and state is namespaced per widget so one widget
 * cannot overwrite another's slice - or the canvas layout itself.
 */
async function serve(
  bridge: Bridge,
  anchor: { widget: string; subject?: string },
  method: Call["method"],
  args: unknown[],
): Promise<unknown> {
  const first = typeof args[0] === "string" ? args[0] : "";
  switch (method) {
    case "read": {
      if (!first.startsWith("/")) throw new Error("read takes a /v1 path");
      return bridge.read(first);
    }
    case "ask":
      return bridge.ask(anchor, first);
    case "loadState":
      return bridge.loadState(`widget.${anchor.widget}.${first}`);
    case "saveState":
      return bridge.saveState(`widget.${anchor.widget}.${first}`, args[1]);
  }
}
