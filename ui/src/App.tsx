import { useCallback, useEffect, useRef, useState } from "react";
import { createBridge, type Anchor } from "./bridge";
import {
  DEFAULT_WIDGET_SPAN,
  DEFAULT_WIDGET_UNIT,
  MAX_UNIT_HEIGHT,
  MAX_UNIT_WIDTH,
  MIN_UNIT_HEIGHT,
  MIN_UNIT_WIDTH,
  WIDGET_SPANS,
  moveBy,
  moveToSlot,
  normalizeLayout,
  normalizeSpan,
  normalizeUnit,
  nudged,
  resized,
  toggleMounted,
  type CanvasLayout,
  type Span,
  type WidgetUnit,
} from "./layout";
import { onSelection } from "./selection";
import { restoreProject } from "./project";
import { QuotaWidget } from "./widgets/quota";
import { ClockWidget } from "./widgets/clock";
import { FleetWidget } from "./widgets/fleet";
import { ActivityWidget } from "./widgets/activity";
import { RunsWidget } from "./widgets/runs";
import { RunnersWidget } from "./widgets/runners";
import { SettingsWidget } from "./widgets/settings";
import { ProjectsWidget } from "./widgets/projects";
import { ConversationWidget } from "./widgets/conversation";
import { WorkWidget } from "./widgets/work";
import { DelegationWidget } from "./widgets/delegation";
import { RunWidget } from "./widgets/run";
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
 * Agent-authored widgets from `widgets/` join this built registry through
 * `28 operator surface`'s import, sandbox, and keep-or-undo gates.
 */
const REGISTRY = {
  conversation: {
    title: "Conversation",
    subtitle: "you and the top manager",
    render: ConversationWidget,
  },
  work: {
    title: "Work",
    subtitle: "board, conversations and graph",
    render: WorkWidget,
  },
  fleet: { title: "Fleet", subtitle: "cell definitions", render: FleetWidget },
  capacity: { title: "Capacity", subtitle: "provider accounts", render: QuotaWidget },
  clock: { title: "Clock", subtitle: "local time", render: ClockWidget },
  delegation: {
    title: "Delegation",
    subtitle: "manager and workers",
    render: DelegationWidget,
  },
  activity: { title: "Activity", subtitle: "record, live", render: ActivityWidget },
  runners: {
    title: "Runners",
    subtitle: "active processes",
    render: RunnersWidget,
  },
  runs: { title: "Runs", subtitle: "recent work", render: RunsWidget },
  run: { title: "Run", subtitle: "selected run", render: RunWidget },
  projects: {
    title: "Projects",
    subtitle: "authorized folders",
    render: ProjectsWidget,
  },
} as const;

type WidgetId = string;

/**
 * A widget cell zero wrote, discovered from `widgets/` rather than compiled into
 * this build. `28 operator surface`'s three gates apply to exactly these: the
 * import allowlist at compile, the sandboxed render, and keep-or-undo per turn.
 */
type AgentWidget = { id: string; title: string; subtitle: string; cell?: string };


const LAYOUT_VERSION = 8;

/**
 * `40 work model and session explorer` makes the fresh-install home four
 * operational faces. Detailed and optional widgets remain available.
 */
const DEFAULT_LAYOUT: CanvasLayout = {
  v: LAYOUT_VERSION,
  mounted: ["conversation", "work", "fleet", "capacity"],
  span: {
    conversation: DEFAULT_WIDGET_SPAN,
    work: DEFAULT_WIDGET_SPAN,
    fleet: DEFAULT_WIDGET_SPAN,
    capacity: DEFAULT_WIDGET_SPAN,
  },
  unit: DEFAULT_WIDGET_UNIT,
};

/**
 * One widget's mount toggle.
 *
 * Lifted out of the header's map so the two groups - what farseer ships, what a
 * cell wrote - render the same control rather than two copies of it that can
 * drift apart.
 */
