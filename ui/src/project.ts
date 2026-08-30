/**
 * Which project the canvas is working in.
 *
 * `39 what an installed farseer points at` settled that farseer manages several
 * projects rather than living inside one, so "which repository" stopped being a
 * property of the process and became a choice the operator makes on the screen.
 * Every instruction the canvas sends carries it.
 *
 * Same shape as `selection.ts` - subscribe, notify, unsubscribe - and for the
 * same reason: one fact crossing the gap between widgets that otherwise know
 * nothing about each other.
 *
 * **Persisted, where the selected run is not.** `24 ui state persistence` draws
 * the line at an arrangement the operator chose: which run they had open two
 * days ago is not one, and which project they are working in is - reopening the
 * canvas in a different repository from the one they left it in would be the
 * surface changing its mind about where the work goes.
 */
const KEY = "project";

type Listener = (project: string | null) => void;

let current: string | null = null;
const listeners = new Set<Listener>();

/** The project's absolute path, or `null` for farseer's own working directory. */
export function currentProject(): string | null {
  return current;
}

export function setProject(path: string | null): void {
  current = path;
  for (const listener of [...listeners]) listener(current);
  // Fire and forget: a failed write costs the operator one re-pick after a
  // restart, and blocking the switch on a store round trip costs them the
  // switch.
  void fetch(`/v1/ui-state/${KEY}`, { method: "PUT", body: JSON.stringify({ path }) }).catch(
    () => {},
  );
}

export function onProject(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Read the stored choice back, once, at startup.
 *
 * Not validated here. The runtime refuses a project outside every authorized
 * root on the launch that names it, and a canvas that silently dropped a stored
 * path would leave the operator wondering which project their instruction went
 * to - which is the failure this whole file exists to prevent.
 */
export async function restoreProject(): Promise<void> {
  try {
    const response = await fetch(`/v1/ui-state/${KEY}`);
    if (!response.ok) return;
    const stored = (await response.json()) as { path?: string | null };
    if (typeof stored?.path === "string") {
      current = stored.path;
      for (const listener of [...listeners]) listener(current);
    }
  } catch {
    // No stored choice is the resting state, not an error.
  }
}
