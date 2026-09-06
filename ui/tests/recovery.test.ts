import { describe, expect, test } from "bun:test";
import { readFailureIncident, readFailureStatus } from "../src/ReadFailure";
import { follow, type RecordEvent } from "../src/stream";
import { WidgetBoundary } from "../src/WidgetBoundary";

describe("widget recovery", () => {
  test("turns a widget render throw into an isolated retry state", () => {
    const state = WidgetBoundary.getDerivedStateFromError(new Error("private path"));
    expect(state.error).toBe(true);
    expect(state.incident).toMatch(/^widget-/);
    expect(state.incident).not.toContain("private");
  });

  test("reduces a backend failure to a safe status diagnostic", () => {
    const error = new Error("GET /v1/projects?path=C:\\Users\\secret: 503 bearer=hidden");
    expect(readFailureStatus(error)).toBe("503");
    const incident = readFailureIncident("project roots", error);
    expect(incident).toMatch(/^read-project-roots-/);
    expect(incident).not.toContain("secret");
    expect(incident).not.toContain("hidden");
  });

  test("drops replayed stream frames at the cursor seam", async () => {
    const original = globalThis.fetch;
    const event = (seq: number): RecordEvent => ({
      seq,
      event_id: `event-${seq}`,
      ts: seq,
      cell_id: "zero",
      run_id: "run",
      kind: "run_finished",
      actor: "system",
      payload: {},
    });
    globalThis.fetch = (async () => new Response(
      new ReadableStream({
        start(controller) {
          const frames = [event(5), event(5), event(6)]
            .map((value) => `event: run_finished\ndata: ${JSON.stringify(value)}\n\n`)
            .join("");
          controller.enqueue(new TextEncoder().encode(frames));
          controller.close();
        },
      }),
      { status: 200 },
    )) as typeof fetch;

    const seen: number[] = [];
    const subscription = follow((value) => seen.push(value.seq), { since: 4 });
    await new Promise((resolve) => setTimeout(resolve, 25));
    subscription.close();
    globalThis.fetch = original;
    expect(seen).toEqual([5, 6]);
  });
});
