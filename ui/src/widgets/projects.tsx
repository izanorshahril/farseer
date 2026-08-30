import { useCallback, useEffect, useState, type KeyboardEvent } from "react";
import type { Bridge } from "../bridge";
import { currentProject, onProject, setProject } from "../project";
import { confirmGrantWithdrawal } from "../confirm";
import { meaningOf } from "../meaning";

/**
 * The folders farseer may work in, and the projects inside them.
 *
 * `39 what an installed farseer points at` settled the shape this widget makes
 * visible: farseer is an application that **points at** projects rather than a
 * tool run inside one. A **root** is a directory the operator authorized - the
 * unit of trust - and a **project** is a directory inside a root, never
 * registered, so this list cannot drift from a disk the operator also edits in
 * Explorer.
 *
 * Two refusals are on the surface rather than in the runtime alone:
 *
 * - Farseer creates projects and **never creates a root**. Authorizing is the
 *   operator's act; an application that can widen its own authorization has
 *   none. So the root field takes a path the operator already has, and there is
 *   no browse-and-create.
 * - Removing a root removes the **grant**, not the directory. The button says
 *   so, because "remove" beside a folder path reads like a delete.
 */
type Project = { name: string; path: string; git: boolean };
type Root = { path: string; missing: boolean; projects: Project[] };

/**
 * Past this many projects under one root, the list gets a filter.
 *
 * A filter over six entries is a control asking to be ignored; over two dozen -
 * which this widget's own width exists for - a flat list is something the
 * operator scans instead of queries.
 */
const FILTER_AT = 12;

/**
 * Whether a project path sits under a root, by path segment.
 *
 * The same comparison the runtime makes, and the same reason for making it this
 * way rather than with a string prefix: `D:\Dev` does not contain
 * `D:\Development`. The runtime canonicalizes first; here both strings came
 * from the runtime already canonical.
 */
function isInside(root: string, project: string): boolean {
  if (project === root) return true;
  if (!project.startsWith(root)) return false;
  const tail = project.slice(root.length);
  return tail.startsWith("\\") || tail.startsWith("/");
}

/**
 * How the operator arranged this widget. Stored through the same blob the
 * canvas uses, per `24 ui state persistence`: an arrangement they chose, kept
 * so reopening the console does not undo it.
 *
 * `collapsed` folds a root's project list and **never hides the root**. A grant
 * the operator cannot see is a grant they cannot withdraw, so the one thing
 * this control must not do is make authorization disappear from the screen.
 */
type Arrangement = { order: string[]; collapsed: string[] };

