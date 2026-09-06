# Canvas home and focused work

**Parent:** [Command-center decision map](../map.md).
**Label:** wayfinder:grilling.
**Status:** recommendation selected, awaiting the single package review.
**Selection authority:** delegated selection from the user's review request.
**Blocked by:** [Bounded record views and optional analysis](02-data.md).

## Question

Which layout and interaction model gives a versatile command center without turning every card into a tiny IDE?

## Nominees

| Nominee | Glanceable home | Sustained work | Complexity | Decision |
| --- | --- | --- | --- | --- |
| Existing flow canvas plus shared focused workspace | Strong | Optional navigation/main/inspector | Incremental | Selected |
| Everything inside compact cards | Strong only at low density | Clipping and nested navigation | Current limitations remain | Rejected |
| Full arbitrary docking framework | Possible | Flexible | Layout trees, migrations, drag rules | Deferred |
| Free-position infinite plane | Visually flexible | Requires camera/collision behavior | Adds another layout model | Deferred |

## Proposed resolution

Keep the existing non-overlapping flow grid, size spans, and canvas home.
Extend manual sidebar collapse with a persisted pin/reveal preference.
Put global size metrics and arrangement controls in edit-layout mode.
Use per-widget useful defaults and a deterministic arrange/reset action rather than making the whole canvas fill available height.
Bound content expansion so live output cannot repeatedly move unrelated cards.

Provide one consistent Open details action for meaningful subjects; mark purely decorative widgets as not drillable.
The focused workspace shares selected project/conversation/task/run/session identities with the canvas.
Its initial layout has toggleable navigation, main content, and inspector panes, plus one optional second main pane for comparing two subjects.
Use the current React/CSS stack for that bounded layout; select no docking dependency in this plan.
Returning to the canvas preserves filters, scroll position, and unsent draft text.

Explicit clicks/keyboard selection pin composer context.
Hover may preview a subject but cannot retarget a draft.
The composer shows project, conversation, requested runner, and supported requested model, with observed values distinguished after dispatch.
Switching harness starts the appropriate successor run/session; it never rewrites a sealed contract or implies cross-vendor native session migration.
Ctrl+K opens commands; it does not create a second ambiguous instruction destination.

Each built-in widget gets render-failure isolation, retry/remount, stale-data retention, and structured load states.
Data refresh is single-flight with a dirty flag and a maximum delay; sustained events cannot postpone it forever.
Late responses are ignored after a newer selection/query supersedes them.
Unavailability of an optional graph is not failure of the task board.

Privacy mode is a persisted presentation preference with explicit reveal, not access control or retroactive record scrubbing.
It covers text, tooltips, accessible names, copy/export, and identifying account/path/session fields.
The initial reveal expires when privacy mode is re-enabled or the view closes.
Raw transcript access retains its separate custody and authorization rules.

## Terminal nominee

Select an optional supervised Windows ConPTY terminal surface with a single session as its first complete slice.
PowerShell, cmd, and Git Bash are configured executable/argv profiles discovered for availability.
Changing a profile starts a new terminal session; an existing shell and a harness protocol channel remain unchanged.
The adapter owns resize/input/output, bounded scrollback, reconnect, and process cleanup.
Closing a terminal view detaches; explicit End session terminates the owned process and releases its workspace lease.
Run-workspace cleanup remains pending while a terminal lease holds that directory, with an explicit End session action rather than deletion under a live process.
It holds the shell authority granted to that session and cannot acquire new project authority from a UI dropdown.
No terminal-emulator package is selected without the implementation ticket checking maintained compatibility and pinning the chosen dependency.
External launch and attaching arbitrary existing terminals are outside the first slice because their ownership/observation guarantees differ.

## Verification and justification

Use browser interaction tests through HTTP fixtures and desktop checks for terminal/lifecycle behavior.
Check 1280x820, 1920x1080, and a narrow window at 100% and 150% display scaling.
Verify keyboard-only navigation, screen-readable errors, privacy text surfaces, fast selection changes, and a widget that throws.
Those tests fill the gap left by the six current layout-arithmetic tests.

The bounded workspace addresses the user's Berd/Orca-inspired interaction goals while retaining current data and security seams.
No claimed pixel result comes from a new mockup in this planning pass.

## Existing decisions affected

Append a correction to [Operator surface](../../farseer/issues/28-operator-surface.md) and [Work and session explorer](../../farseer/issues/40-work-and-session-explorer.md) after approval: a shared focused workspace is allowed beneath the canvas home.
[Attach semantics](../../farseer/issues/07-attach-semantics.md) still keeps PTY handling at an adapter rather than in pure core.
Top-manager instruction ownership is unchanged by layout or selection.

## Revisit condition

Consider arbitrary docking only after two concrete workflows cannot be expressed by the selected pane toggles and comparison split.
