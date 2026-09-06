# 08: Responsive canvas layout

**Parent:** [Command-center decision map](../map.md).

**What to build:** Make the canvas readable from narrow desktop windows through large monitors while retaining the calm flow grid, explicit widget spans, and accessible navigation.

**Blocked by:** None; package approved.

**Status:** in-progress.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R05; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** Keep a flow grid as the default layout model.
Support focused navigation and optional manual ordering without adding a docking library.
Keep layout state opaque to the runtime.

- [x] The default Conversation, Work, Fleet, and Capacity arrangement fits the supported desktop viewport without permanent nested scrollbars.
- [x] Narrow windows collapse the rail and reflow cards while preserving keyboard reachability and focus order.
- [x] Layout changes provide compact, content-aware spans and never silently hide required controls.
- [x] Sidebar collapse, widget order, and span preferences survive restart through the existing UI-state contract.
- [ ] Reduced-motion and keyboard demos cover move, resize, focus restoration, and empty-canvas recovery.

**Evidence:** `ui/src/style.css` collapses the rail at 900px, reflows widgets to one column, keeps the composer reachable, and honors reduced-motion preferences.
`ui/src/layout.ts` and `ui/src/App.tsx` persist order, spans, unit metrics, sidebar state, and focused presentation state through the existing canvas blob.
Browser-level viewport and keyboard demos remain open.

**Exclusions:** No freeform overlapping canvas, docking framework, second top-level application mode, or runtime layout parsing.

**Test seam/demo:** Verify the same persisted layout at narrow, common, and wide viewport sizes, including Work graph expansion and composer visibility.
