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
- [x] Errors identify the affected capability and correlation context without exposing bearer tokens or raw private paths.
- [x] Stream disconnects show stale or reconnecting state and recover without duplicate event rows.
- [ ] A failure and recovery demo proves the runtime continues accepting and recording work while the UI widget is broken.

**Exclusions:** No retry loop inside the Rust core, silent fallback to fabricated data, or privileged bridge expansion for authored widgets.

**Test seam/demo:** Inject a 404, malformed JSON response, stream disconnect, and component throw into separate widgets and verify localized recovery.

## Evidence

`ui/src/WidgetBoundary.tsx` surrounds every first-party and authored widget, so a render exception is localized to that widget.
`ui/src/ReadFailure.tsx` gives failed reads a capability label, retry action, optional scope reduction, and a bounded incident/status diagnostic without rendering backend messages.
Conversation, run detail, runner thread, delegation, fleet, projects, quota, work, settings, and list widgets retain prior projections where available and use the shared recovery surface.
`ui/src/stream.ts` marks EOF and transport errors stale, reconnects from the exclusive cursor, and drops replayed sequence numbers before dispatch.
`ui/tests/recovery.test.ts` covers diagnostic redaction and duplicate stream-frame suppression.
`ui/tests/recovery.test.ts` also closes a first stream after one event, observes stale then live state, verifies the exclusive cursor on the second request, and receives the next event without duplication.
`ui/tests/recovery.test.ts` also covers non-OK and rejected fetches, malformed SSE data followed by a valid frame, and cursor-specific followers reporting their own state instead of overwriting the shared canvas connection's status.
`bun run --cwd ui check` and `bun run --cwd ui test` pass.

The runtime-continuity demo with an injected browser failure remains a manual E2E check because the UI test harness has no browser renderer or live farseer process.
