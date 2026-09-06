# 10: Focused workspace

**Parent:** [Command-center decision map](../map.md).

**What to build:** Give the operational cards a common Open details action into a shared focused workspace with optional navigation, main content, and inspector panes.

**Blocked by:** [Widget recovery and isolation](06-widget-recovery.md), [Explicit composer context](07-explicit-composer-context.md), [Responsive canvas layout](08-responsive-canvas.md)

**Status:** complete.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R05; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** The canvas remains home; focused mode is a shared presentation over the same selected identities and commands.
Each feature supplies relevant detail rather than every feature rendering a full IDE.
Provide route, breadcrumb, Back, focus restoration, and one optional second main pane for comparing two subjects.
No docking library is required.

- [x] Conversation, Work, Fleet, and Capacity expose the same titlebar convention for opening a focused face.
- [x] Navigation/main/inspector toggles work for a selected task and run, with an optional comparison pane; hidden panes do not alter execution state.
- [x] Back restores the originating face, subject, and keyboard focus, including after restart.
- [x] Inactive faces are removed from accessibility navigation and reduced-motion mode remains understandable.
- [x] A focused workspace failure returns to the last valid face without affecting runtime execution.

**Evidence:** `ui/src/App.tsx` gives each face the same focus action, persists focus, pane, comparison state, and the originating subject/anchor in the canvas document, restores the subject and focus button after restart, restores focus on Back or Escape, masks inspector identities, and clears stale focused faces.
`ui/src/layout.ts` validates the persisted focus origin, and `ui/tests/layout.test.ts` covers its round trip.
`ui/src/style.css` hides inactive faces from the accessibility tree through layout removal and supplies responsive navigation and reduced-motion behavior.
The 2026-09-06 browser smoke opened Work through its shared focus action, exposed `Back to canvas`, returned to the canvas, and restored the Work focus action for the originating face.

**Exclusions:** No arbitrary nested docking layout, terminal pane as runtime truth, or widget-owned agents.

**Test seam/demo:** Open a task, inspect its run/event details, compare two subjects, press Back, and verify filter/scroll/focus restoration; graph content is integrated when its own slice is available.
