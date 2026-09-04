/**
 * Where a widget sits and how big it is.
 *
 * `24 ui state persistence` keeps the arrangement in farseer rather than in the
 * browser, and this module is the arithmetic that arrangement is made of. It is
 * deliberately the only place that knows what a column is worth, so a resize
 * handle, a keyboard press and a stored blob cannot disagree about it.
 *
 * **The board flows; it is not a plane.** Widgets take a span and the grid packs
 * them, rather than each holding an (x, y) it defends against its neighbours.
 * The alternative - free placement - needs collision resolution, and collision
 * resolution on a nine-card board spends a few hundred lines letting an operator
 * make a mess that a second pass then has to tidy. Spans cannot overlap, so
 * there is no mess to tidy and no camera to drive.
 */

/** The four widget scales, in the order shown by every size control. */
export const WIDGET_SPANS: Span[] = [
  { w: 1, h: 1 },
  { w: 2, h: 1 },
  { w: 2, h: 2 },
  { w: 1, h: 2 },
];

/** `28 operator surface` keeps each scale axis to one or two base units. */
export const MIN_W = 1;
export const MIN_H = 1;
export const MAX_W = 2;
export const MAX_H = 2;

export type Span = { w: number; h: number };
export type WidgetUnit = { width: number; height: number };
export type CanvasLayout = {
  v: number;
  mounted: string[];
  span: Record<string, Span>;
  unit: WidgetUnit;
};

/** The first standard size is the default for every newly mounted widget. */
export const DEFAULT_WIDGET_SPAN: Span = { w: 1, h: 1 };

/** The pixel metric represented by one widget unit. */
export const DEFAULT_WIDGET_UNIT: WidgetUnit = { width: 300, height: 220 };
export const MIN_UNIT_WIDTH = 200;
export const MAX_UNIT_WIDTH = 600;
export const MIN_UNIT_HEIGHT = 140;
export const MAX_UNIT_HEIGHT = 500;

const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Normalize the operator-configurable pixel metric for one widget unit. */
export function normalizeUnit(value: unknown): WidgetUnit {
  const unit = record(value) ? value : {};
  const width =
    typeof unit.width === "number" && Number.isFinite(unit.width)
      ? unit.width
      : DEFAULT_WIDGET_UNIT.width;
  const height =
    typeof unit.height === "number" && Number.isFinite(unit.height)
      ? unit.height
      : DEFAULT_WIDGET_UNIT.height;
  return {
    width: Math.max(MIN_UNIT_WIDTH, Math.min(MAX_UNIT_WIDTH, Math.round(width))),
    height: Math.max(MIN_UNIT_HEIGHT, Math.min(MAX_UNIT_HEIGHT, Math.round(height))),
  };
}

/**
 * Normalize one untrusted stored span.
 *
 * `24 ui state persistence` leaves opaque blob validation to this client.
 */
export function normalizeSpan(value: unknown): Span {
  const span = record(value) ? value : {};
  const w =
    typeof span.w === "number" && Number.isFinite(span.w) ? span.w : DEFAULT_WIDGET_SPAN.w;
  const h =
    typeof span.h === "number" && Number.isFinite(span.h) ? span.h : DEFAULT_WIDGET_SPAN.h;
  return {
    w: Math.max(MIN_W, Math.min(MAX_W, Math.round(w))),
    h: Math.max(MIN_H, Math.min(MAX_H, Math.round(h))),
  };
}

/**
 * Normalize the canvas blob before React sees it.
 *
 * `24 ui state persistence` makes malformed and stale blobs the UI's problem.
 * A prior version resets to the current default; a damaged current version
 * keeps its valid arrangement and repairs missing spans.
 */
export function normalizeLayout(value: unknown, fallback: CanvasLayout): CanvasLayout {
  if (
    !record(value) ||
    value.v !== fallback.v ||
    !Array.isArray(value.mounted) ||
    !value.mounted.every((id) => typeof id === "string")
  ) {
    return fallback;
  }
  const mounted = [...new Set(value.mounted)];
  const stored = record(value.span) ? value.span : {};
  const ids = new Set([...Object.keys(stored), ...mounted]);
  return {
    v: fallback.v,
    mounted,
    span: Object.fromEntries([...ids].map((id) => [id, normalizeSpan(stored[id])])),
    unit: normalizeUnit(value.unit),
  };
}

/**
 * The span a drag lands on.
 *
 * `28 operator surface` makes pointer resizing use the rendered grid step, so
 * its 12px gutters cannot accumulate drift between the pointer and the card.
 */
export function resized(
  from: Span,
  dx: number,
  dy: number,
  step: { x: number; y: number },
): Span {
  return normalizeSpan({
    w: from.w + Math.round(dx / step.x),
    h: from.h + Math.round(dy / step.y),
  });
}

/**
 * The span an arrow key lands on.
 *
 * `28 operator surface` gives the resize handle both axes so keyboard and
 * pointer users edit the same arrangement.
 */
export function nudged(from: Span, key: string): Span | null {
  const by: Record<string, [number, number]> = {
    ArrowLeft: [-1, 0],
    ArrowRight: [1, 0],
    ArrowUp: [0, -1],
    ArrowDown: [0, 1],
  };
  const step = by[key];
  if (!step) return null;
  const next = normalizeSpan({ w: from.w + step[0], h: from.h + step[1] });
  return next.w === from.w && next.h === from.h ? null : next;
}

/**
 * Move one widget into another widget's current slot without mutating order.
 *
 * Inserting at the target's original index is deliberate: removing a source
 * from its left shifts the target left, and inserting before that shifted
 * target would land one slot early.
 *
 * Pointer and keyboard movement share this operation so desktop and browser
 * canvases cannot disagree about what a move means.
 */
export function moveToSlot(order: string[], from: string, target: string): string[] {
  if (from === target || !order.includes(from) || !order.includes(target)) return order;
  const targetIndex = order.indexOf(target);
  const next = order.filter((id) => id !== from);
  next.splice(targetIndex, 0, from);
  return next;
}

/** Show or hide one widget while repairing its stored span. */
export function toggleMounted(layout: CanvasLayout, id: string, first = false): CanvasLayout {
  const mounted = layout.mounted.includes(id)
    ? layout.mounted.filter((widget) => widget !== id)
    : first
      ? [id, ...layout.mounted]
      : [...layout.mounted, id];
  return {
    ...layout,
    mounted,
    span: { ...layout.span, [id]: normalizeSpan(layout.span[id]) },
  };
}

/** Move a mounted widget by one or more slots, clamped to the list. */
export function moveBy(order: string[], id: string, delta: number): string[] {
  const from = order.indexOf(id);
  const to = Math.max(0, Math.min(order.length - 1, from + delta));
  if (from < 0 || from === to) return order;
  const moved = [...order];
  moved.splice(to, 0, ...moved.splice(from, 1));
  return moved;
}
