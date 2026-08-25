import { useCallback, useEffect, useState } from "react";
import { createBridge, type Anchor } from "./bridge";
import { QuotaWidget } from "./widgets/quota";
import { FleetWidget } from "./widgets/fleet";

/**
 * The canvas.
 *
 * `28 operator surface`: the canvas is the home screen, and if it is not the
 * canvas it is a widget on it. There is no second layout and no mode switch.
 *
 * The arrangement is stored through `PUT /v1/ui-state/canvas`, which farseer
 * holds as an **opaque blob it never parses** - `24 ui state persistence` chose
 * that over `localStorage` so the command center comes back the way it was left
 * even in a different window, and so a backup of farseer's data directory backs
 * up the layout for free.
 */
const bridge = createBridge();

/**
 * The widgets this build knows how to render.
 *
 * Agent-authored widgets from `widgets/` in git mount here later, through
 * `28`'s three gates. Today the registry is static, which is the honest state:
 * the host contract is proven by two widgets before it is asked to compile a
 * third one written by a manager.
 */
const REGISTRY = {
  quota: { title: "Windows", subtitle: "by account", render: QuotaWidget },
  fleet: { title: "Cells", subtitle: "loaded definitions", render: FleetWidget },
} as const;

type WidgetId = keyof typeof REGISTRY;

/** The blob. Farseer never reads this shape; only the canvas does. */
type Layout = { mounted: WidgetId[]; wide: WidgetId[] };

const DEFAULT_LAYOUT: Layout = { mounted: ["quota", "fleet"], wide: ["quota"] };

export function App() {
  const [layout, setLayout] = useState<Layout | null>(null);
  const [anchor, setAnchor] = useState<Anchor>({ widget: "canvas" });
  const [asking, setAsking] = useState(false);
  const [lastRun, setLastRun] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    bridge
      .loadState<Layout>("canvas")
      .then((stored) => setLayout(stored ?? DEFAULT_LAYOUT))
      .catch(() => setLayout(DEFAULT_LAYOUT));
  }, []);

  const persist = useCallback((next: Layout) => {
    setLayout(next);
    bridge.saveState("canvas", next).catch((e: Error) => setError(e.message));
  }, []);

  const ask = useCallback(
    async (text: string) => {
      setAsking(true);
      setError(null);
      try {
        setLastRun(await bridge.ask(anchor, text));
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setAsking(false);
      }
    },
    [anchor],
  );

  if (!layout) return <main className="loading">loading the canvas...</main>;

  return (
    <div className="app">
      <header>
        <span className="brand">
          far<span>seer</span>
        </span>
        <span className="dim small">
          canvas - arrangement saved to farseer, not to this browser
        </span>
        <span className="grow" />
        {REGISTRY &&
          (Object.keys(REGISTRY) as WidgetId[]).map((id) => (
            <button
              key={id}
              className={layout.mounted.includes(id) ? "chip on" : "chip"}
              onClick={() =>
                persist({
                  ...layout,
                  mounted: layout.mounted.includes(id)
                    ? layout.mounted.filter((m) => m !== id)
                    : [...layout.mounted, id],
                })
              }
            >
              {REGISTRY[id].title}
            </button>
          ))}
      </header>

      <main className="canvas">
        {layout.mounted.map((id) => {
          const widget = REGISTRY[id];
          const Render = widget.render;
          const wide = layout.wide.includes(id);
          return (
            <section
              key={id}
              className={wide ? "widget wide" : "widget"}
              onFocus={() => setAnchor({ widget: widget.title })}
              onMouseEnter={() => setAnchor({ widget: widget.title })}
            >
              <div className="head">
                <span className="grip" aria-hidden>
                  ⠿
                </span>
                <b>{widget.title}</b>
                <span className="dim small">{widget.subtitle}</span>
                <span className="grow" />
                <button
                  className="chip"
                  title={wide ? "make it narrow" : "make it wide"}
                  onClick={() =>
                    persist({
                      ...layout,
                      wide: wide ? layout.wide.filter((w) => w !== id) : [...layout.wide, id],
                    })
                  }
                >
                  {wide ? "narrow" : "wide"}
                </button>
              </div>
              <div className="body">
                <Render bridge={bridge} />
              </div>
            </section>
          );
        })}
        {layout.mounted.length === 0 && (
          <p className="empty">Nothing mounted. Add a widget from the header.</p>
        )}
      </main>

      <footer>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            const field = new FormData(event.currentTarget).get("ask");
            const text = typeof field === "string" ? field.trim() : "";
            if (text) {
              void ask(text);
              event.currentTarget.reset();
            }
          }}
        >
          <span className="badge" title="every request goes to the top manager, whatever you are looking at">
            to: top manager
          </span>
          <input
            name="ask"
            autoComplete="off"
            disabled={asking}
            placeholder={`Ask for anything - anchored to ${anchor.widget}`}
          />
          <button className="chip on" disabled={asking}>
            {asking ? "sending" : "send"}
          </button>
        </form>
        <p className="dim small">
          {error ? (
            <span className="bad">{error}</span>
          ) : lastRun ? (
            <>
              accepted as run <span className="mono">{lastRun.slice(0, 8)}</span> - the answer
              arrives on the event stream, not here
            </>
          ) : (
            <>
              The widget you type from is the anchor, never the destination. The top manager
              decides where the work goes.
            </>
          )}
        </p>
      </footer>
    </div>
  );
}
