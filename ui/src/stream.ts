/**
 * The record, live.
 *
 * `16 local api surface` made this one endpoint, scoped **server-side**, and
 * `07 attach semantics` made live and replay the same call with a different
 * cursor. So there is nothing here for "attach to a running worker" that is not
 * also "replay a finished one" - only where the cursor starts.
 *
 * Parsed by hand rather than with `EventSource`, for one concrete reason: farseer
 * puts the **event kind** in the SSE `event:` field, and `02 record scope` left
 * kinds open because runner adapters emit ones farseer's own code does not name.
 * `EventSource.onmessage` fires only for unnamed events, so it would silently
 * receive nothing at all.
 */
export type RecordEvent = {
  seq: number;
  event_id: string;
  ts: number;
  cell_id: string;
  run_id: string;
  kind: string;
  actor: string;
  payload: unknown;
};

export type Subscription = { close: () => void };

/**
 * Follow the log from `since`, reconnecting on its own.
 *
 * The cursor is exclusive, so a reconnect resumes with no gap and no duplicate -
 * which is what makes dropping the connection a non-event rather than a hole in
 * the operator's view.
 */
/**
 * The one connection this page holds, and everyone who is listening to it.
 *
 * `16 local api surface` gives farseer **one** stream endpoint. The canvas had
 * quietly stopped agreeing with that: every widget that wanted live data called
 * `follow` and got its own connection, and at five of them the page hit the
 * browser's six-connection-per-origin limit for HTTP/1.1. An SSE stream never
 * closes, so those five were permanent - and the sixth request the page made,
 * whatever it happened to be, hung forever with no error. The Cells widget sat
 * on "reading cells..." because a *different* widget had taken the last socket.
 *
 * One connection, fanned out. Adding a live widget is now free, which is the
 * property that was silently missing.
 */
let shared: { subscribers: Set<(event: RecordEvent) => void>; stop: () => void } | null = null;

/**
 * Subscribe to the record.
 *
 * A caller passing `since` gets its own connection, because a cursor is a
 * position and two readers at different positions are two reads. Everybody else
 * - which is every widget on the canvas - shares one.
 */
export function follow(
  onEvent: (event: RecordEvent) => void,
  options: { since?: number } = {},
): Subscription {
  if (options.since === undefined) {
    if (!shared) {
      const subscribers = new Set<(event: RecordEvent) => void>();
      const connection = connect((event) => {
        // A copy, so a subscriber unsubscribing inside its own handler does not
        // mutate the set being iterated.
        for (const subscriber of [...subscribers]) subscriber(event);
      }, {});
      shared = { subscribers, stop: connection.close };
    }
    const here = shared;
    here.subscribers.add(onEvent);
    return {
      close: () => {
        here.subscribers.delete(onEvent);
        // The last widget to unmount closes the connection, so a page with no
        // live widgets holds no socket.
        if (here.subscribers.size === 0 && shared === here) {
          shared = null;
          here.stop();
        }
      },
    };
  }
  return connect(onEvent, options);
}

function connect(
  onEvent: (event: RecordEvent) => void,
  options: { since?: number },
): Subscription {
  const controller = new AbortController();
  let cursor = options.since;
  let stopped = false;

  const run = async () => {
    while (!stopped) {
      try {
        const query = cursor === undefined ? "" : `?since=${cursor}`;
        const response = await fetch(`/v1/stream${query}`, { signal: controller.signal });
        if (!response.ok || !response.body) throw new Error(`stream: ${response.status}`);
        const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
        let buffer = "";
        while (!stopped) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += value;
          // SSE frames are separated by a blank line. A comment-only frame is
          // farseer's quiet-tick probe and parses to nothing, which is correct.
          let split = buffer.indexOf("\n\n");
          while (split !== -1) {
            const frame = buffer.slice(0, split);
            buffer = buffer.slice(split + 2);
            const data = frame
              .split("\n")
              .filter((line) => line.startsWith("data:"))
              .map((line) => line.slice(5).trim())
              .join("\n");
            if (data) {
              try {
                const event = JSON.parse(data) as RecordEvent;
                cursor = event.seq;
                onEvent(event);
              } catch {
                // A frame farseer could not serialise arrives as a comment; it
                // is not worth tearing the stream down over.
              }
            }
            split = buffer.indexOf("\n\n");
          }
        }
      } catch (error) {
        if (stopped || (error as Error).name === "AbortError") return;
      }
      // Reconnect, unhurried. The cursor means nothing is lost by waiting.
      await new Promise((resolve) => setTimeout(resolve, 1_000));
    }
  };

  void run();
  return {
    close: () => {
      stopped = true;
      controller.abort();
    },
  };
}
