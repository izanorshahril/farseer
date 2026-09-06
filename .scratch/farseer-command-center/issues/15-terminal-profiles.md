# 15: Terminal profiles

**Parent:** [Command-center decision map](../map.md).

**What to build:** Add optional terminal profiles for Git Bash, PowerShell, cmd, and other approved shells as new sessions associated with a project or focused run workspace.

**Blocked by:** [Independent runtime lifecycle](02-independent-runtime.md), [Focused workspace](10-focused-workspace.md)

**Status:** not-started.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R10; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** Each shell selection creates a new terminal session with explicit cwd and environment.
Terminal output is an attachment or UI stream, never canonical run truth.
ConPTY is optional and isolated from the core runner control channel.
The adapter owns the process tree, bounded retained output of at most 10,000 lines and 1 MiB, resize/input, and reconnect.
Closing a terminal view detaches; explicit End session terminates the owned process and releases its workspace lease.
An active run workspace cannot be torn down while an authorized terminal lease holds it; the UI must offer End session or open a separate project terminal when cleanup is pending.

- [ ] The operator can open Git Bash, PowerShell, and cmd profiles with an explicit project or run workspace.
- [ ] Switching shell profiles creates a new session and never mutates an existing process or run contract.
- [ ] Resize, input, bounded scrollback, cwd/profile/owner, and termination state work through the optional view, including spaces and non-ASCII paths.
- [ ] Closing a view/desktop preserves the terminal for reconnect; explicit End session reaps it, releases its workspace lease, and lets deferred workspace cleanup complete.
- [ ] A profile with unavailable executable, invalid workspace, or denied authority fails before process creation.

**Exclusions:** No PTY in the core manager contract, shell command approval gateway, remote shell, WSL requirement, or implicit shell substitution.

**Test seam/demo:** Open each supported shell in the selected project, run a harmless identity command, close and reopen the desktop, and verify independent lifecycle.
