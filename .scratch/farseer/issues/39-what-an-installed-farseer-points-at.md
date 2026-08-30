# 39 what an installed farseer points at

**Status:** open 2026-08-30.
**Found:** 2026-08-30, packaging the desktop shell. Simulating an installed layout - the executable, its canvas and its daemon in a directory that is not the repository - produced an application that opened, rendered, and had **zero cells**.

## The finding

Every path the shell used was relative to the working directory, which is the repository when farseer is run from one and the **installation directory** when it is launched from a Start Menu shortcut. So an installed farseer:

- loaded no cell definitions, and therefore had no top manager to talk to;
- served no canvas, because `ui/dist` is a repository path;
- started no daemon, because the sidecar was never bundled.

The first two are fixed and the third is a bundle entry. What is **not** fixed is the question underneath them, and it is a design question rather than a path.

## The question

`zero.toml` declares `workspace_strategy = "worktree"`. A worktree needs a git repository, and `repo_root()` is the working directory. So cell zero - farseer's own builder harness, and the operator's default address - **cannot run in an installed application at all** unless the operator launched it from inside a repository.

That is not a bug in the shortcut. It is the shape of the product being undecided:

1. **farseer is a tool you run inside a project.** The working directory is the project, the record is per-project, and an installer is a convenience for putting the binary on `PATH`. Then a Start Menu shortcut is close to meaningless and should probably not exist.
2. **farseer is an application you open, and it points at projects.** Then a project is a thing the operator picks in the UI, `repo_root` is per-cell or per-run rather than per-process, and the record is machine-wide - which it already is, at `%LOCALAPPDATA%\farseer\record.sqlite3`.

The record already assumes (2). Everything else assumes (1). They have not been reconciled because nothing forced the question until there was an installer.

## What was built anyway

Only the mechanical part, and deliberately no policy:

- The shell looks for `cells/` in the working directory, then beside the executable, then in its own data directory. **Nothing is seeded and nothing is copied** - `01 cell primitive` makes a definition a plain file the operator edits, and a directory farseer filled with cells nobody wrote is exactly the opinion `13 harness build kit` warns against.
- When it finds none, the canvas names the three places it looked rather than rendering an empty fleet as though that were a state somebody chose.
- `cells/`, `runners.toml` and the built canvas are bundle resources, so the pieces are present however the question resolves.

## Not decided here

Whether an installed farseer should offer to open a project directory, and what `repo_root` means when it does. That is `28 operator surface`'s territory and wants a person.

---

## Resolution, 2026-08-30: farseer points at projects, and holds a list of where it may look

The operator settled the fork in the second direction, and added the part neither option contained:

> farseer not run in a project, it manage multiple projects, user add folders of projects authorized for farseer, farseer can create new project inside the allowed folders.

So option 2, with an **authorization boundary that is the operator's, not the filesystem's**. That last clause is what makes this buildable rather than a rename: option 2 on its own leaves "which directory" answered by whoever types a path, and a run's workspace strategy is `git worktree add` under that directory. A path that arrives from a manager, a widget, or a URL query and is used as a repository root is a write primitive with no fence around it.

### The model

Two nouns, and only the first is authorized.

- A **root** is a directory the operator named. It is the unit of trust. `D:\Dev` is one root; nothing outside a root is addressable.
- A **project** is a directory inside a root. It is not authorized separately, is not registered, and can appear because the operator made it in Explorer. Farseer lists what is there rather than keeping a second list that drifts from the disk.

Farseer may create a project inside a root, and may not create a root. Creating a root is the act of granting access, and an application that can widen its own authorization has none.

### Where the list lives

The record, at `%LOCALAPPDATA%\farseer\record.sqlite3`, in its own table - **not** a file beside `cells/`.

`16 local api surface` refuses an edit path for cell definitions and that refusal is not in tension with this one: a definition is a thing the operator authors in their editor, and farseer writing it would put farseer and the editor in a merge. A root list is not authored, it is *granted*, and a grant made by clicking "add folder" has no editor to conflict with. `39`'s own finding is the second argument: the record is already machine-wide while everything else was per-checkout, and this is the fact that made it so.

### What `repo_root` becomes

Per run, not per process. The process-wide `repo_root` stays as the fallback for `farseer` launched inside a repository - the CLI keeps working, and every existing test keeps its meaning - but a launch may name a project, and then that project is the repository the worktree is cut from.

A named project is checked against the roots **after canonicalization**, so `..`, a symlink out, and a UNC path all resolve before the comparison rather than after it. A check on the string the caller sent is not a check.

### zero.toml

Cell zero declares `workspace_strategy = "worktree"` and is farseer's own builder harness, so its project is the farseer checkout. It is now a cell like any other in this respect too: it runs against whichever project the run names, and against the process's own repository when a run names none. Nothing is seeded - `13 harness build kit`'s ruling holds, and an installed farseer with no roots shows an empty list that says what a root is, not a directory farseer picked.
