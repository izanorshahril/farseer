# 02: Independent runtime lifecycle

**Parent:** [Command-center decision map](../map.md).

**What to build:** Let the desktop shell close without killing the farseer runtime, while giving the operator separate drain and force controls for intentional shutdown.

**Blocked by:** [Verified startup](01-verified-startup.md)

**Status:** not-started.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R01; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/01-runtime.md).

**Contract choices:** Desktop close leaves the daemon running.
Drain waits for accepted work to reach a safe terminal point.
The initial 30-second deadline reports remaining runs and leaves the daemon alive and draining; it never forces cancellation.
The 30-second drain expiry keeps the runtime alive and still draining; it never auto-cancels.
Force is a separate explicit action, records its reason, and uses existing Job Object cancellation semantics.

- [ ] Closing the desktop window leaves the runtime reachable and active runs unchanged.
- [ ] Reopening the shell reconnects to the same authenticated runtime and shows current runs and events without duplicating stream subscriptions.
- [ ] Drain refuses new work, reports pending runs, and exits after they finish; a 30-second expiry remains in draining until force is selected.
- [ ] Force presents the affected run count, records operator intent, and cancels process trees through the existing runtime control path.
- [ ] A runtime started without the shell remains usable through the local API and CLI.

**Exclusions:** No Windows service, machine-wide background process, cloud daemon, or automatic force timeout.

**Test seam/demo:** Launch runtime, close shell, run a task from CLI, reopen shell, then demonstrate drain and force with recorded lifecycle events.
