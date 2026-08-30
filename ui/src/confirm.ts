/**
 * Ask before doing something that cannot be taken back.
 *
 * **Cancelling a run had no confirmation and discarding a git diff did.** The
 * canvas asked "Discard N change(s) under widgets/?" before an undo, and killed
 * a live agent mid-task on a single click - the more consequential action, with
 * less friction, in the same application. On a dense list of twenty-five runs,
 * at the hour this console is actually read, that is a misclick with in-flight
 * work and spent quota behind it and no path back.
 *
 * One helper rather than three `confirm()` calls, so the three call sites
 * cannot come to disagree about which verbs are dangerous - which is how the
 * inconsistency started.
 *
 * `05 run state model` is why `cancel` is the only run verb here: `steer` moves
 * within a contract, `rerun` and `rescope` **start** something, and only cancel
 * ends work that is happening.
 */

/** Verbs that end something. Everything else runs on one click, as it should. */
const DESTRUCTIVE = new Set(["cancel"]);

/**
 * `true` when the caller should go ahead.
 *
 * Names what is about to happen and what it is about to happen to, because
 * "Are you sure?" is a question nobody can answer - the whole risk is having
 * clicked the wrong row, and a dialog that does not name the row cannot catch
 * that.
 */
export function confirmVerb(verb: string, subject: string): boolean {
  if (!DESTRUCTIVE.has(verb)) return true;
  return confirm(`Cancel ${subject}?\n\nThe run stops where it is. What it has already done stays done.`);
}

/**
 * Withdrawing farseer's access to a folder.
 *
 * Here rather than inline at the call site for the reason this file already
 * states: the first thing that happened after `confirmVerb` centralized run
 * verbs was a **second** bespoke `confirm()` growing next to a different
 * dangerous action, with its own wording. That is the drift this module exists
 * to stop, so a grant gets a function here rather than a string over there.
 *
 * `losing` names the project the operator is currently pointed at when it is
 * inside this root, because the aftermath is the part that bites: the grant
 * goes, the composer keeps saying where work is headed, and the failure lands
 * one instruction later, decoupled from the click that caused it.
 */
export function confirmGrantWithdrawal(root: string, losing: string | null): boolean {
  const aftermath = losing
    ? `

Work is currently pointed at ${losing}, which is inside it. That will be cleared, and instructions go back to the folder farseer itself was started in.`
    : "";
  return confirm(
    `Stop farseer working in ${root}?

The folder and everything in it stays exactly where it is.${aftermath}`,
  );
}
