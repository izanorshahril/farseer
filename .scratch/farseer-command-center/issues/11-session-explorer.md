# 11: Session explorer

**Parent:** [Command-center decision map](../map.md).

**What to build:** Let operators explore harness sessions and log references across a task or project while distinguishing farseer evidence from harness-owned transcript custody.

**Blocked by:** [Bounded transcript projection](05-bounded-transcript-index.md), [Focused workspace](10-focused-workspace.md)

**Status:** in-progress.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R06; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/02-data.md).

**Contract choices:** Preserve provider identifier kind and observed model/provider fields.
Treat missing or dangling log pointers as honest states.
Use event replay as the reliable fallback when harness logs are absent.

- [ ] A project, conversation, task, or run can open a bounded session list with provider kind, runner, model, first-seen time, and availability.
- [ ] Session detail links to observed topology, transcript custody, derived search text, and the originating run without duplicating truth.
- [ ] Missing, rotated, and unavailable logs are labelled distinctly and do not break the session explorer.
- [ ] Search returns paged scrubbed excerpts with source digest and projection version.
- [ ] A session explorer demo works after runtime restart and with two sessions from different harness protocols.

**Evidence:** The API exposes a bounded `/v1/work/sessions` projection, the Work widget has a paged Sessions face, and task detail includes session and transcript-projection references.
The additive `/v1/work/search/page` route now returns bounded scrubbed excerpts, source digests, projection versions, and cursors.
The two-protocol restart demo and a dedicated search face remain open.

**Exclusions:** No automatic transcript copying, private harness-directory guessing, external embeddings service, or raw log viewer without explicit custody.

**Test seam/demo:** Use one reference session, one copied session, and one missing pointer; navigate from task to session to graph evidence and back.
