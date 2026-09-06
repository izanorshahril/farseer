# 13: Explainable routing

**Parent:** [Command-center decision map](../map.md).

**What to build:** Route a task or worker contract deterministically among declared runner candidates using observed availability, enforceable constraints, and provenance that explains every fallback.

**Blocked by:** [Attributed usage](12-attributed-usage.md)

**Status:** proposed-awaiting-review.

**Execution:** blocked until the command-center ticket package receives one final approval.

**Review refs:** R08; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/04-routing.md).

**Contract choices:** Runner and model pins are explicit contract inputs.
The router may reorder only candidates the cell author declared equivalent.
An absent cost remains unknown unless a versioned price basis supports an explicitly labelled estimate.
Explicit unsupported pins fail instead of silently falling through; zero automatic retries is the initial default.

- [ ] A run is sealed with the selected runner, model policy, candidate list, budget, and selection reason before spawn.
- [ ] Exhausted, overage, unknown, and available states produce deterministic candidate ordering without introducing undeclared runners.
- [ ] Every fallback records preferred candidate, selected candidate, observed pressure, actor, and estimated or reported cost basis.
- [ ] Each requested bounded dimension retains its existing enforceability check; post-run accounting never substitutes for verified pre-spend enforcement.
- [ ] Replaying the same input and observations produces the same selection and provenance.

**Exclusions:** No token-level router, opaque LLM judge, arbitrary installed model picker, or vendor gateway in the core.

**Test seam/demo:** Declare two candidates, mark the preferred account exhausted, run once, and inspect the sealed contract, fallback event, and resulting analytics attribution.
