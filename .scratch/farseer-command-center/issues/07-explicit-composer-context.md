# 07: Explicit composer context

**Parent:** [Command-center decision map](../map.md).

**What to build:** Make every operator request visibly and deliberately target a conversation, project, and optional declared harness choice while preserving top-manager ingress.

**Blocked by:** None; package approved.

**Status:** in-progress.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R04; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** Every request still enters through the top manager.
Widget context is an anchor, never an alternate execution address.
Runner or model selection is permitted only when declared by the cell and recorded in the resulting run contract.

- [x] Hovering or traversing cards never changes the active composer target.
- [x] Click, keyboard command, or accessible picker explicitly pins and clears project, conversation, task, and widget anchor context.
- [x] The composer shows explicit context or the valid global default; stale/unauthorized context fails clearly, and valid submission needs no extra confirmation.
- [x] A conversation harness pin accepts only a declared candidate and creates a new manager run when changed.
- [x] The accepted request records project, conversation, anchor, selected runner, and actor provenance.

**Exclusions:** No arbitrary installed-runner picker, model migration claim, second per-widget agent address, or hidden routing through UI state.

**Test seam/demo:** Pin a project and conversation, move the pointer across widgets, submit, then verify the same explicit context in the queued run and conversation projection.

## Evidence

`ui/src/selection.ts` now creates an immutable `ComposerContext` snapshot at submission time, so a later selection cannot retarget an in-flight request.
The composer exposes accessible face, project, conversation, task, and manager-runner pickers with explicit clear values.
`ui/src/SandboxWidget.tsx` stamps the same snapshot at the host bridge boundary, while hover remains inert.
`crates/farseer-api/src/lib.rs` rejects contradictory body and anchor identifiers before creating conversations, tasks, or runs.
`ui/tests/composer-context.test.ts` covers immutable capture and stale optional-anchor fields, and `a_contradictory_operator_anchor_is_refused_before_creating_work` proves the public refusal path leaves the store unchanged.
Automated browser interaction coverage remains open because the UI test harness has no browser renderer.
