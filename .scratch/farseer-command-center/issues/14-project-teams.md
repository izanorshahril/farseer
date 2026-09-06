# 14: Project team profiles

**Parent:** [Command-center decision map](../map.md).

**What to build:** Let a project choose a solo or multi-harness team by pointing at an existing cell definition, while keeping grants, budgets, and policy owned by that cell.

**Blocked by:** [Explicit composer context](07-explicit-composer-context.md), [Explainable routing](13-explainable-routing.md)

**Status:** complete.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R09; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/04-routing.md).

**Contract choices:** Versionable file-based project profiles point at existing cells and never duplicate grants.
Validation/reload exposes the effective profile through a read projection; file configuration remains authoritative.
The existing top-manager ingress and cell-call path remain the first delivery.
The profile supplies a validated default eligible coordinating cell; actual top-manager tokens remain visible in usage and provenance.

- [x] A project can select one existing cell profile and display its manager and worker roster before submitting work.
- [x] A project with one roster entry behaves as a solo team; a project with several entries records each supervised delegation and cap.
- [x] An invalid, missing, or unauthorized cell reference blocks submission with a repair path and no partial run.
- [x] Switching a project profile affects future tasks only and records old profile, new profile, actor, and reason.
- [x] Global and project boards show the same task under the selected project team without duplicating it.

**Evidence:** Version-one `.farseer/profile.toml` files are validated against authorized project paths and existing cells, exposed through project views and `/v1/projects/profile`, and applied to future top-manager submissions.
When a profile declares specialist cells, the existing pinned cell roster remains the grant and the profile narrows calls to the declared specialist set; each accepted cell call records the profile specialist set beside its bounded call budget.
The project widget now expands the selected profile to show manager runners, each declared roster entry, and the last profile transitions with old/new cell, actor, reason, and timestamp.
The public profile tests cover manager/roster projection and missing specialist refusal before any work rows are created.
The deterministic `a_project_team_records_its_specialist_set_and_refuses_other_cells` regression records two accepted specialist calls, preserves each supervised call's autonomy cap and specialist set, refuses a callable cell outside that set before creating a run, and preserves the caller's run count.
The profile route now projects append-only transition history; the public profile-switch regression covers two future tasks, old/new cells, actor/reason, and equal global/project task projections without duplication.
`a_top_manager_routes_a_project_team_through_public_ingress` is the full public multi-cell proof: one task receives two declared specialist calls, each call records the reversible cap and specialist set, graph topology remains attached to the task, and global/project boards return one identical task row.

**Exclusions:** No project-specific copy of cell grants, arbitrary runner discovery, direct non-zero project ingress, or promise of zero coordination cost.

**Test seam/demo:** Bind two projects to existing solo and multi-harness cells, submit work through the top manager, and verify policy, run graph, token accounting, and board provenance.