export function ProjectsWidget({ bridge }: { bridge: Bridge }) {
  const [roots, setRoots] = useState<Root[] | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [rootPath, setRootPath] = useState("");
  /** Which root the new-project field is open under, or `null` for none. */
  const [adding, setAdding] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [newGit, setNewGit] = useState(true);
  const [selected, setSelected] = useState(currentProject());
  /** Narrows the chip lists. See [`FILTER_AT`]. */
  const [filter, setFilter] = useState("");
  const [arrangement, setArrangement] = useState<Arrangement>({ order: [], collapsed: [] });
  /**
   * Which chip in each root's list holds the tab stop.
   *
   * A root with two dozen projects is two dozen tab stops, which is a keyboard
   * operator tabbing past the whole widget to reach the field below it. One
   * stop per list, arrows inside - the roving pattern every toolbar uses.
   */
  const [cursor, setCursor] = useState<Record<string, number>>({});

  const load = useCallback(
    () =>
      bridge
        .read<Root[]>("/projects")
        .then(setRoots)
        .catch((e: Error) => setNote(e.message)),
    [bridge],
  );

  useEffect(() => {
    void load();
    bridge
      .loadState<Arrangement>("projects")
      .then((stored) =>
        setArrangement({ order: stored?.order ?? [], collapsed: stored?.collapsed ?? [] }),
      )
      // No stored arrangement is the resting state: the roots render in the
      // order the runtime returned them.
      .catch(() => {});
    return onProject(setSelected);
  }, [bridge, load]);

  const arrange = (change: (current: Arrangement) => Arrangement) =>
    setArrangement((current) => {
      const next = change(current);
      // Fire and forget, like the project choice itself: a failed write costs
      // one re-drag after a restart, and blocking the gesture on a round trip
      // costs the gesture.
      bridge.saveState("projects", next).catch(() => {});
      return next;
    });

  /**
   * Move a root one place.
   *
   * Written back as the **whole** displayed order rather than as a pair of
   * swapped entries, so a list holding roots the operator never arranged comes
   * out of one drag fully ordered instead of half.
   */
  const move = (paths: string[], path: string, by: number) => {
    const at = paths.indexOf(path);
    const to = at + by;
    if (at < 0 || to < 0 || to >= paths.length) return;
    const order = [...paths];
    order.splice(to, 0, ...order.splice(at, 1));
    arrange((current) => ({ ...current, order }));
  };

  /**
   * Arrows move within a root's chips; the list itself is one tab stop.
   *
   * Focus is moved on the DOM rather than through state because the chips are
   * already rendered and a React round trip would move the tab stop a frame
   * after the key, which reads as a dropped keystroke.
   */
  const roam = (event: KeyboardEvent<HTMLUListElement>, root: string) => {
    const keys = ["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp", "Home", "End"];
    if (!keys.includes(event.key)) return;
    const chips = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button.project")];
    if (chips.length === 0) return;
    const from = chips.findIndex((chip) => chip === document.activeElement);
    const forward = event.key === "ArrowRight" || event.key === "ArrowDown";
    const at =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? chips.length - 1
          : Math.min(chips.length - 1, Math.max(0, (from < 0 ? 0 : from) + (forward ? 1 : -1)));
    event.preventDefault();
    chips[at]?.focus();
    setCursor((current) => ({ ...current, [root]: at }));
  };

  /**
   * Withdrawing a root clears a project selected **inside** it.
   *
   * Without this the grant goes and the composer keeps saying where work is
   * headed, so the refusal arrives one instruction later, detached from the
   * click that caused it - the worst shape an error can have for the operator
   * this console is built for.
   */
  const withdraw = (root: Root) => {
    const losing = selected && isInside(root.path, selected) ? selected : null;
    if (!confirmGrantWithdrawal(root.path, losing)) return;
    if (losing) setProject(null);
    // A withdrawn root leaves nothing behind in the arrangement either, so
    // re-authorizing it later starts where a new root starts rather than in a
    // position and a fold the operator has forgotten choosing.
    arrange((current) => ({
      order: current.order.filter((path) => path !== root.path),
      collapsed: current.collapsed.filter((path) => path !== root.path),
    }));
    void act(() => bridge.del("/projects/roots", { path: root.path }));
  };

  const act = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setNote(null);
    try {
      await work();
      await load();
    } catch (e) {
      setNote((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  if (!roots && note) return <p className="empty bad">{note}</p>;
  if (!roots) return <p className="empty">reading folders...</p>;

  // A root the operator never arranged sorts after every one they did, and
  // keeps the runtime's order among its own kind - `Array.sort` is stable, so
  // an equal rank leaves the list alone rather than reshuffling it.
  const rank = (path: string) => {
    const at = arrangement.order.indexOf(path);
    return at < 0 ? arrangement.order.length : at;
  };
  const ordered = [...roots].sort((a, b) => rank(a.path) - rank(b.path));
  const paths = ordered.map((root) => root.path);

  return (
    <div className="projects">
      {roots.length === 0 && (
        <p className="empty">
          Farseer has no folder to work in yet. Add one below - everything it builds stays inside
          it, and nothing outside one is addressable.
        </p>
      )}

      <ul className="roots">
        {ordered.map((root, place) => {
          const folded = arrangement.collapsed.includes(root.path);
          const shown = root.projects.filter(
            (project) =>
              root.projects.length <= FILTER_AT ||
              project.name.toLowerCase().includes(filter.trim().toLowerCase()),
          );
          // The tab stop sits on the project already chosen when there is one,
          // so arriving at the list starts where the operator left off rather
          // than at the alphabetical first.
          const stop = Math.max(
            0,
            cursor[root.path] ?? shown.findIndex((project) => project.path === selected),
          );
          return (
          <li key={root.path}>
            <div className="row">
              {/* Folding a root, never hiding it: see [`Arrangement`]. */}
              <button
                className="chip fold"
                aria-expanded={!folded}
                title={folded ? "show what is in this folder" : "fold this folder away"}
                onClick={() =>
                  arrange((current) => ({
                    ...current,
                    collapsed: folded
                      ? current.collapsed.filter((path) => path !== root.path)
                      : [...current.collapsed, root.path],
                  }))
                }
              >
                {folded ? "▸" : "▾"}
              </button>
              <b className="mono root-path" title={root.path}>
                {root.path}
              </b>
              {folded && root.projects.length > 0 && (
                <span className="dim small">
                  {root.projects.length} project{root.projects.length === 1 ? "" : "s"}
                </span>
              )}
              {root.missing && (
                <span className="badge bad" title="the grant is still here; the directory is not">
                  not on disk
                </span>
              )}
              <span className="grow" />
              {/* Buttons rather than a drag handle. The canvas above can afford
                  a grip because a widget is a big target; a root is one line,
                  and one line that only moves by dragging is one line a
                  keyboard cannot move at all. */}
              <button
                className="chip move"
                disabled={place === 0}
                aria-label={`move ${root.path} up`}
                title="move this folder up"
                onClick={() => move(paths, root.path, -1)}
              >
                ▲
              </button>
              <button
                className="chip move"
                disabled={place === paths.length - 1}
                aria-label={`move ${root.path} down`}
                title="move this folder down"
                onClick={() => move(paths, root.path, 1)}
              >
                ▼
              </button>
              <button
                className="chip"
                disabled={busy || root.missing}
                onClick={() => {
                  setAdding(adding === root.path ? null : root.path);
                  setNewName("");
                }}
                title="create a new project folder inside this one"
              >
                new project
              </button>
              {/* Named as a grant, through `confirm.ts` rather than an inline
                  dialog here, so the two dangerous actions on this canvas
                  cannot come to disagree about wording a second time. */}
              <button className="chip danger" disabled={busy} onClick={() => withdraw(root)}>
                withdraw
              </button>
            </div>

            {adding === root.path && (
              <div className="row new-project">
                <input
                  aria-label="new project name"
                  placeholder="folder name"
                  value={newName}
                  disabled={busy}
                  onChange={(e) => setNewName(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && newName.trim()) {
                      void act(() =>
                        bridge.post("/projects", {
                          root: root.path,
                          name: newName.trim(),
                          git: newGit,
                        }),
                      ).then(() => setAdding(null));
                    }
                  }}
                />
                <label className="dim small">
                  <input
                    type="checkbox"
                    checked={newGit}
                    disabled={busy}
                    onChange={(e) => setNewGit(e.currentTarget.checked)}
                  />{" "}
                  git init
                </label>
                <button
                  className="chip"
                  disabled={busy || !newName.trim()}
                  onClick={() =>
                    void act(() =>
                      bridge.post("/projects", {
                        root: root.path,
                        name: newName.trim(),
                        git: newGit,
                      }),
                    ).then(() => setAdding(null))
                  }
                >
                  create
                </button>
              </div>
            )}

            {!folded && !root.missing && root.projects.length === 0 && (
              <p className="empty small">Nothing in this folder yet.</p>
            )}

            {!folded && root.projects.length > FILTER_AT && (
              <div className="row project-filter">
                <input
                  aria-label={`filter projects in ${root.path}`}
                  placeholder={`filter ${root.projects.length} projects`}
                  value={filter}
                  onChange={(e) => setFilter(e.currentTarget.value)}
                />
                {filter && (
                  <button className="chip" onClick={() => setFilter("")}>
                    clear
                  </button>
                )}
              </div>
            )}

            {!folded && (
            <ul
              className="project-list"
              // A list of chips the arrows walk through: see [`roam`].
              onKeyDown={(event) => roam(event, root.path)}
            >
              {shown.map((project, index) => {
                const on = selected === project.path;
                return (
                  <li key={project.path}>
                    <button
                      className={on ? "project on" : "project"}
                      aria-pressed={on}
                      tabIndex={index === Math.min(stop, shown.length - 1) ? 0 : -1}
                      onFocus={() => setCursor((current) => ({ ...current, [root.path]: index }))}
                      onClick={() => setProject(on ? null : project.path)}
                      title={project.path}
                    >
                      <span className="project-name">{project.name}</span>
                      {/* Reported, not filtered. A `worktree` cell needs a
                          repository, and a project farseer hid because it has
                          none is worse than one shown with the reason. */}
                      {!project.git && (
                        <span className="dim small" title="a worktree cell needs a git repository">
                          no git
                        </span>
                      )}
                    </button>
                  </li>
                );
              })}
            </ul>
            )}
          </li>
          );
        })}
      </ul>

      <div className="row add-root">
        <span className="dim small" title={meaningOf("root")}>
          root
        </span>
        <input
          aria-label="folder to authorize"
          placeholder={String.raw`a folder farseer may work in, e.g. D:\Dev`}
          value={rootPath}
          disabled={busy}
          onChange={(e) => setRootPath(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && rootPath.trim()) {
              void act(() => bridge.post("/projects/roots", { path: rootPath.trim() })).then(() =>
                setRootPath(""),
              );
            }
          }}
        />
        <button
          className="chip"
          disabled={busy || !rootPath.trim()}
          onClick={() =>
            void act(() => bridge.post("/projects/roots", { path: rootPath.trim() })).then(() =>
              setRootPath(""),
            )
          }
        >
          authorize
        </button>
      </div>

      <p className="dim small">
        {selected ? (
          <>
            Work goes to <span className="mono">{selected}</span>.
          </>
        ) : roots.length > 0 ? (
          // Said here rather than only in the footer: an operator who has just
          // authorized a folder and picked nothing is one click from the state
          // they wanted, and the sentence that names the click belongs next to
          // the chips it is about.
          <>No project picked - pick one above, or work goes to the folder farseer was started in.</>
        ) : (
          <>No project picked - work goes to the folder farseer itself was started in.</>
        )}
      </p>
      {note && <p className="dim small bad">{note}</p>}
    </div>
  );
}
