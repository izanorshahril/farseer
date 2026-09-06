# 10: Focused workspace

**Parent:** [Command-center decision map](../map.md).

**What to build:** Give the operational cards a common Open details action into a shared focused workspace with optional navigation, main content, and inspector panes.

**Blocked by:** [Widget recovery and isolation](06-widget-recovery.md), [Explicit composer context](07-explicit-composer-context.md), [Responsive canvas layout](08-responsive-canvas.md)

**Status:** not-started.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R05; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** The canvas remains home; focused mode is a shared presentation over the same selected identities and commands.
Each feature supplies relevant detail rather than every feature rendering a full IDE.
Provide route, breadcrumb, Back, focus restoration, and one optional second main pane for comparing two subjects.
No docking library is required.

- [ ] Conversation, Work, Fleet, and Capacity expose the same titlebar convention for opening a focused face.
- [ ] Navigation/main/inspector toggles work for a selected task and run, with an optional comparison pane; hidden panes do not alter execution state.
- [ ] Back restores the originating face, subject, and keyboard focus, including after restart.
- [ ] Inactive faces are removed from accessibility navigation and reduced-motion mode remains understandable.
- [ ] A focused workspace failure returns to the last valid face without affecting runtime execution.

**Exclusions:** No arbitrary nested docking layout, terminal pane as runtime truth, or widget-owned agents.

**Test seam/demo:** Open a task, inspect its run/event details, compare two subjects, press Back, and verify filter/scroll/focus restoration; graph content is integrated when its own slice is available.
