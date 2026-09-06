# 08: Responsive canvas layout

**Parent:** [Command-center decision map](../map.md).

**What to build:** Make the canvas readable from narrow desktop windows through large monitors while retaining the calm flow grid, explicit widget spans, and accessible navigation.

**Blocked by:** None (eligible only after package approval).

**Status:** proposed-awaiting-review.

**Execution:** blocked until the command-center ticket package receives one final approval.

**Review refs:** R05; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** Keep a flow grid as the default layout model.
Support focused navigation and optional manual ordering without adding a docking library.
Keep layout state opaque to the runtime.

- [ ] The default Conversation, Work, Fleet, and Capacity arrangement fits the supported desktop viewport without permanent nested scrollbars.
- [ ] Narrow windows collapse the rail and reflow cards while preserving keyboard reachability and focus order.
- [ ] Layout changes provide compact, content-aware spans and never silently hide required controls.
- [ ] Sidebar collapse, widget order, and span preferences survive restart through the existing UI-state contract.
- [ ] Reduced-motion and keyboard demos cover move, resize, focus restoration, and empty-canvas recovery.

**Exclusions:** No freeform overlapping canvas, docking framework, second top-level application mode, or runtime layout parsing.

**Test seam/demo:** Verify the same persisted layout at narrow, common, and wide viewport sizes, including Work graph expansion and composer visibility.
