# 21: Optional supervised resource monitor

**Parent:** [Command-center decision map](../map.md).
**What to build:** Show measured resource use for supervised work and retain its final totals, while monitoring can be disabled without affecting execution.
**Blocked by:** [Independent runtime lifecycle](02-independent-runtime.md), [Attributed usage](12-attributed-usage.md), [Recoverable schema](16-recoverable-schema.md)
**Status:** in-progress.
**Execution gate:** the single package approval is required in addition to the named blockers.
**Review refs:** R07; R13 applies as targeted cleanup within this slice.
**Decision:** [Bounded record views and optional analysis](../decisions/02-data.md).

## Contract choices

The initial observer reads Windows supervised-job cumulative CPU time and memory high-water mark at a five-second default interval.
Record units, timestamp, run/job scope, collector version, and missing/error state.
Use verified job ownership; a reused process ID cannot transfer observations to another run.
Retain detailed samples for 48 hours by default and retain the final cumulative observation with the run.
CPU time is additive only across disjoint scopes; a memory high-water mark is not a summable total.
The optional view labels coverage and does not claim whole-host or GPU/network telemetry.
Sampling has a bounded queue and cannot block lifecycle writes; a dropped sample increments a coverage counter while authoritative lifecycle events remain intact.

## Acceptance criteria

- [x] A deterministic CPU/memory fixture produces nonnegative measured values with source, units, timestamp, and ownership scope, visible from run detail.
- [ ] Disabling or failing the collector leaves launch, cancellation, finalization, and unrelated views working; failure fallback is implemented, but an operator toggle is still open.
- [ ] The collector queries the owned Job Object handle rather than a PID, so reused PIDs and unrelated parent/child trees cannot transfer ownership; deterministic reused-PID and parent/child fixtures remain open.
- [x] Retention removes expired non-final samples while the final cumulative observation and task/run identity remain queryable.
- [x] The run detail distinguishes unavailable, stale, and measured data; it shows job scope and does not infer a host-wide percentage.

## Test seam and demo

The deterministic fixture and retention tests run in `farseer-runner` and `farseer-store` without sleeps.
The public run detail reads `/v1/runs/{id}/resources`, and the UI renders the latest state with safe fallback when that read fails.
The remaining live demo is a Windows supervised run with a runtime toggle and periodic samples.

## Exclusions

Cross-platform collectors, GPU/network tracing, OTLP servers, privileged host monitoring, and externally launched unowned processes remain outside this slice.
The first bounded slice records start and final cumulative observations; a five-second background sampler and operator toggle remain open before this ticket can be marked complete.