function WidgetChip({
  id,
  title,
  subtitle,
  layout,
  persist,
}: {
  id: WidgetId;
  title: string;
  subtitle: string;
  layout: CanvasLayout;
  persist: (change: (current: CanvasLayout) => CanvasLayout) => void;
}) {
  const mounted = layout.mounted.includes(id);
  return (
    <button
      className={mounted ? "widget-toggle active" : "widget-toggle"}
      aria-pressed={mounted}
      title={mounted ? `Hide ${title}` : `Show ${title}`}
      onClick={() => persist((current) => toggleMounted(current, id))}
    >
      <span className={`widget-avatar widget-avatar-${id}`} aria-hidden>
        {title.slice(0, 1)}
      </span>
      <span className="widget-toggle-copy">
        <b>{title}</b>
        <small>{subtitle}</small>
      </span>
      <span className={mounted ? "mount-state on" : "mount-state"} aria-hidden />
    </button>
  );
}

/** The pixel metric behind every widget's 1x dimension. */
function UnitControls({
  unit,
  persist,
}: {
  unit: WidgetUnit;
  persist: (change: (current: CanvasLayout) => CanvasLayout) => void;
}) {
  const [draft, setDraft] = useState({
    width: String(unit.width),
    height: String(unit.height),
  });

  useEffect(() => {
    setDraft({ width: String(unit.width), height: String(unit.height) });
  }, [unit.width, unit.height]);

  const commit = (part: keyof WidgetUnit) => {
    const value = Number(draft[part]);
    if (!Number.isFinite(value)) {
      setDraft((current) => ({ ...current, [part]: String(unit[part]) }));
      return;
    }
    const next = normalizeUnit({ ...unit, [part]: value });
    setDraft({ width: String(next.width), height: String(next.height) });
    persist((current) =>
      current.unit.width === next.width && current.unit.height === next.height
        ? current
        : { ...current, unit: next },
    );
  };

  return (
    <fieldset className="unit-controls">
      <legend>1x widget metric</legend>
      <label>
        <span>W</span>
        <input
          type="number"
          min={MIN_UNIT_WIDTH}
          max={MAX_UNIT_WIDTH}
          step={1}
          value={draft.width}
          onChange={(event) => {
            const width = event.currentTarget.value;
            setDraft((current) => ({ ...current, width }));
          }}
          onBlur={() => commit("width")}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
        <small>px</small>
      </label>
      <label>
        <span>H</span>
        <input
          type="number"
          min={MIN_UNIT_HEIGHT}
          max={MAX_UNIT_HEIGHT}
          step={1}
          value={draft.height}
          onChange={(event) => {
            const height = event.currentTarget.value;
            setDraft((current) => ({ ...current, height }));
          }}
          onBlur={() => commit("height")}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
        <small>px</small>
      </label>
    </fieldset>
  );
}

/**
 * The corner a card is resized by.
 *
 * `28 operator surface` makes the canvas arrangeable. Pointer capture keeps
 * the resize alive when the pointer leaves the card, using the same WebView2-
 * safe gesture as movement.
 */
