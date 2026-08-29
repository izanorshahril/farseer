import { useCallback, useEffect, useState } from "react";
import { createBridge, type Anchor } from "./bridge";
import { QuotaWidget } from "./widgets/quota";
import { FleetWidget } from "./widgets/fleet";
import { ActivityWidget } from "./widgets/activity";
import { RunsWidget } from "./widgets/runs";
import { RunnersWidget } from "./widgets/runners";
import { SettingsWidget } from "./widgets/settings";
import { ConversationWidget } from "./widgets/conversation";
import { DelegationWidget } from "./widgets/delegation";
import { SandboxWidget } from "./SandboxWidget";
import { GateBar } from "./GateBar";

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
  conversation: {
    title: "Conversation",
    subtitle: "you and the top manager",
    render: ConversationWidget,
  },
  delegation: {
    title: "Delegation",
    subtitle: "the top manager and its workers",
    render: DelegationWidget,
  },
  quota: { title: "Windows", subtitle: "by account", render: QuotaWidget },
  fleet: { title: "Cells", subtitle: "loaded definitions", render: FleetWidget },
  activity: { title: "Activity", subtitle: "the record, live", render: ActivityWidget },
  runners: {
    title: "Runners",
    subtitle: "spawned right now",
    render: RunnersWidget,
  },
  runs: { title: "Runs", subtitle: "with 05's verbs", render: RunsWidget },
  settings: { title: "Settings", subtitle: "the harness in front", render: SettingsWidget },
} as const;

type WidgetId = string;

/**
 * A widget cell zero wrote, discovered from `widgets/` rather than compiled into
 * this build. `28 operator surface`'s three gates apply to exactly these: the
 * import allowlist at compile, the sandboxed render, and keep-or-undo per turn.
 */
type AgentWidget = { id: string; title: string; subtitle: string; cell?: string };

/**
 * The blob. Farseer never reads this shape; only the canvas does.
 *
 * `v` is the canvas's own, and a stored blob from an older one is **replaced
 * rather than migrated**. `24 ui state persistence` made this an arrangement,
 * not data: nothing is lost by starting again from a better default, and a
 * migration for a value the operator can restore by dragging two cards is
 * machinery that has to be right forever to save one gesture once.
 */
type Layout = { v?: number; mounted: WidgetId[]; wide: WidgetId[] };

const LAYOUT_VERSION = 2;

/**
 * The two conversations first, side by side, and everything else below them.
 *
 * Reading order is the argument: the operator's own exchange with the top
 * manager is what they came to the screen for, and what the manager did about it
 * is the next question rather than a separate one. Both are `wide`, so on any
 * screen wide enough for four columns they occupy the top row together.
 */
const DEFAULT_LAYOUT: Layout = {
  v: LAYOUT_VERSION,
  mounted: ["conversation", "delegation", "runs", "activity", "quota", "runners", "fleet"],
  wide: ["conversation", "delegation", "runs"],
};

