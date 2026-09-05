import { describe, expect, test } from "bun:test";
import {
  DEFAULT_WIDGET_SPAN,
  DEFAULT_WIDGET_UNIT,
  WIDGET_SPANS,
  moveBy,
  moveToSlot,
  normalizeLayout,
  normalizeUnit,
  nudged,
  resized,
  toggleMounted,
  type CanvasLayout,
} from "../src/layout";

const fallback: CanvasLayout = {
  v: 8,
  mounted: ["conversation"],
  span: { conversation: DEFAULT_WIDGET_SPAN },
  unit: DEFAULT_WIDGET_UNIT,
};

describe("canvas layout", () => {
  test("repairs a malformed current blob before rendering", () => {
    expect(normalizeLayout({ v: 8, mounted: ["conversation"] }, fallback)).toEqual({
      v: 8,
      mounted: ["conversation"],
      span: { conversation: DEFAULT_WIDGET_SPAN },
      unit: DEFAULT_WIDGET_UNIT,
    });
    expect(
      normalizeLayout(
        {
          v: 8,
          mounted: ["conversation", "conversation"],
          span: { conversation: { w: 99, h: -2 }, hidden: { w: 1.4, h: 1.6 } },
          unit: { width: 319.6, height: 239.4 },
        },
        fallback,
      ),
    ).toEqual({
      v: 8,
      mounted: ["conversation"],
      span: { conversation: { w: 2, h: 1 }, hidden: { w: 1, h: 2 } },
      unit: { width: 320, height: 239 },
    });
  });

  test("resets stale or structurally invalid blobs", () => {
    expect(normalizeLayout({ v: 7, mounted: [], span: {} }, fallback)).toBe(fallback);
    expect(normalizeLayout({ v: 8, mounted: [7], span: {} }, fallback)).toBe(fallback);
  });

  test("offers four standard sizes with one by one first", () => {
    expect(WIDGET_SPANS).toEqual([
      { w: 1, h: 1 },
      { w: 2, h: 1 },
      { w: 2, h: 2 },
      { w: 1, h: 2 },
    ]);
    expect(resized({ w: 1, h: 1 }, 312, 0, { x: 312, y: 252 })).toEqual({ w: 2, h: 1 });
    expect(nudged({ w: 2, h: 2 }, "ArrowRight")).toBeNull();
    expect(nudged({ w: 1, h: 1 }, "ArrowDown")).toEqual({ w: 1, h: 2 });
  });

  test("configures the base widget metric to one pixel", () => {
    expect(normalizeUnit({ width: 321.4, height: 238.6 })).toEqual({ width: 321, height: 239 });
    expect(normalizeUnit({ width: 1, height: 9999 })).toEqual({ width: 200, height: 500 });
  });

  test("moves a widget into the target's original slot from either direction", () => {
    expect(moveToSlot(["activity", "runners", "projects", "delegation", "runs"], "activity", "runs"))
      .toEqual(["runners", "projects", "delegation", "runs", "activity"]);
    expect(moveToSlot(["activity", "runners", "projects", "delegation", "runs"], "runs", "activity"))
      .toEqual(["runs", "activity", "runners", "projects", "delegation"]);
    expect(moveToSlot(["projects", "clock"], "projects", "projects")).toEqual(["projects", "clock"]);
  });

  test("shares mount and one-slot movement across every control", () => {
    const shown = toggleMounted(fallback, "clock", true);
    expect(shown.mounted).toEqual(["clock", "conversation"]);
    expect(toggleMounted(shown, "clock").mounted).toEqual(["conversation"]);
    expect(moveBy(["a", "b", "c"], "b", -1)).toEqual(["b", "a", "c"]);
    expect(moveBy(["a", "b", "c"], "b", 1)).toEqual(["a", "c", "b"]);
  });
});
