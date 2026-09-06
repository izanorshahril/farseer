# 13: Explainable routing

**Parent:** [Command-center decision map](../map.md).

**What to build:** Route a task or worker contract deterministically among declared runner candidates using observed availability, enforceable constraints, and provenance that explains every fallback.

**Blocked by:** [Attributed usage](12-attributed-usage.md)

**Status:** in-progress.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R08; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/04-routing.md).

**Contract choices:** Runner and model pins are explicit contract inputs.
The router may reorder only candidates the cell author declared equivalent.
An absent cost remains unknown unless a versioned price basis supports an explicitly labelled estimate.
Explicit unsupported pins fail instead of silently falling through; zero automatic retries is the initial default.

- [x] A run is sealed with the selected runner, model policy, candidate list, budget, and selection reason before spawn.
- [x] Exhausted, overage, unknown, and available states produce deterministic candidate ordering without introducing undeclared runners.
- [x] Every fallback records preferred candidate, selected candidate, observed pressure, actor, and estimated or reported cost basis.
- [ ] Each requested bounded dimension retains its existing enforceability check; post-run accounting never substitutes for verified pre-spend enforcement.
- [x] Replaying the same input and observations produces the same selection and provenance.

**Evidence:** New instructions honor explicit and conversation runner pins, select the first non-exhausted declared candidate, and record a preferred-runner fallback event.
`routing_sealed` is now appended before admission and process creation for API-launched runs, with the selected runner, declared candidate order, observed pressure, preferred candidate, model and effort policy, budget, cost basis, retry policy, and selection reason.
Delegated worker contracts emit the same bounded record before their workspace is created.
The routing projection has a deterministic replay test covering an exhausted preferred account and an unknown fallback candidate.
The full acceptance demo and analytics attribution read model remain open.

**Exclusions:** No token-level router, opaque LLM judge, arbitrary installed model picker, or vendor gateway in the core.

**Test seam/demo:** Declare two candidates, mark the preferred account exhausted, run once, and inspect the sealed contract, fallback event, and resulting analytics attribution.
