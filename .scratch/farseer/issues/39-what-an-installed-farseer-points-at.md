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
