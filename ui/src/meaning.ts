/**
 * The words this product invented, in one place, so the canvas teaches them
 * everywhere they appear.
 *
 * They started in the Run widget, which is where `28 operator surface`'s review
 * found the vocabulary was explained only in source comments. The dictionary
 * then stayed there while `ceiling`, `roster` and `worktree` went on being used
 * in Delegation and Projects with nothing to hover - so the fix was half done,
 * in the half that happened to be reviewed.
 *
 * A term earns a line here when farseer means something by it that a reader
 * cannot get from the English word: `run` versus `task`, a `ceiling` that is a
 * policy rather than a limit, a `root` that is a grant rather than a directory.
 */
export const MEANING: Record<string, string> = {
  cell: "The definition this run belongs to - its roster, policy and manager, as a file in `cells/`",
  role: "manager: a conversation you can steer. worker: a job a manager handed off",
  runner: "The binary that actually ran, from farseer's own inventory",
  state: "Where the run is, or how it ended",
  took: "Wall time from queued to finished",
  cost: "What the runner said it spent - not every runner says",
  tokens: "What the runner said it used - not every runner says",
  "tool level": "How much of the runner's own tool set this run got: read, edit or shell",
  ceiling: "The most irreversible action allowed without asking a person",
  "tool grants":
    "Cell-level capabilities the roster named. Recorded, and reaching them is not built yet",
  skills:
    "Instruction directories handed to the runner by path, never discovered from your home directory",
  budget: "The ceiling on what this run may spend before farseer stops it",
  "done when":
    "The cell's own definition of finished, checked by the cell's tools rather than by farseer",
  roster: "Who a manager may hand work to: named workers, named cells, and declared tools",
  worker: "A job a manager handed off, under its own contract and its own share of the budget",
  worktree:
    "A private git checkout cut for one run, so its writes are reversible until something is pushed",
  root: "A folder you have authorized farseer to work inside. Farseer never adds one itself",
  project: "A folder inside a root. Never registered, so this list is whatever is on disk",
};

/**
 * The tooltip for a term, or `undefined` when farseer has nothing extra to say.
 *
 * A function rather than a bare index so a label with no entry is a deliberate
 * `undefined` at one site instead of a silently empty `title` at every one.
 */
export function meaningOf(label: string): string | undefined {
  return MEANING[label];
}
