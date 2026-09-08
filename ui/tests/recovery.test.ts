import { describe, expect, test } from "bun:test";
import { readFailureIncident, readFailureStatus } from "../src/ReadFailure";
import { createBridge } from "../src/bridge";
import { follow, onStreamState, type RecordEvent } from "../src/stream";
import { WidgetBoundary } from "../src/WidgetBoundary";

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

const frame = (value: RecordEvent) =>
  `event: run_finished\ndata: ${JSON.stringify(value)}\n\n`;

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

  test("localizes widget 404 and malformed JSON reads", async () => {
    const original = globalThis.fetch;
    let calls = 0;
    try {
      globalThis.fetch = (async () => {
        calls += 1;
        if (calls === 1) return new Response("{not-json", { status: 200 });
        return new Response("{\"error\":\"C:\\\\Users\\secret\"}", { status: 404 });
      }) as typeof fetch;
      const bridge = createBridge();

      let malformed: unknown;
      try {
        await bridge.read("/projects");
      } catch (error) {
        malformed = error;
      }
      expect(readFailureStatus(malformed)).toBe("unknown");
      expect(readFailureIncident("projects", malformed)).not.toContain("secret");

      let missing: unknown;
      try {
        await bridge.read("/projects?path=C:\\\\Users\\secret");
      } catch (error) {
        missing = error;
      }
      expect(readFailureStatus(missing)).toBe("404");
      expect(readFailureIncident("projects", missing)).not.toContain("secret");
    } finally {
      globalThis.fetch = original;
    }
  });

  test("drops replayed stream frames at the cursor seam", async () => {
    const original = globalThis.fetch;
    const seen: number[] = [];
    let subscription: { close: () => void } | undefined;
    try {
      globalThis.fetch = (async () => new Response(
        new ReadableStream({
          start(controller) {
            const frames = [event(5), event(5), event(6)].map(frame).join("");
            controller.enqueue(new TextEncoder().encode(frames));
            controller.close();
          },
        }),
        { status: 200 },
      )) as typeof fetch;
      subscription = follow((value) => seen.push(value.seq), { since: 4 });
      await new Promise((resolve) => setTimeout(resolve, 25));
      expect(seen).toEqual([5, 6]);
    } finally {
      subscription?.close();
      globalThis.fetch = original;
    }
  });

  test("reconnects after EOF from the last exclusive cursor", async () => {
    const original = globalThis.fetch;
    const requests: string[] = [];
    const states: string[] = [];
    let calls = 0;
    globalThis.fetch = (async (input) => {
      requests.push(String(input));
      calls += 1;
      const values = calls === 1 ? [event(1)] : [event(1), event(2)];
      return new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(new TextEncoder().encode(values.map(frame).join("")));
            controller.close();
          },
        }),
        { status: 200 },
      );
    }) as typeof fetch;

    const seen: number[] = [];
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => { finish = resolve; });
    let subscription: { close: () => void } | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      subscription = follow((value) => {
        seen.push(value.seq);
        if (seen.length === 2) finish();
      }, { since: 0, onState: (state) => states.push(state), reconnectDelayMs: 0 });
      await Promise.race([
        finished,
        new Promise((_, reject) => {
          timeout = setTimeout(() => reject(new Error("reconnect timed out")), 2_500);
        }),
      ]);
      expect(seen).toEqual([1, 2]);
      expect(requests).toEqual(["/v1/stream?since=0", "/v1/stream?since=1"]);
      const firstLive = states.indexOf("live");
      const stale = states.indexOf("stale", firstLive + 1);
      const secondLive = states.indexOf("live", stale + 1);
      expect(firstLive).toBeGreaterThanOrEqual(0);
      expect(stale).toBeGreaterThan(firstLive);
      expect(secondLive).toBeGreaterThan(stale);
    } finally {
      subscription?.close();
      if (timeout) clearTimeout(timeout);
      globalThis.fetch = original;
    }
  });

  test("retries a rejected stream fetch from the same cursor", async () => {
    const original = globalThis.fetch;
    const requests: string[] = [];
    const states: string[] = [];
    let calls = 0;
    let nonOkStateObserved = false;
    const seen: number[] = [];
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => { finish = resolve; });
    let subscription: { close: () => void } | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      globalThis.fetch = (async (input) => {
        requests.push(String(input));
        calls += 1;
        if (calls === 1) return new Response("temporarily unavailable", { status: 404 });
        if (calls === 2) throw new Error("offline");
        return new Response(
          new ReadableStream({
            start(controller) {
              controller.enqueue(new TextEncoder().encode(frame(event(7))));
              controller.close();
            },
          }),
          { status: 200 },
        );
      }) as typeof fetch;
      subscription = follow((value) => {
        seen.push(value.seq);
        finish();
      }, {
        since: 6,
        onState: (state) => {
          states.push(state);
          if (state === "stale" && calls === 1) nonOkStateObserved = true;
        },
        reconnectDelayMs: 0,
      });
      await Promise.race([
        finished,
        new Promise((_, reject) => {
          timeout = setTimeout(() => reject(new Error("transport retry timed out")), 2_500);
        }),
      ]);
      expect(seen).toEqual([7]);
      expect(requests).toEqual([
        "/v1/stream?since=6",
        "/v1/stream?since=6",
        "/v1/stream?since=6",
      ]);
      expect(states).toContain("stale");
      expect(states).toContain("live");
      expect(nonOkStateObserved).toBe(true);
    } finally {
      subscription?.close();
      if (timeout) clearTimeout(timeout);
      globalThis.fetch = original;
    }
  });

  test("survives malformed SSE data and dispatches the next valid frame", async () => {
    const original = globalThis.fetch;
    let subscription: { close: () => void } | undefined;
    try {
      globalThis.fetch = (async () => new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(new TextEncoder().encode(`event: run_finished
data: {bad json}

event: run_finished
data: ${JSON.stringify(event(8))}

`));
          },
        }),
        { status: 200 },
      )) as typeof fetch;
      const seen: number[] = [];
      subscription = follow((value) => seen.push(value.seq), { since: 7 });
      await new Promise((resolve) => setTimeout(resolve, 25));
      expect(seen).toEqual([8]);
    } finally {
      subscription?.close();
      globalThis.fetch = original;
    }
  });

  test("keeps cursor-specific state out of the shared canvas state", async () => {
    const original = globalThis.fetch;
    const sharedStates: string[] = [];
    const cursorStates: string[] = [];
    let sharedSubscription: { close: () => void } | undefined;
    let cursorSubscription: { close: () => void } | undefined;
    let removeObservedShared: (() => void) | undefined;
    const removeSharedState = onStreamState((state) => sharedStates.push(state));
    try {
      globalThis.fetch = (async (input) => {
        if (String(input).includes("since=")) {
          return new Response("cursor unavailable", { status: 404 });
        }
        return new Response(
          new ReadableStream({
            start(controller) {
              controller.enqueue(new TextEncoder().encode(frame(event(9))));
            },
          }),
          { status: 200 },
        );
      }) as typeof fetch;
      let sharedLive!: () => void;
      const live = new Promise<void>((resolve) => { sharedLive = resolve; });
      const sharedSubscriberStates: string[] = [];
      removeSharedState();
      sharedStates.length = 0;
      removeObservedShared = onStreamState((state) => {
        sharedStates.push(state);
        if (state === "live") sharedLive();
      });
      sharedSubscription = follow(() => {}, {
        onState: (state) => sharedSubscriberStates.push(state),
        reconnectDelayMs: 0,
      });
      await live;
      cursorSubscription = follow(() => {}, {
        since: 8,
        reconnectDelayMs: 1_000,
        onState: (state) => cursorStates.push(state),
      });
      await new Promise((resolve) => setTimeout(resolve, 25));
      expect(cursorStates).toContain("stale");
      expect(sharedStates).toEqual(["connecting", "live"]);
      expect(sharedSubscriberStates).toEqual(["connecting", "live"]);
    } finally {
      cursorSubscription?.close();
      sharedSubscription?.close();
      removeObservedShared?.();
      removeSharedState();
      globalThis.fetch = original;
    }
  });
});
