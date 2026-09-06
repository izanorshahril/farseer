# 18: Safe staged runtime promotion

**Parent:** [Command-center decision map](../map.md).
**What to build:** Promote one approved local runtime candidate and recover to its matched prior binary/data if activation fails.
**Blocked by:** [Independent runtime lifecycle](02-independent-runtime.md), [Recoverable schema and backup](16-recoverable-schema.md), [Bounded maintenance source proposals](17-maintenance-proposals.md).
**Status:** in-progress; bounded migration and candidate-startup execution is implemented, while the public end-to-end smoke demonstration remains open.
**Execution:** package approved; verify named blockers before implementation; a real future promotion separately requires product-level operator authorization.
**Review refs:** R11, R14; R13 applies as targeted cleanup.
**Decision:** [Self-maintenance and domain integration](../decisions/05-maintenance.md).

## Contract choices

Promotion names immutable candidate and prior-version identities, not the current dirty tree.
A deterministic release operation stages beside the active installation, drains, takes a consistent verified backup, migrates if needed, starts the candidate, and verifies authenticated health plus a local smoke scenario.
Drain expiry reports remaining work and does not force cancellation.
A pre-switch failure leaves the old version active; a post-switch failure restores the matched old binary and data before admitting work.
Recovery runs independently of the candidate process.
A failed restore leaves an actionable stopped state rather than an endless restart loop.
Use an isolated installation/data fixture for development and tests.

## Acceptance criteria

- [x] An explicitly authorized fixture candidate is staged, backed up, activated, and health-verified with recorded phase and version identities.
- [x] A changed candidate identity, failed drain, failed backup, or migration failure prevents unsafe activation and reports the recovery state.
- [x] A fixture candidate startup/smoke failure restores the prior matched binary/data and leaves admission gated until verification.
- [x] The rollback command works without starting the candidate; an injected restore failure remains stopped with concrete recovery instructions.
- [x] No running executable is overwritten in place and no old binary is opened against incompatible new-schema data.

**Exclusions:** Multi-host rollout, live operator installation changes in the test, silent hot patching, and an automatic agent judgment as the promotion gate.
**Test seam/demo:** Use a disposable versioned installation with a deliberately failing candidate; observe automatic bounded recovery and query the restored task record.

## Evidence

`farseer-store::maintenance::FixturePromotion` now stages a digest-checked candidate beside an active fixture, requires a zero-run drain and matched backup gate, runs an explicitly configured bounded migration before activation when the schema changes, switches directory identities only through an explicit call, runs an explicitly configured bounded candidate startup probe, verifies authenticated health and smoke evidence, and restores the prior directory without consulting the candidate during rollback.
Promotion journal writes retain the previous metadata file during the Windows replacement window and restore it if publishing the new journal fails.
The fixture tests cover stage, backup gate, activation, health verification, failed-health rollback, and an injected restore failure that records a stopped recovery note.
`farseer promote-fixture` now drives those phases for a disposable root and persists each phase, while `farseer rollback-fixture` works from the durable journal without starting the candidate.
The unit seam covers successful and failing migration/startup commands, timeout bounds, journal observations, and recovery back to the previous fixture.
The remaining evidence gap is a public CLI demonstration with a deliberately failing candidate smoke command and the resulting restored task/admission record.
