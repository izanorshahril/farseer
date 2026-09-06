# 12: Attributed usage

**Parent:** [Command-center decision map](../map.md).

**What to build:** Show retry-inclusive task/run usage in Capacity and run detail, with distinct observation bases and bounded time/project scope.

**Blocked by:** [Paged work board](03-paged-work-board.md)

**Status:** in-progress.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R07; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/04-routing.md).

**Contract choices:** Provider percentages remain provider-reported.
Model share is analytics, never quota.
Task and run breakdowns use observed totals and explicit estimated or reported cost basis.
Process CPU and memory sampling remains a later bounded extension.

- [x] Capacity exposes provider windows, reset times, account sharing, and source provenance without deriving a percentage from farseer spend.
- [x] Context usage shows `used / size` only when both were observed and labels absent denominators explicitly.
- [x] Task and run views show tokens, reported cost, estimated cost, duration, runner, model, outcome, and denominator scope.
- [ ] A parent aggregate and its child observations are not counted twice; a task with retries includes coordination and failed-attempt cost while naming its acceptance/outcome criterion.
- [x] A bounded read returns stable page or cursor metadata and never loads all historical usage by default.

**Evidence:** The API exposes bounded attributed cost pages and the Capacity widget renders observed spend groups.
Provider windows remain source-labelled and do not derive a percentage from farseer spend.
Run detail now exposes observed duration, cost basis (`reported`, `estimated`, or `unknown`), and run scope.
Task detail aggregates distinct run rows with successful/failed counts, tokens, reported and estimated spend, duration, and an explicit task scope.
Parent/child aggregate de-duplication evidence and the two-task acceptance demo remain open.

**Exclusions:** No GPU claim, one-second process sampler, fleet-wide invented billing, or model share presented as provider quota.

**Test seam/demo:** Run two tasks with distinct models and cost evidence, inspect task/run breakdowns and Capacity analytics, and verify unavailable fields remain absent.
