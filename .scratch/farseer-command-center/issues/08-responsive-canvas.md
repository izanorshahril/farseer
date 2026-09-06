# 08: Responsive canvas layout

**Parent:** [Command-center decision map](../map.md).

**What to build:** Make the canvas readable from narrow desktop windows through large monitors while retaining the calm flow grid, explicit widget spans, and accessible navigation.

**Blocked by:** None; package approved.

**Status:** complete.

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
- [x] Reduced-motion and keyboard demos cover move, resize, focus restoration, and empty-canvas recovery.

**Evidence:** `ui/src/style.css` collapses the rail at 900px, reflows widgets to one column, keeps the composer reachable, and honors reduced-motion preferences.
`ui/src/layout.ts` and `ui/src/App.tsx` persist order, spans, unit metrics, sidebar state, and focused presentation state through the existing canvas blob.
Browser-level viewport and keyboard demos remain open.
The 2026-09-06 browser smoke at the supported narrow desktop viewport moved Work with its keyboard grip, resized it with the accessible size control, restored focus after Escape from the focused workspace, and recovered the empty canvas by showing Work again; the stylesheet continues to honor `prefers-reduced-motion`.
At 867x912, Work stayed within the viewport at 829x452 with the graph selected and the composer visible, and the 2x2 span survived a reload without horizontal overflow.
At 1024x720 and 1440x900, the same graph, composer, and 2x2 span remained visible with no horizontal overflow; Work measured 612x452 in both layouts.
These three viewport checks supersede the earlier line stating that browser demos remained open.

**Exclusions:** No freeform overlapping canvas, docking framework, second top-level application mode, or runtime layout parsing.

**Test seam/demo:** Verify the same persisted layout at narrow, common, and wide viewport sizes, including Work graph expansion and composer visibility.
