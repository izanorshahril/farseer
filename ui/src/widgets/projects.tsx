import { useCallback, useEffect, useState } from "react";
import type { Bridge } from "../bridge";
import { currentProject, onProject, setProject } from "../project";

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
              <button
                className="chip danger"
                disabled={busy}
                onClick={() => {
                  // Named, and named as a grant: the whole risk of this button
                  // is that it reads as a delete of the operator's work.
                  if (
                    confirm(
                      `Stop farseer working in ${root.path}?\n\nThe folder and everything in it stays exactly where it is.`,
                    )
                  ) {
                    void act(() => bridge.del("/projects/roots", { path: root.path }));
                  }
                }}
              >
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

            <ul className="project-list">
              {root.projects.map((project) => {
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
        ) : (
          <>No project picked - work goes to the folder farseer itself was started in.</>
        )}
      </p>
      {note && <p className="dim small bad">{note}</p>}
    </div>
  );
}
