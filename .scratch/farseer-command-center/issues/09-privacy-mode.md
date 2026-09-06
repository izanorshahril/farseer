# 09: Privacy presentation mode

**Parent:** [Command-center decision map](../map.md).

**What to build:** Add a screenshot-safe presentation mode that masks configured identities and sensitive locators across first-party command-center views.

**Blocked by:** None; package approved.

**Status:** in-progress.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R05; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/03-workspace.md).

**Contract choices:** Privacy masking is a presentation policy, never data deletion.
Raw values remain available to authorized operator actions through explicit reveal.
Masking defaults to on for account identities and sensitive paths in screenshot surfaces.

- [x] Capacity masks account identifiers by default while retaining provider, window, runner, and status meaning.
- [x] Project paths, transcript locators, session identifiers, and diagnostics follow an explicit field classification policy.
- [x] An operator can reveal one field temporarily with accessible state and automatic re-masking on restart or lock.
- [ ] Copy, export, notification, and screenshot surfaces use the masked representation unless explicitly authorized.
- [ ] Tests prove that masking changes presentation only and does not change routing, accounting, or stored record values.

**Evidence:** `ui/src/privacy.tsx` defaults presentation masking on and leaves source values untouched.
First-party widgets classify account, path, session, and diagnostic display fields through `mask`, including tooltip text, accessible labels, confirmation prompts, and graph/run identifiers.
Live runner task goals and project labels use the same diagnostic classification rather than bypassing the presentation policy.
Sealed skill paths in the Run contract use an explicit path reveal instead of raw `Fact` output.
`RevealField` exposes one field for ten seconds with accessible state, and visibility changes or privacy re-enable clear all reveals.
`copyPresentation` and `exportPresentation` keep masked output as the default and raw output requires a live field reveal or privacy being explicitly disabled.
The Run widget now exposes an export-report control that copies a bounded JSON presentation through the same masking helper.
The notification plane now correlates finished events by record sequence, maps unknown outcomes to a fixed generic state, and never exports a run/session identifier to an external sink.
The remaining acceptance evidence is a browser screenshot/copy/export pass over representative first-party views.

**Exclusions:** No encryption redesign, credential rotation, irreversible scrubbing, or privacy inference from arbitrary event payloads.

**Test seam/demo:** Populate fixtures with account, path, and session identifiers, enable presentation mode, and verify every first-party widget and export path.
