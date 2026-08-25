import { useCallback, useEffect, useState } from "react";

/**
 * `28 operator surface` gate 1: keep or undo, per turn.
 *
 * This is `12 autonomy and deny list`'s `reversible` level expressed in the UI,
 * and **git is what makes it true** - the bar reads `git status` scoped to
 * `widgets/` and the two buttons are `git add` and `git restore` plus a scoped
 * `git clean`.
 *
 * It says **what** changed rather than that something did. A bar that reads
 * "changes were made" teaches the operator to click through it; a bar naming
 * three files is one they read.
 *
 * `28` refused a fourth gate - review before a widget may mount - because it
 * fires twice for the same decision. This is that one decision.
 */
type Change = { state: string; file: string };

/** A widget cell zero wrote, waiting on the operator. */
type Pending = { branch: string; subject: string; id: string };

const label = (state: string) =>
  state === "??" ? "added" : state.includes("D") ? "deleted" : "changed";

export function GateBar() {
  const [changes, setChanges] = useState<Change[]>([]);
  const [pending, setPending] = useState<Pending[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(
    () =>
      Promise.all([
        fetch("/__widgets/changes")
          .then((response) => response.json() as Promise<Change[]>)
          .then(setChanges)
          .catch(() => setChanges([])),
        fetch("/__widgets/pending")
          .then((response) => response.json() as Promise<Pending[]>)
          .then(setPending)
          .catch(() => setPending([])),
      ]).then(() => undefined),
    [],
  );

  useEffect(() => {
    void refresh();
    const tick = setInterval(refresh, 5_000);
    return () => clearInterval(tick);
  }, [refresh]);

  const act = async (verb: "keep" | "undo") => {
    // Undo throws work away, and it is the operator's own click that does it -
    // but not without being told what it will reach.
    if (verb === "undo" && !confirm(`Discard ${changes.length} change(s) under widgets/?`)) return;
    setBusy(true);
    try {
      await fetch(`/__widgets/${verb}`, { method: "POST" });
      await refresh();
      if (verb === "undo") location.reload();
    } finally {
      setBusy(false);
    }
  };

  const decide = async (id: string, verb: "keep" | "undo") => {
    if (verb === "undo" && !confirm(`Delete the ${id} widget branch?`)) return;
    setBusy(true);
    try {
      await fetch(`/__widgets/pending/${id}/${verb}`, { method: "POST" });
      await refresh();
      location.reload();
    } finally {
      setBusy(false);
    }
  };

  if (changes.length === 0 && pending.length === 0) return null;

  return (
    <>
      {pending.map((widget) => (
        <div className="gate" key={widget.branch}>
          <b>cell zero wrote a widget</b>
          <span className="files">
            {widget.id} - {widget.subject}
          </span>
          <span className="grow" />
          <button
            className="chip undo"
            disabled={busy}
            onClick={() => void decide(widget.id, "undo")}
          >
            undo
          </button>
          <button
            className="chip on"
            disabled={busy}
            onClick={() => void decide(widget.id, "keep")}
          >
            keep
          </button>
        </div>
      ))}
      {changes.length > 0 && <WorkingTreeGate changes={changes} busy={busy} act={act} />}
    </>
  );
}

/** Uncommitted edits under `widgets/`, which is the other way widget code moves. */
function WorkingTreeGate({
  changes,
  busy,
  act,
}: {
  changes: Change[];
  busy: boolean;
  act: (verb: "keep" | "undo") => Promise<void>;
}) {
  return (
    <div className="gate">
      <b>
        {changes.length} widget file{changes.length === 1 ? "" : "s"} changed
      </b>
      <span className="files">
        {changes.map((change) => `${label(change.state)} ${change.file}`).join(" · ")}
      </span>
      <span className="grow" />
      <button className="chip undo" disabled={busy} onClick={() => void act("undo")}>
        undo
      </button>
      <button className="chip on" disabled={busy} onClick={() => void act("keep")}>
        keep
      </button>
    </div>
  );
}