export function App() {
  const [layout, setLayout] = useState<Layout | null>(null);
  const [agentWidgets, setAgentWidgets] = useState<AgentWidget[]>([]);
  const [anchor, setAnchor] = useState<Anchor>({ widget: "canvas" });
  const [asking, setAsking] = useState(false);
  const [lastRun, setLastRun] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Which widget is being dragged, and which one it is currently over.
  // `24 ui state persistence` already stores the order; the grip has looked
  // like a handle since the first canvas and never moved anything.
  //
  // **The grip is the draggable element**, and the first attempt at this made
  // the whole card `draggable={dragging === id}` on the grip's `mousedown`.
  // That cannot work: the browser decides whether a gesture is a drag at
  // mousedown, and React sets the attribute a render later - by which time the
  // gesture has already been rejected as a text selection. The card stays a
  // plain drop target, which is the half that was right.
  const [dragging, setDragging] = useState<WidgetId | null>(null);
  const [over, setOver] = useState<WidgetId | null>(null);

  useEffect(() => {
    bridge
      .loadState<Layout>("canvas")
      .then((stored) => setLayout(stored?.v === LAYOUT_VERSION ? stored : DEFAULT_LAYOUT))
      .catch(() => setLayout(DEFAULT_LAYOUT));
    // Discovered, not imported: a widget appears because a file exists, which
    // is what makes "ask for a widget" a thing the operator can do.
    fetch("/__widgets")
      .then((response) => response.json() as Promise<AgentWidget[]>)
      .then(setAgentWidgets)
      .catch(() => setAgentWidgets([]));
  }, []);

  /**
   * Layout edits compose: two toggles in the same tick must not race, and a
   * closure over the last render's `layout` is exactly how one silently loses.
   */
  const persist = useCallback((change: (current: Layout) => Layout) => {
    setLayout((current) => {
      if (!current) return current;
      const next = change(current);
      bridge.saveState("canvas", next).catch((e: Error) => setError(e.message));
      return next;
    });
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

  const built = Object.entries(REGISTRY).map(([id, widget]) => ({
    id,
    title: widget.title,
    subtitle: widget.subtitle,
    agent: false as const,
  }));
  const authored = agentWidgets.map((widget) => ({ ...widget, agent: true as const }));
  const available = [...built, ...authored];

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
        {available.map(({ id, title }) => (
          <button
            key={id}
            className={layout.mounted.includes(id) ? "chip on" : "chip"}
            onClick={() =>
              persist((current) => ({
                ...current,
                mounted: current.mounted.includes(id)
                  ? current.mounted.filter((m) => m !== id)
                  : [...current.mounted, id],
              }))
            }
          >
            {title}
          </button>
        ))}
      </header>

      <GateBar />

      <main className="canvas">
        {layout.mounted.map((id) => {
          const widget = available.find((candidate) => candidate.id === id);
          if (!widget) return null;
          const wide = layout.wide.includes(id);
          return (
            <section
              key={id}
              className={[
                wide ? "widget wide" : "widget",
                dragging === id ? "dragging" : "",
                over === id && dragging !== id ? "drop-target" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              onFocus={() => setAnchor({ widget: widget.title })}
              onMouseEnter={() => setAnchor({ widget: widget.title })}
              // Drop lands on the whole widget rather than the grip, because a
              // 12px target is a target nobody hits.
              onDragOver={(event) => {
                if (!dragging) return;
                event.preventDefault();
                setOver(id);
              }}
              onDragLeave={() => setOver((current) => (current === id ? null : current))}
              onDrop={(event) => {
                event.preventDefault();
                setOver(null);
                // Read from the transfer rather than from state: a drop is
                // handled on a different element than the one that set the
                // state, and Firefox clears `dragging` before it fires.
                const from = event.dataTransfer.getData("text/farseer-widget") || dragging;
                setDragging(null);
                if (!from || from === id) return;
                // Move rather than swap: an operator dragging one card past
                // another expects the rest to close up behind it.
                persist((current) => {
                  const order = current.mounted.filter((w) => w !== from);
                  const at = order.indexOf(id);
                  order.splice(at < 0 ? order.length : at, 0, from);
                  return { ...current, mounted: order };
                });
              }}
            >
              <div className="head">
                <span
                  className="grip"
                  role="button"
                  aria-label={`move the ${widget.title} widget`}
                  title="drag to move this widget"
                  // Only the grip drags, so selecting text in a widget body is
                  // still a selection.
                  draggable
                  onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = "move";
                    event.dataTransfer.setData("text/farseer-widget", id);
                    setDragging(id);
                  }}
                  onDragEnd={() => {
                    setDragging(null);
                    setOver(null);
                  }}
                >
                  ⠿
                </span>
                <b>{widget.title}</b>
                <span className="dim small">{widget.subtitle}</span>
                {widget.agent && (
                  <span className="badge agent" title="written into widgets/ and compiled here, sandboxed">
                    authored
                  </span>
                )}
                <span className="grow" />
                <button
                  className="chip"
                  title={wide ? "make it narrow" : "make it wide"}
                  onClick={() =>
                    persist((current) => ({
                      ...current,
                      wide: current.wide.includes(id)
                        ? current.wide.filter((w) => w !== id)
                        : [...current.wide, id],
                    }))
                  }
                >
                  {wide ? "narrow" : "wide"}
                </button>
              </div>
              <div className="body">
                {widget.agent ? (
                  <SandboxWidget
                    id={widget.id}
                    title={widget.title}
                    bridge={bridge}
                    {...("cell" in widget && widget.cell ? { cell: widget.cell } : {})}
                  />
                ) : (
                  (() => {
                    const Render = REGISTRY[widget.id as keyof typeof REGISTRY].render;
                    return <Render bridge={bridge} />;
                  })()
                )}
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
          {/* The anchor is the whole reason this box exists beside the one in
              the Conversation widget. Both go to the top manager - `28 operator
              surface`: the widget you type from is the anchor, never the
              destination - so with the anchor invisible they were two identical
              boxes, and a duplicate control with no visible difference reads as
              a bug. Naming what it is anchored to is the difference. */}
          <span
            className="badge"
            title="every request goes to the top manager, whatever you are looking at"
          >
            to: top manager
          </span>
          <span className="badge accent" title="what the top manager is told you were looking at">
            about: {anchor.widget}
          </span>
          <input
            name="ask"
            autoComplete="off"
            disabled={asking}
            placeholder={`Ask about ${anchor.widget} - hover another widget to ask about that instead`}
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
              decides where the work goes - to say something unanchored, use the box inside the
              Conversation widget.
            </>
          )}
        </p>
      </footer>
    </div>
  );
}