function ResizeHandle({
  id,
  title,
  span,
  gridStep,
  preview,
  persist,
}: {
  id: WidgetId;
  title: string;
  span: Span;
  gridStep: () => { x: number; y: number };
  preview: (next: { id: WidgetId; span: Span } | null) => void;
  persist: (change: (current: CanvasLayout) => CanvasLayout) => void;
}) {
  // The gesture's own state. Not React state: it is read on every pointermove
  // and never rendered, and a re-render per pixel is exactly what the preview
  // above exists to avoid.
  const from = useRef<{ x: number; y: number; span: Span } | null>(null);

  const commit = useCallback(
    (next: Span) => {
      preview(null);
      persist((current) => ({ ...current, span: { ...current.span, [id]: next } }));
    },
    [id, preview, persist],
  );

  return (
    <span
      className="resize"
      role="button"
      tabIndex={0}
      aria-label={`resize the ${title} widget - arrow keys, or drag`}
      aria-keyshortcuts="ArrowLeft ArrowRight ArrowUp ArrowDown"
      title={`drag to resize, or focus and press an arrow key - now ${span.w}x${span.h}`}
      onPointerDown={(event) => {
        // Left button only. A right-click that started a resize would leave the
        // gesture running under the context menu.
        if (event.button !== 0) return;
        event.preventDefault();
        from.current = { x: event.clientX, y: event.clientY, span };
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        const start = from.current;
        if (!start) return;
        preview({
          id,
          span: resized(start.span, event.clientX - start.x, event.clientY - start.y, gridStep()),
        });
      }}
      onPointerUp={(event) => {
        const start = from.current;
        from.current = null;
        if (!start) return;
        event.currentTarget.releasePointerCapture(event.pointerId);
        commit(resized(start.span, event.clientX - start.x, event.clientY - start.y, gridStep()));
      }}
      // A cancelled gesture - a browser stealing the pointer, a touch turning
      // into a scroll - puts the card back rather than leaving it mid-drag.
      onPointerCancel={() => {
        from.current = null;
        preview(null);
      }}
      onKeyDown={(event) => {
        const next = nudged(span, event.key);
        if (!next) return;
        event.preventDefault();
        commit(next);
      }}
    >
      <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden focusable="false">
        <path d="M9 1 1 9M9 5 5 9" stroke="currentColor" strokeWidth="1.2" fill="none" />
      </svg>
    </span>
  );
}

