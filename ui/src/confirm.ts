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
