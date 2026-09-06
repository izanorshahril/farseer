# 06: Widget recovery and isolation

**Parent:** [Command-center decision map](../map.md).

**What to build:** Keep one malformed or unavailable widget from blanking the command center, and give every first-party read a consistent recoverable state.

**Blocked by:** None; package approved.

**Status:** in-progress.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R03; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** UI failures never alter runtime truth.
Each widget owns a bounded loading and error boundary.
Diagnostics may be expanded, but raw backend traces are not the primary message.

- [ ] A render exception in one built-in or authored widget leaves all sibling widgets and shell controls interactive.
- [ ] A failed read keeps the last successful projection when available and offers retry, scope reduction, or diagnostics.
- [ ] Errors identify the affected capability and correlation context without exposing bearer tokens or raw private paths.
- [ ] Stream disconnects show stale or reconnecting state and recover without duplicate event rows.
- [ ] A failure and recovery demo proves the runtime continues accepting and recording work while the UI widget is broken.

**Exclusions:** No retry loop inside the Rust core, silent fallback to fabricated data, or privileged bridge expansion for authored widgets.

**Test seam/demo:** Inject a 404, malformed JSON response, stream disconnect, and component throw into separate widgets and verify localized recovery.

## Evidence

The current increment adds `ui/src/WidgetBoundary.tsx` around every first-party and authored widget, a retry action with redacted operator-facing diagnostics, and shared SSE `connecting`/`live`/`stale` state rendered by Activity.
`bun run --cwd ui check` and `bun run --cwd ui test` pass.
Per-widget stale projection retention and injected browser failure tests remain open.
