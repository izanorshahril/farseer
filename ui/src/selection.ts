/**
 * Which run the canvas is looking at.
 *
 * `28 operator surface` keeps widgets independent - each one takes a bridge and
 * nothing else, which is what lets an agent-authored widget mount beside the
 * built-in ones without either knowing about the other. A detail view breaks
 * that in exactly one place: it shows the run some *other* widget is pointing
 * at, so one fact has to cross the gap.
 *
 * One fact, in one module, with the same shape `stream.ts` already uses -
 * subscribe, notify, unsubscribe. Not React context, because that would require
 * every widget to be rendered inside a provider and would make the coupling
 * structural rather than optional: a widget that never imports this file cannot
 * be affected by it.
 *
 * **Deliberately not persisted.** `24 ui state persistence` stores an
 * arrangement - which widgets, in what order, how wide - because that is a
 * choice the operator made about their workspace. Which run they had open two
 * days ago is not; restoring it would reopen a finished run over whatever is
 * happening now.
 */
type Listener = (runId: string | null) => void;

let selected: string | null = null;
const listeners = new Set<Listener>();

export type SubjectSelection = {
  conversation: string | null;
  task: string | null;
  run: string | null;
  project: string | null;
  managerRunner: string | null;
};

type SubjectListener = (selection: SubjectSelection) => void;

let subject: SubjectSelection = {
  conversation: null,
  task: null,
  run: null,
  project: null,
  managerRunner: null,
};
const subjectListeners = new Set<SubjectListener>();

/** Shared first-party subject context from `40 work model and session explorer`. */
export function selectedSubject(): SubjectSelection {
  return subject;
}

export function selectSubject(change: Partial<SubjectSelection>): void {
  subject = { ...subject, ...change };
  selected = subject.run;
  for (const listener of [...listeners]) listener(selected);
  for (const listener of [...subjectListeners]) listener(subject);
}

export function onSubjectSelection(listener: SubjectListener): () => void {
  subjectListeners.add(listener);
  return () => subjectListeners.delete(listener);
}

/** The run currently selected, or `null` when none is. */
export function selectedRun(): string | null {
  return selected;
}

/** Point the canvas at a run. Passing the selected run again clears it. */
export function selectRun(runId: string | null): void {
  const next = selected === runId ? null : runId;
  selectSubject({ run: next });
}

/** Listen for changes. Returns the unsubscribe. */
export function onSelection(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
