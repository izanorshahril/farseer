/**
 * The host bridge: everything a widget may reach, and nothing else.
 *
 * `28 operator surface` gate 3 - a widget never holds the operator token and
 * never touches the file system. It reaches farseer only through this object,
 * which the host passes in. A widget that imports `fetch` directly is a widget
 * that has left the seam, and the import allowlist is what catches that.
 *
 * The second rule this file exists to keep is `28`'s correction: **a widget
 * displays a cell and never addresses one.** There is no `instructCell(id)`
 * here, deliberately. `ask` goes to the top manager, always, carrying an anchor
 * that says what the operator was looking at - the same shape as a comment on a
 * Claude Design component returning to the main agent.
 */

import { currentProject } from "./project";

/** Which cell farseer's operator talks to. `01 cell primitive` made it the address. */
const TOP_MANAGER = "zero";

export type Anchor = {
  /** The widget the operator was looking at when they typed. */
  widget: string;
  /** What it was showing, if anything - a cell, a run, an account. */
  subject?: string;
};

export type Bridge = {
  /** Read any `/v1` resource. Reads are safe. */
  read: <T>(path: string) => Promise<T>;
  /**
   * One of `05 run state model`'s verbs, on one run.
   *
   * Narrowed to `/runs/{id}/{verb}` rather than a general POST, and
   * **deliberately absent from the sandbox bridge** in `SandboxWidget.tsx`: a
   * widget the operator did not write can show a run, and cannot cancel one.
   */
  post: (path: string, body?: unknown) => Promise<unknown>;
  /**
   * Withdraw something. One entry today - a project root - and the same
   * allowlist discipline as `post`.
   *
   * A separate verb rather than a `POST /remove`, because `39 what an installed
   * farseer points at` makes revoking a grant the operator's act and it should
   * read as one wherever it is written down.
   */
  del: (path: string, body?: unknown) => Promise<void>;
  /**
   * Send a request to the **top manager**, anchored to where it came from.
   *
   * Returns the run id farseer accepted, not an answer: `16 local api surface`
   * made an instruction fire-and-forget, and the answer arrives on the stream.
   */
  ask: (anchor: Anchor, text: string) => Promise<string>;
  /** Read this widget's slice of the canvas blob. Opaque to farseer per `24`. */
  loadState: <T>(key: string) => Promise<T | null>;
  saveState: (key: string, value: unknown) => Promise<void>;
};

async function json<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, init);
  if (!response.ok) throw new Error(`${init?.method ?? "GET"} ${path}: ${response.status}`);
  return (await response.json()) as T;
}

export function createBridge(): Bridge {
  return {
    read: (path) => json(`/v1${path}`),

    post: async (path, body) => {
      // Still an allowlist, now with two entries. `28 operator surface` gate 3
      // narrows what a widget may reach, and the narrowing is the point - so a
      // second entry is added by name rather than by loosening the pattern.
      //
      // `quota/refresh` launches a poll the operator has already enabled in
      // `runners.toml`; the runtime refuses it when they have not.
      const allowed = [
        /^\/runs\/[0-9a-f-]+\/(steer|cancel|rerun|rescope)$/,
        /^\/quota\/refresh$/,
        // `39 what an installed farseer points at`: authorizing a folder and
        // creating a project inside one. Neither widens farseer's reach on its
        // own - the runtime refuses a path outside every authorized root.
        /^\/projects$/,
        /^\/projects\/roots$/,
      ];
      if (!allowed.some((pattern) => pattern.test(path))) {
        throw new Error(`${path} is not a verb this bridge offers`);
      }
      const response = await fetch(`/v1${path}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body ?? {}),
      });
      // `400` is a runner with no steering path and `404` a run already
      // finished. Both are the runtime refusing a verb the surface should not
      // have offered, so they surface as errors rather than being swallowed.
      //
      // The runtime's own sentence when it wrote one: a refusal that says
      // `quota/refresh: 400` tells the operator nothing, while the body says
      // which line of `runners.toml` to add.
      if (!response.ok) {
        const said = await response
          .json()
          .then((body: { error?: string }) => body.error)
          .catch(() => undefined);
        throw new Error(said ?? `${path}: ${response.status}`);
      }
      // The body, for the verbs that answer with one. `rerun` and `rescope`
      // return the **new** run's id - they start a run rather than change this
      // one - and a caller that cannot see it has no way to follow what it just
      // started. `204` and an unparseable body are both `undefined`, which is
      // what `cancel` and `steer` have always effectively returned.
      return await response.json().catch(() => undefined);
    },

    del: async (path, body) => {
      if (path !== "/projects/roots") {
        throw new Error(`${path} is not a verb this bridge offers`);
      }
      const response = await fetch(`/v1${path}`, {
        method: "DELETE",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body ?? {}),
      });
      if (!response.ok) {
        const said = await response
          .json()
          .then((body: { error?: string }) => body.error)
          .catch(() => undefined);
        throw new Error(said ?? `${path}: ${response.status}`);
      }
    },

    ask: async (anchor, text) => {
      // The anchor rides in the goal as prose because the reader is an LLM.
      // `16 local api surface`'s additive-only promise leaves room for a
      // structured `context` field the day something needs to machine-read it.
      const where = anchor.subject ? `${anchor.widget}, showing ${anchor.subject}` : anchor.widget;
      const body = await json<{ run_id: string }>(`/v1/cells/${TOP_MANAGER}/instruct`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          goal: `[from the ${where} widget]\n${text}`,
          // `39 what an installed farseer points at`: which project this goes
          // to, or nothing at all - which the runtime reads as farseer's own
          // working directory.
          project: currentProject(),
        }),
      });
      return body.run_id;
    },

    loadState: async <T,>(key: string) => {
      const response = await fetch(`/v1/ui-state/${encodeURIComponent(key)}`);
      if (response.status === 404) return null;
      if (!response.ok) throw new Error(`GET ui-state/${key}: ${response.status}`);
      return (await response.json()) as T;
    },

    saveState: async (key, value) => {
      const response = await fetch(`/v1/ui-state/${encodeURIComponent(key)}`, {
        method: "PUT",
        body: JSON.stringify(value),
      });
      // 413 is the 1 MiB value cap and 414 the 256-byte key cap, both from
      // `24 ui state persistence`. A layout hitting either is a bug in the
      // widget, not something to retry.
      if (!response.ok) throw new Error(`PUT ui-state/${key}: ${response.status}`);
    },
  };
}
