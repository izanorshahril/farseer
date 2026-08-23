# Spike: Win32 Job Object kill-on-close with a real harness child

Type: task
Status: closed
Blocked by: none

## Question

Throwaway spike. Prove or disprove the control-plane assumption everything else rests on.

Prove:

- Spawn a real Claude Code child (a `.cmd` shim, so via `cmd /c` or explicit resolution) inside a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
- Kill the supervisor. Confirm the whole tree dies, including node and git grandchildren, with no orphans.
- Confirm the same under ConPTY when a PTY view is attached.
- Measure how long handles actually take to release after the kill.

Record the answer as facts later tickets depend on: does it work, what survives, what the release latency is.

## Carried from 18

`18 How do comparable tools detect hangs and handle observation and takeover?` downgraded this from a **discovery** spike to a **confirmation** spike.
The primitive is known good, no surveyed tool uses it, and every herdr and firstmate Windows failure traced to an implementation choice rather than a platform wall.

Verify specifically:

- **One Win32 Job Object per worker process tree**, with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assigned at spawn.
- **Explicit `.cmd` and `.exe` resolution before `CreateProcess`**, never a bare command name. This is a named root cause of herdr's spawn failures.
- **`CREATE_NO_WINDOW`**, whose absence is another named root cause.
- That closing the job handle actually reaps a tree containing a shell, a node process, and a grandchild, which is the shape a real runner has.

Also confirm the negative case that makes Job Objects worth the effort: that killing the parent alone leaves the grandchild running.

Findings: [hang-detection-and-attach.md](../research/hang-detection-and-attach.md).

## Carried from 19

`windows-rs` v0.62.2 is confirmed working on `x86_64-pc-windows-msvc`.

Feature gates are finer than the module path suggests.
`CreateJobObjectW` lives in `Win32::System::JobObjects` but is gated behind **`Win32_Security`**, because its first parameter is a `SECURITY_ATTRIBUTES` pointer.
Enabling `Win32_System_JobObjects` alone produces a misleading "no `CreateJobObjectW` in this module" error.
Expected feature set: `Win32_Foundation`, `Win32_System_JobObjects`, `Win32_Security`, `Win32_System_Threading`.

## Resolution

Resolved 2026-08-22.
Spike code: [spikes/jobspike](../spikes/jobspike/README.md), `x86_64-pc-windows-msvc`, `windows-rs` v0.62.2, Windows 11 build 26200.8875.

The tree under test was five deep and rooted at a real `.cmd` shim:
`cmd.exe` -> `node.exe` (npm-cli) -> `cmd.exe` (npm script shell) -> `node.exe` (spawner) -> `node.exe` (grandchild).
That is the shape a real runner has, and it is the shape herdr fails on.

### Verdict

**Confirmed. The control-plane assumption holds, and it holds under ConPTY.**

| Mode | Tree | Survivors | Time to fully dead |
| --- | --- | --- | --- |
| `job` - job object, kill-on-close | 6 processes | **0** | 332us, 352us, 399us, 409us |
| `conpty` - same, behind a pseudoconsole | 5 processes | **0** | 298us, 322us, 347us |
| `naive` - no job, `TerminateProcess` on the root | 6 processes | **5**, all of them | never, still alive at 10s |

Release latency is **sub-millisecond**, roughly 300 to 400 microseconds for the whole five-deep tree.
There is no teardown budget to design around.
Any timeout farseer sets for reaping is a safety net against a pathological case, not a normal-path expectation.

### The negative case, confirmed hard

Killing the root alone left **five of six processes alive**, including all three `node.exe` processes and the intermediate `cmd.exe`.
Not one of them noticed its parent had died.
This is exactly the orphan storm `BRIEF.md` describes, reproduced deliberately in about ten seconds of runtime.

The Job Object is not an optimisation over careful parent-walking.
It is the difference between full reaping and none.

### Three implementation facts to carry into the runtime

**1. Suspended, assign, resume. In that order.**
`CREATE_SUSPENDED`, then `AssignProcessToJobObject`, then `ResumeThread`.
A process that runs even briefly before assignment can spawn a child outside the job, and that child is then unreapable.
Any other ordering is a race that fails rarely and unreproducibly, which is the worst failure mode farseer can ship.

**2. `.cmd` resolution has a second trap nobody warns about.**
`18` already named unqualified spawn as a herdr root cause.
The spike hit a further one.
`C:\Program Files\nodejs\` contains **both** `npm.cmd` and an extension-less `npm`, the latter being the POSIX sh script, which Windows cannot execute.
A `which`-style resolver that checks the bare name before walking `PATHEXT` finds the sh script, hands it to `CreateProcessW`, and fails.
The spike's first run did exactly this.

The rule: **walk `PATHEXT` candidates first, and only accept a bare filename if it already carries an executable extension.**

Then, since a `.cmd` is a batch script and not an image, `CreateProcessW` cannot run it at all.
It must go through the interpreter explicitly, and the quoting is not obvious:

```
"%ComSpec%" /s /c ""C:\Program Files\nodejs\npm.cmd" run spawner"
```

The `/s` plus doubled outer quotes form is required when the path contains spaces.
The obvious form without `/s` silently produces a process that starts and does nothing.

**3. `CREATE_NO_WINDOW` does not suppress `conhost.exe`.**
In `job` mode a `conhost.exe` still appeared in the tree.
It is reaped with everything else, so it is harmless, but the flag suppresses the *window*, not the console host process.
Anything that counts processes to decide whether a worker is healthy must expect it.

### ConPTY

`07` put the PTY in the runner adapter rather than the runtime, so the question here was narrower than originally written: does the reap still work when the adapter owns a pseudoconsole, and does ConPTY leak a console host that outlives the job.

Both answered.
Zero survivors, sub-millisecond, and **zero console hosts leaked** - measured by a global `conhost.exe` / `OpenConsole.exe` census diffed before spawn and after job close, since ConPTY's host is started by the system and is never a descendant of the spawned process.
`ClosePseudoConsole` afterwards also left nothing behind.

A pseudoconsole is not an escape hatch from the job.

### The finding that was not on the ticket

**Reconstructing a process tree from parent pids is unsafe on Windows, and the spike proved it the expensive way.**

Windows recycles pids aggressively.
An early version of the spike walked parent pids to census the tree, and a pid freed by a previous run had already been reused by an unrelated desktop application.
That application and its five child processes were admitted to the "tree" and terminated.

The fix is cheap: a descendant cannot predate its ancestor, so every candidate is checked against the root's creation time via `GetProcessTimes` before admission, and liveness is keyed on the `(pid, creation_time)` pair rather than the pid alone.

The design consequence is larger than the fix.
**Farseer must never identify a process by pid alone**, anywhere - not in the record, not in the run state, not in a kill path.
The identity is `(pid, creation_time)`, or better, job membership, which the kernel tracks and which cannot go stale.

This also sharpens why the Job Object is the right primitive and not merely a convenient one.
Job membership is a kernel fact.
A parent-pid chain is an **inference**, and an inference that goes wrong does not fail safe - it terminates the wrong process.
No amount of care makes pid-walking correct.

The spike no longer terminates anything it inferred; it reports survivors and prints a `Stop-Process` line for a human.

### Tickets this informs

- `05 worker control contract` - inherits `(pid, creation_time)` as the process identity, and the fact that reap latency is sub-millisecond so no teardown grace period is needed.
- `04 spike: workspace teardown` - the same pid-reuse hazard applies to any process-holding-a-file-lock check it performs.