export function App() {
  const [layout, setLayout] = useState<CanvasLayout | null>(null);
  const layoutRef = useRef<CanvasLayout | null>(null);
  const saveQueue = useRef(Promise.resolve());
  const [agentWidgets, setAgentWidgets] = useState<AgentWidget[]>([]);
  const [anchor, setAnchor] = useState<Anchor>({ widget: "canvas" });
  const [asking, setAsking] = useState(false);
  const [lastRun, setLastRun] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Pointer capture works in both Chromium and the desktop WebView2. Native
  // HTML drag-and-drop did not: WebView2 emitted dragover but no drop for the
  // same grip gesture that completed in the browser.
  const [dragging, setDragging] = useState<WidgetId | null>(null);
  const [over, setOver] = useState<WidgetId | null>(null);
  const moving = useRef<{ id: WidgetId; pointerId: number } | null>(null);
  // `28 operator surface` makes the rendered grid the source of truth for both
  // pointer axes, including its padding and gutters.
  const board = useRef<HTMLElement | null>(null);
  // `28 operator surface` previews a resize locally and persists only its final
  // snapped span, rather than issuing one PUT per pointer pixel.
  const [preview, setPreview] = useState<{ id: WidgetId; span: Span } | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    id: WidgetId;
    title: string;
    x: number;
    y: number;
  } | null>(null);

  const gridStep = useCallback(() => {
    const element = board.current;
    const unit = layoutRef.current?.unit ?? DEFAULT_WIDGET_UNIT;
    if (!element) return { x: unit.width, y: unit.height };
    const style = getComputedStyle(element);
    return {
      x: unit.width + (Number.parseFloat(style.columnGap) || 0),
      y: unit.height + (Number.parseFloat(style.rowGap) || 0),
    };
  }, []);

  useEffect(() => {
    bridge
      .loadState<unknown>("canvas")
      .then((stored) => {
        const next = normalizeLayout(stored, DEFAULT_LAYOUT);
        layoutRef.current = next;
        setLayout(next);
      })
      .catch(() => {
        layoutRef.current = DEFAULT_LAYOUT;
        setLayout(DEFAULT_LAYOUT);
      });
    // Discovered, not imported: a widget appears because a file exists, which
    // is what makes "ask for a widget" a thing the operator can do.
    // Which project the canvas is pointed at, restored before anything can be
    // sent - `39 what an installed farseer points at` makes every instruction
    // carry one.
    void restoreProject();
    fetch("/__widgets")
      .then((response) => response.json() as Promise<AgentWidget[]>)
      .then(setAgentWidgets)
      .catch(() => setAgentWidgets([]));
  }, []);

  useEffect(() => {
    const closeOverlays = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setSidebarOpen(false);
      setContextMenu(null);
      setSettingsOpen(false);
    };
    const closeContextMenu = (event: PointerEvent) => {
      if (!(event.target instanceof Element) || !event.target.closest(".widget-context-menu")) {
        setContextMenu(null);
      }
    };
    document.addEventListener("keydown", closeOverlays);
    document.addEventListener("pointerdown", closeContextMenu);
    return () => {
      document.removeEventListener("keydown", closeOverlays);
      document.removeEventListener("pointerdown", closeContextMenu);
    };
  }, []);

  /**
   * Compose layout edits synchronously, then serialize their opaque PUTs.
   *
   * `24 ui state persistence` deliberately has no concurrency control. Keeping
   * the current layout in this client and saving in order prevents Strict Mode
   * duplicate effects and an older response from winning a rapid later edit.
   */
  const persist = useCallback((change: (current: CanvasLayout) => CanvasLayout) => {
    const current = layoutRef.current;
    if (!current) return;
    const next = change(current);
    if (next === current) return;
    layoutRef.current = next;
    setLayout(next);
    saveQueue.current = saveQueue.current
      .then(() => bridge.saveState("canvas", next))
      .catch((e: Error) => setError(e.message));
  }, []);

  // Selecting a run has to *show* one. A click that opens a widget the operator
  // has unmounted looks like a click that did nothing, which is the same class
  // of failure as the grip that rendered a handle and moved nothing.
  useEffect(
    () =>
      onSelection((runId) => {
        if (!runId) return;
        persist((current) =>
          current.mounted.includes("run") ? current : toggleMounted(current, "run"),
        );
      }),
    // `persist` is stable, and re-subscribing on every layout change would drop
    // a selection made between renders.
    [persist],
  );

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
  const contextIndex = contextMenu ? layout.mounted.indexOf(contextMenu.id) : -1;

  return (
    <div
      className={[
        "app",
        sidebarOpen ? "sidebar-open" : "",
        sidebarCollapsed ? "sidebar-collapsed" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <button
        className="sidebar-backdrop"
        aria-label="Close widget navigation"
        onClick={() => setSidebarOpen(false)}
      />
      <aside className="sidebar" aria-label="Widget navigation">
        <div className="brand-row">
          <div className="brand">
            <svg viewBox="0 0 28 28" aria-hidden>
              <path d="M4 14c2.8-5 6.1-7.5 10-7.5S21.2 9 24 14c-2.8 5-6.1 7.5-10 7.5S6.8 19 4 14Z" />
              <circle cx="14" cy="14" r="3.2" />
            </svg>
            <span>farseer</span>
          </div>
          <button
            className="icon-button collapse-sidebar"
            aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
            onClick={() => setSidebarCollapsed((current) => !current)}
          >
            <svg viewBox="0 0 24 24" aria-hidden>
              <path d={sidebarCollapsed ? "m9.5 6 6 6-6 6" : "m14.5 6-6 6 6 6"} />
            </svg>
          </button>
        </div>

        <div className="fleet-switcher">
          <span className="fleet-mark" aria-hidden>F</span>
          <span className="fleet-copy">
            <b>Local fleet</b>
            <small>layout saved to farseer</small>
          </span>
        </div>

        <nav className="widget-nav" aria-label="Home widgets">
          <p className="group-label">Home widgets</p>
          {built.map(({ id, title, subtitle }) => (
            <WidgetChip
              key={id}
              id={id}
              title={title}
              subtitle={subtitle}
              layout={layout}
              persist={persist}
            />
          ))}
          {authored.length > 0 && (
            <>
              <p className="group-label authored-label">Authored</p>
              {authored.map(({ id, title, subtitle }) => (
                <WidgetChip
                  key={id}
                  id={id}
                  title={title}
                  subtitle={subtitle}
                  layout={layout}
                  persist={persist}
                />
              ))}
            </>
          )}
        </nav>

        <UnitControls unit={layout.unit} persist={persist} />

        <div className="sidebar-foot">
          <span className="status-orb" aria-hidden />
          <span className="sidebar-foot-copy">
            <b>Windows native</b>
            <small>local operator surface</small>
          </span>
        </div>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <button
            className="icon-button open-sidebar"
            aria-label="Open widget navigation"
            onClick={() => {
              setSidebarCollapsed(false);
              setSidebarOpen(true);
            }}
          >
            <svg viewBox="0 0 24 24" aria-hidden>
              <path d="M4 7h16M4 12h16M4 17h16" />
            </svg>
          </button>
          <div className="crumb">
            <b>Home</b>
            <span>live canvas</span>
          </div>
          <div className="top-actions">
            <span className="saved-state"><span className="status-orb" aria-hidden />arrangement saved</span>
            <button
              className="icon-button"
              aria-pressed={layout.mounted.includes("clock")}
              aria-label={layout.mounted.includes("clock") ? "Hide clock widget" : "Show clock widget"}
              title={layout.mounted.includes("clock") ? "Hide clock widget" : "Show clock widget"}
              onClick={() => persist((current) => toggleMounted(current, "clock", true))}
            >
              <svg viewBox="0 0 24 24" aria-hidden>
                <circle cx="12" cy="12" r="7.5" />
                <path d="M12 7.5V12l3 2" />
              </svg>
            </button>
            <button
              className="icon-button"
              aria-pressed={settingsOpen}
              aria-label={settingsOpen ? "Close settings" : "Open settings"}
              title={settingsOpen ? "Close settings" : "Open settings"}
              onClick={() => setSettingsOpen((current) => !current)}
            >
              <svg viewBox="0 0 24 24" aria-hidden>
                <circle cx="12" cy="12" r="3" />
                <path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" />
              </svg>
            </button>
            <span className="profile" aria-label="Local operator">
              <svg viewBox="0 0 24 24" aria-hidden>
                <circle cx="12" cy="8.5" r="3.5" />
                <path d="M5.5 20c.8-4 3-6 6.5-6s5.7 2 6.5 6" />
              </svg>
            </span>
          </div>
        </header>
        {settingsOpen && (
          <aside className="settings-popover" aria-label="Settings">
            <div className="row">
              <b>Settings</b>
              <button className="chip" onClick={() => setSettingsOpen(false)}>close</button>
            </div>
            <SettingsWidget bridge={bridge} />
          </aside>
        )}

        <GateBar />

        <main
          className="canvas"
          ref={board}
          style={
            {
              "--unit-width": `${layout.unit.width}px`,
              "--unit-height": `${layout.unit.height}px`,
            } as React.CSSProperties
          }
        >
        {layout.mounted.map((id) => {
          const widget = available.find((candidate) => candidate.id === id);
          if (!widget) return null;
          const span = normalizeSpan(preview?.id === id ? preview.span : layout.span[id]);
          return (
            <section
              key={id}
              aria-labelledby={`widget-${id}-title`}
              data-widget-id={id}
              className={[
                "widget",
                `widget-${id}`,
                dragging === id ? "dragging" : "",
                over === id && dragging !== id ? "drop-target" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              // `28 operator surface` makes CSS grid spans the sole rendered
              // interpretation of the arrangement blob.
              style={
                {
                  "--w": span.w,
                  "--h": span.h,
                } as React.CSSProperties
              }
              onFocus={() => setAnchor({ widget: widget.title })}
              onMouseEnter={() => setAnchor({ widget: widget.title })}
              onPointerDown={() => setAnchor({ widget: widget.title })}
              onContextMenu={(event) => {
                event.preventDefault();
                setAnchor({ widget: widget.title });
                setContextMenu({
                  id,
                  title: widget.title,
                  x: Math.max(8, Math.min(event.clientX, window.innerWidth - 196)),
                  y: Math.max(8, Math.min(event.clientY, window.innerHeight - 174)),
                });
              }}
            >
              <div className="head">
                <span
                  className="grip"
                  role="button"
                  aria-label={`move the ${widget.title} widget - arrow keys, or drag`}
                  aria-keyshortcuts="ArrowLeft ArrowRight ArrowUp ArrowDown"
                  title="drag to move this widget, or focus it and press an arrow key"
                  // Focusable, because a grip that only answers to a drag is a
                  // canvas a keyboard cannot arrange at all - and the drag was
                  // itself the fix for a grip that looked like a handle and
                  // moved nothing. Same failure, one input device along.
                  tabIndex={0}
                  onKeyDown={(event) => {
                    const by =
                      event.key === "ArrowLeft" || event.key === "ArrowUp"
                        ? -1
                        : event.key === "ArrowRight" || event.key === "ArrowDown"
                          ? 1
                          : 0;
                    if (by === 0) return;
                    event.preventDefault();
                    persist((current) => {
                      const order = [...current.mounted];
                      const at = order.indexOf(id);
                      const to = at + by;
                      if (at < 0 || to < 0 || to >= order.length) return current;
                      // The same move the drop performs, one place at a time.
                      // React reconciles the section by key, so the grip travels
                      // with its card and keeps focus for the next press.
                      order.splice(to, 0, ...order.splice(at, 1));
                      return { ...current, mounted: order };
                    });
                  }}
                  // Pointer events are shared by Chromium and WebView2, unlike
                  // native HTML drag-and-drop, which never completed its drop
                  // in the desktop shell.
                  onPointerDown={(event) => {
                    if (event.button !== 0) return;
                    event.preventDefault();
                    moving.current = { id, pointerId: event.pointerId };
                    setDragging(id);
                    event.currentTarget.setPointerCapture(event.pointerId);
                  }}
                  onPointerMove={(event) => {
                    const active = moving.current;
                    if (!active || active.pointerId !== event.pointerId) return;
                    const target = document
                      .elementFromPoint(event.clientX, event.clientY)
                      ?.closest<HTMLElement>("[data-widget-id]")?.dataset.widgetId;
                    setOver(target && target !== active.id ? target : null);
                  }}
                  onPointerUp={(event) => {
                    const active = moving.current;
                    if (!active || active.pointerId !== event.pointerId) return;
                    const target = document
                      .elementFromPoint(event.clientX, event.clientY)
                      ?.closest<HTMLElement>("[data-widget-id]")?.dataset.widgetId;
                    moving.current = null;
                    setDragging(null);
                    setOver(null);
                    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                      event.currentTarget.releasePointerCapture(event.pointerId);
                    }
                    if (!target || target === active.id) return;
                    persist((current) => {
                      const mounted = moveToSlot(current.mounted, active.id, target);
                      return mounted === current.mounted ? current : { ...current, mounted };
                    });
                  }}
                  onPointerCancel={() => {
                    moving.current = null;
                    setDragging(null);
                    setOver(null);
                  }}
                >
                  <svg viewBox="0 0 16 16" aria-hidden focusable="false">
                    <circle cx="5" cy="5" r="1" />
                    <circle cx="11" cy="5" r="1" />
                    <circle cx="5" cy="11" r="1" />
                    <circle cx="11" cy="11" r="1" />
                  </svg>
                </span>
                <h2 id={`widget-${id}-title`}>{widget.title}</h2>
                <span className="dim small">{widget.subtitle}</span>
                {widget.agent && (
                  <span className="badge agent" title="written into widgets/ and compiled here, sandboxed">
                    authored
                  </span>
                )}
                <span className="grow" />
                <select
                  className="badge size"
                  aria-label={`Size of ${widget.title} widget`}
                  value={`${span.w}x${span.h}`}
                  onChange={(event) => {
                    const next = WIDGET_SPANS.find(
                      (candidate) => `${candidate.w}x${candidate.h}` === event.currentTarget.value,
                    );
                    if (!next) return;
                    persist((current) => ({
                      ...current,
                      span: { ...current.span, [id]: next },
                    }));
                  }}
                >
                  {WIDGET_SPANS.map((candidate) => (
                    <option key={`${candidate.w}x${candidate.h}`} value={`${candidate.w}x${candidate.h}`}>
                      {candidate.w}x{candidate.h}
                    </option>
                  ))}
                </select>
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
              <ResizeHandle
                id={id}
                title={widget.title}
                span={span}
                gridStep={gridStep}
                preview={setPreview}
                persist={persist}
              />
            </section>
          );
        })}
        {layout.mounted.length === 0 && (
          <p className="empty">Nothing is visible. Show a widget from the sidebar.</p>
        )}
      </main>
        {contextMenu && (
          <div
            className="widget-context-menu"
            role="menu"
            aria-label={`${contextMenu.title} widget actions`}
            style={{ left: contextMenu.x, top: contextMenu.y }}
            onContextMenu={(event) => event.preventDefault()}
          >
            <p>{contextMenu.title}</p>
            <button
              type="button"
              role="menuitem"
              disabled={contextIndex <= 0}
              onClick={() => {
                persist((current) => {
                  const mounted = moveBy(current.mounted, contextMenu.id, -1);
                  return mounted === current.mounted ? current : { ...current, mounted };
                });
                setContextMenu(null);
              }}
            >
              Move left
            </button>
            <button
              type="button"
              role="menuitem"
              disabled={contextIndex < 0 || contextIndex >= layout.mounted.length - 1}
              onClick={() => {
                persist((current) => {
                  const mounted = moveBy(current.mounted, contextMenu.id, 1);
                  return mounted === current.mounted ? current : { ...current, mounted };
                });
                setContextMenu(null);
              }}
            >
              Move right
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                persist((current) => ({
                  ...current,
                  span: { ...current.span, [contextMenu.id]: DEFAULT_WIDGET_SPAN },
                }));
                setContextMenu(null);
              }}
            >
              Reset to 1x1
            </button>
            <button
              type="button"
              role="menuitem"
              className="danger"
              onClick={() => {
                persist((current) => ({
                  ...current,
                  mounted: current.mounted.filter((id) => id !== contextMenu.id),
                }));
                setContextMenu(null);
              }}
            >
              Unpin from canvas
            </button>
          </div>
        )}

        <footer className="home-composer">
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
            <div className="composer-context">
              <span className="route-chip" title="every request goes to the top manager">
                to top manager
              </span>
              <button
                type="button"
                className="anchor-chip"
                title="reset the request context to the canvas"
                onClick={() => setAnchor({ widget: "canvas" })}
              >
                about {anchor.widget}
              </button>
            </div>
            <textarea
              name="ask"
              rows={1}
              autoComplete="off"
              disabled={asking}
              aria-label={`Ask the top manager about ${anchor.widget}`}
              placeholder={`Ask the top manager about ${anchor.widget}`}
            />
            <div className="composer-actions">
              <p role={error ? "alert" : "status"}>
                {error ? (
                  <span className="bad">{error}</span>
                ) : lastRun ? (
                  <>
                    accepted as run <span className="mono">{lastRun.slice(0, 8)}</span>
                  </>
                ) : (
                  "Point to a widget to change context."
                )}
              </p>
              <button
                className="send-button"
                disabled={asking}
                aria-label={asking ? "Sending to top manager" : "Send to top manager"}
              >
                <svg viewBox="0 0 24 24" aria-hidden>
                  <path d="m5 12 14-7-4 14-3-6Z" />
                </svg>
              </button>
            </div>
          </form>
        </footer>
      </section>
    </div>
  );
}
