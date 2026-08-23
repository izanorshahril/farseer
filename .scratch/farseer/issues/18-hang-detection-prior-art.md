# How do comparable tools detect hangs and handle observation and takeover?

Type: research
Status: closed
Blocked by: none

## Question

Raised by `07 Attach: to a worker or to a run, and what does intervention do?`.
That ticket decided the shape of attach, takeover and auto-release.
It did not decide the numbers or the detection mechanism, and the operator has not yet checked prior art.

Farseer exists to fix a specific failure list in `BRIEF.md`: hangs, workers that cannot be spawned, permission failures, orphaned processes on Windows.
The operator suspects most of these come from herdr's behaviour on Windows rather than from anything fundamental, and that suspicion should be confirmed or killed rather than carried into the spec.

Survey how comparable tools do this, and report what farseer should copy:

- **Hang detection.** What actually counts as hung: no output for N seconds, no tool call for N seconds, no token for N seconds, a process alive but idle, or a heartbeat the agent must emit. What thresholds do real tools use, and are they fixed or adaptive.
- **Auto-release and orphan reaping.** When a client disconnects mid-takeover, or the supervisor dies, what happens to the worker. Timeouts, leases, keepalives, or nothing.
- **Observation surfaces.** How Traycer, and other orchestrators worth naming, present a live worker to a human: rendered event stream, terminal mirror, diff view, log tail. Note specifically whether any of them can replay a dead session through the same surface, which is what `07` decided farseer must do.
- **Takeover.** Which tools let a human seize control of a live agent session at all, and what they do to the supervising agent's plan afterwards.
- **Windows specifics.** Confirm or kill the operator's suspicion that the `BRIEF.md` failure list is herdr-specific rather than inherent to Windows. Relevant: job objects, ConPTY, the absence of process groups, and how other Windows-native tools reap process trees.

Deliverable: findings file plus a short list of concrete numbers and mechanisms farseer should adopt, and any it should deliberately reject.

Do not fire this until the operator asks, per the map's Notes.

## Resolution

Resolved 2026-08-19 by research subagent.
Findings: [hang-detection-and-attach.md](../research/hang-detection-and-attach.md).

### Verdict on the Windows hypothesis

**The operator's hypothesis is confirmed for blame and killed for framing.**

Windows genuinely lacks POSIX process groups and signals, and that gap is inherent.
But every concrete herdr, firstmate and buzz Windows bug the survey found has a stated root cause that is an **implementation choice**, not a platform wall:

- Unqualified `Start-Process` spawn.
- A `kill -0` liveness check with no Windows fallback, despite the fix existing elsewhere in the same codebase.
- Missing `CREATE_NO_WINDOW`.
- An admittedly never-built Job Object story.

Not one bug traced to an unsolvable platform limit.

A herdr maintainer directly confirmed Windows attach is unsupported because the input path is "Unix raw-byte" and Windows "needs a semantic input or event path", explicitly calling it unbuilt feature work.
That is independent confirmation of `07 Attach: to a worker or to a run, and what does intervention do?`, which chose a structured event stream over a PTY.

Consequence for the map: the `BRIEF.md` failure catalogue is farseer's opportunity rather than its risk.
The scary platform questions are smaller than assumed, and `03 spike: job objects` is now expected to confirm a known-good primitive rather than discover whether one exists.

### Hang detection, recommended

**No-progress-event watchdog**, tracking the last structured event per worker: tool call start, tool result, status change.
Two fixed, configurable thresholds:

- **120 seconds** with no progress event: mark the run `stalled`.
- **600 seconds** with no progress event: flag the run `likely-hung`.

**No auto-kill at either threshold.**
Cancel remains a separate verb, per `07`.

Calibrated against Kubernetes probe defaults and multica's real 10-minute per-runtime idle watchdogs.
Note that this watches *progress events*, not stdout, which is what makes it work for a rendered event stream with no PTY.

### Windows orphan reaping, recommended

One **Win32 Job Object per worker process tree**, with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assigned at spawn.
Plus explicit `.cmd` and `.exe` resolution before `CreateProcess`, never a bare command name.

**Zero surveyed Windows agent tool was confirmed to use Job Objects at all.**
This is both the correct primitive and an unoccupied position.

### Three findings worth carrying forward

1. **Orca and Traycer, the two most mature tools surveyed, both have real hang-watchdog code that only watches their own Electron host process, not the agent worker.** Both have an open, named gap between detecting their own infrastructure hanging and detecting the agent hanging. Farseer watching the worker is a genuine differentiator, not table stakes.
2. **Buzz, with over 28k stars, has an open unresolved maintainer issue admitting Job Objects are not yet adopted for its managed-agent process trees.** That is the exact primitive `BRIEF.md` already names as correct.
3. **Herdr's Windows attach gap is unbuilt feature work, not a platform limitation**, per a maintainer comment. See the verdict section above.

### Tickets this informs

- `03 spike: job objects` - now a confirmation spike, and should specifically verify `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` plus explicit executable resolution.
- `05 worker control contract` - inherits the 120 second and 600 second thresholds and the `stalled` and `likely-hung` states.
- `16 What is the local API surface?` - inherits the run states, which the event stream must expose.

## Superseded in part by 05

The recommendation above defined the watchdog input as **progress events**: tool call start, tool result, status change.
`05 Run state model and control semantics` found that wrong.

A high-end model reasoning for twenty minutes emits no progress events and would have been flagged `likely-hung` while working perfectly.
The watchdog now keys on **activity** - any bytes from the adapter, including token streams - while progress events keep their role in the record and the fleet view.

The two numbers survive unchanged. The signal they measure does not.
Everything else in this resolution stands.
