# 19: Honest tool authority

**Parent:** [Command-center decision map](../map.md).
**What to build:** Make declared capabilities, enforced tool levels, and unavailable tool-serving paths unambiguous in validation and the operator view.
**Blocked by:** None (eligible only after package approval).
**Status:** implemented.
**Execution:** blocked until the command-center package receives its single approval.
**Review refs:** R15; R13 applies as targeted cleanup.
**Decision:** [Self-maintenance and domain integration](../decisions/05-maintenance.md).

## Contract choices

Preserve the existing decision against a general third-party tool gateway.
A roster tool without a callable Farseer path is recorded but explicitly non-enforcing.
Clarify the existing two checks: explicit cell shell grant authorizes reach, while ToolLevel requests a runner restriction.
Effective reach satisfies both; the existing default of ToolLevel::Shell cannot confer a missing explicit grant.
Shell-capable roster metadata affects launch authorization even when the named tool has no callable Farseer verb.
An integration that performs an external action owns its real authorization and reconciliation unless a separately defined Farseer operation actually serves that call.
Shell-capable execution is not presented as per-tool containment.

## Acceptance criteria

- [ ] Validation and Fleet/definition detail distinguish enforced reach, observed capability, and recorded-only tool declarations.
- [ ] A tool entry with no serving path has no misleading approve/run affordance.
- [ ] Missing explicit shell grant remains refused for a shell-equivalent launch even when ToolLevel defaults to Shell; restricted runner fixtures retain their observed enforcement rules.
- [ ] Existing unauthorized launch/delegation requests remain refused server-side; presentation cannot create a grant.
- [ ] Fixtures demonstrate both a supported constrained runner and shell-capable reach without claiming control of arbitrary third-party calls.

**Exclusions:** Universal action approvals, third-party MCP proxying, new tool-grant concepts, and any claim that a shell cannot bypass a decorative tool label.
**Test seam/demo:** Load definitions with differing ToolLevel and shell-grant metadata, inspect effective behavior/advisories, and attempt an ungranted launch and delegation.

## Evidence

Implemented in `crates/farseer-api/src/lib.rs`, `ui/src/widgets/fleet.tsx`, and `ui/src/style.css`.
Cell summaries now expose observed authority, the Fleet detail marks roster declarations as recorded-only, shell reach remains gated by an explicit grant, and unsupported runner containment is not advertised.
Focused authority tests, API checks, UI typecheck, formatting, and diff checks pass.
