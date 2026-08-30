import { useCallback, useEffect, useState } from "react";
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
    return onProject(setSelected);
  }, [load]);

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

  return (
    <div className="projects">
      {roots.length === 0 && (
        <p className="empty">
          Farseer has no folder to work in yet. Add one below - everything it builds stays inside
          it, and nothing outside one is addressable.
        </p>
      )}

      <ul className="roots">
        {roots.map((root) => (
          <li key={root.path}>
            <div className="row">
              <b className="mono root-path" title={root.path}>
                {root.path}
              </b>
              {root.missing && (
                <span className="badge bad" title="the grant is still here; the directory is not">
                  not on disk
                </span>
              )}
              <span className="grow" />
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

            {!root.missing && root.projects.length === 0 && (
              <p className="empty small">Nothing in this folder yet.</p>
            )}

            {root.projects.length > FILTER_AT && (
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

            <ul className="project-list">
              {root.projects
                .filter(
                  (project) =>
                    root.projects.length <= FILTER_AT ||
                    project.name.toLowerCase().includes(filter.trim().toLowerCase()),
                )
                .map((project) => {
                const here = selected === project.path;
                return (
                  <li key={project.path}>
                    <button
                      className={here ? "project on" : "project"}
                      aria-pressed={here}
                      onClick={() => setProject(here ? null : project.path)}
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
          </li>
        ))}
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
