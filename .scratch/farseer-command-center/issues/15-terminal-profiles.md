# 15: Terminal profiles

**Parent:** [Command-center decision map](../map.md).

**What to build:** Add optional terminal profiles for Git Bash, PowerShell, cmd, and other approved shells as new sessions associated with a project or focused run workspace.

**Blocked by:** [Independent runtime lifecycle](02-independent-runtime.md), [Focused workspace](10-focused-workspace.md)

**Status:** complete.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R10; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** Each shell selection creates a new terminal session with explicit cwd and environment.
Terminal output is an attachment or UI stream, never canonical run truth.
ConPTY is optional and isolated from the core runner control channel.
The adapter owns the process tree, bounded retained output of at most 10,000 lines and 1 MiB, resize/input, and reconnect.
Closing a terminal view detaches; explicit End session terminates the owned process and releases its workspace lease.
An active run workspace cannot be torn down while an authorized terminal lease holds it; the UI must offer End session or open a separate project terminal when cleanup is pending.

- [x] The operator can open Git Bash, PowerShell, and cmd profiles with an explicit project or run workspace through the terminal API.
- [x] Switching shell profiles creates a new session and never mutates an existing process or run contract.
- [x] Resize, input, bounded scrollback, cwd/profile/owner, and termination state work through the adapter, including spaces and non-ASCII paths.
- [x] Closing a view/desktop preserves the terminal for reconnect; explicit End session reaps it, releases its workspace lease, and lets deferred workspace cleanup complete.
- [x] A profile with unavailable executable, invalid workspace, or denied authority fails before process creation.

**Exclusions:** No PTY in the core manager contract, shell command approval gateway, remote shell, WSL requirement, or implicit shell substitution.

**Test seam/demo:** Open each supported shell in the selected project, run a harmless identity command, close and reopen the desktop, and verify independent lifecycle.

## Evidence

`crates/farseer-runner/src/terminal.rs` resolves the three named profiles before spawn, validates dimensions and cwd, supervises each process through the existing Job Object, and bounds scrollback to 10,000 lines and 1 MiB.
`TerminalManager` records a workspace lease for every live session, defers run teardown while a session owns that cwd, and completes the deferred delete after explicit End; the Unicode terminal test covers the lease and cleanup ordering.
Deferred cleanup now attempts every ready workspace before returning the first teardown error, so one blocked path cannot strand other pending cleanups without an explicit retry.
`crates/farseer-api/src/terminals.rs` exposes profile discovery, open, reconnect, input, resize, and explicit end routes with authorized project or active-run workspace checks.
The runtime owns the in-memory session manager, so closing the desktop window leaves sessions available while the runtime remains alive.
The first slice intentionally retains resize state at the adapter seam and does not claim ConPTY or durable recovery after a runtime process restart.
The terminal acceptance is complete within that contract: profile resolution, bounded scrollback, resize/input, reconnect, explicit End, deferred workspace cleanup, Unicode paths, and pre-spawn authority failures are covered by the runner and API seams.
ConPTY and recovery after a runtime restart remain explicitly optional follow-up work rather than unfinished core behavior.
