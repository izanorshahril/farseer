# 07: Explicit composer context

**Parent:** [Command-center decision map](../map.md).

**What to build:** Make every operator request visibly and deliberately target a conversation, project, and optional declared harness choice while preserving top-manager ingress.

**Blocked by:** None (eligible only after package approval).

**Status:** in-progress.

**Execution:** blocked until the command-center ticket package receives one final approval.

**Review refs:** R04; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** Every request still enters through the top manager.
Widget context is an anchor, never an alternate execution address.
Runner or model selection is permitted only when declared by the cell and recorded in the resulting run contract.

- [ ] Hovering or traversing cards never changes the active composer target.
- [ ] Click, keyboard command, or accessible picker explicitly pins and clears project, conversation, task, and widget anchor context.
- [ ] The composer shows explicit context or the valid global default; stale/unauthorized context fails clearly, and valid submission needs no extra confirmation.
- [ ] A conversation harness pin accepts only a declared candidate and creates a new manager run when changed.
- [ ] The accepted request records project, conversation, anchor, selected runner, and actor provenance.

**Exclusions:** No arbitrary installed-runner picker, model migration claim, second per-widget agent address, or hidden routing through UI state.

**Test seam/demo:** Pin a project and conversation, move the pointer across widgets, submit, then verify the same explicit context in the queued run and conversation projection.

## Evidence

The current increment makes widget context change on explicit click or context-menu action only, keeps hover and focus inert, and displays the pinned widget, project, and conversation beside the top-manager route.
The existing Work subject picker and bridge request contract continue to carry project, conversation, task, anchor, and manager runner.
Automated browser interaction coverage and stale-context refusal remain open.
