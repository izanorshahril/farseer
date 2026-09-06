# 21: Optional supervised resource monitor

**Parent:** [Command-center decision map](../map.md).
**What to build:** Show measured resource use for supervised work and retain its final totals, while monitoring can be disabled without affecting execution.
**Blocked by:** [Independent runtime lifecycle](02-independent-runtime.md), [Attributed usage](12-attributed-usage.md), [Recoverable schema](16-recoverable-schema.md)
**Status:** proposed-awaiting-review.
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

- [ ] A deterministic CPU/memory fixture produces nonnegative measured values with source, units, timestamp, and ownership scope, visible from run detail.
- [ ] Disabling or failing the collector leaves launch, cancellation, finalization, and unrelated views working.
- [ ] Reused PID and parent/child aggregate fixtures neither misattribute nor double-count usage.
- [ ] Retention removes expired samples while the final cumulative observation and task/run identity remain queryable.
- [ ] The UI distinguishes unavailable, stale, and measured data; no host-wide percentage is inferred from partial observation.

## Test seam and demo

Start a deterministic supervised job through the public command interface and compare its recorded samples with the fixture's known ownership.
Toggle collection while inspecting the same run through the operator view.
Exercise collector failure and retention using injected observation/time inputs rather than sleeps.

## Exclusions

Cross-platform collectors, GPU/network tracing, OTLP servers, privileged host monitoring, and externally launched unowned processes remain outside this slice.
