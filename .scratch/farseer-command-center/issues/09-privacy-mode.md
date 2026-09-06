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
- [ ] An operator can reveal one field temporarily with accessible state and automatic re-masking on restart or lock.
- [ ] Copy, export, notification, and screenshot surfaces use the masked representation unless explicitly authorized.
- [ ] Tests prove that masking changes presentation only and does not change routing, accounting, or stored record values.

**Evidence:** `ui/src/privacy.ts` defaults presentation masking on and leaves source values untouched.
First-party widgets classify account, path, session, and diagnostic display fields through `mask`, and `ui/tests/privacy.test.ts` covers presentation-only masking.
Per-field temporary reveal and explicit copy/export authorization remain open.

**Exclusions:** No encryption redesign, credential rotation, irreversible scrubbing, or privacy inference from arbitrary event payloads.

**Test seam/demo:** Populate fixtures with account, path, and session identifiers, enable presentation mode, and verify every first-party widget and export path.
