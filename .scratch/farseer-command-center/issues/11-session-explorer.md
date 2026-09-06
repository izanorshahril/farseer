# 11: Session explorer

**Parent:** [Command-center decision map](../map.md).

**What to build:** Let operators explore harness sessions and log references across a task or project while distinguishing farseer evidence from harness-owned transcript custody.

**Blocked by:** [Bounded transcript projection](05-bounded-transcript-index.md), [Focused workspace](10-focused-workspace.md)

**Status:** complete.

**Execution:** package approved; verify named blockers before implementation.

**Review refs:** R06; R13 applies as targeted cleanup within this slice.

**Decision:** [Selected contract](../decisions/02-data.md).

**Contract choices:** Preserve provider identifier kind and observed model/provider fields.
Treat missing or dangling log pointers as honest states.
Use event replay as the reliable fallback when harness logs are absent.

- [x] A project, conversation, task, or run can open a bounded session list with provider kind, runner, model, first-seen time, and availability.
- [x] Session detail links to observed topology, transcript custody, derived search text, and the originating run without duplicating truth.
- [x] Missing, rotated, and unavailable logs are labelled distinctly and do not break the session explorer.
- [x] Search returns paged scrubbed excerpts with source digest and projection version.
- [x] A session explorer demo works after runtime restart and with two sessions from different harness protocols.

**Evidence:** The API exposes a bounded `/v1/work/sessions` projection, the Work widget has a paged Sessions face, and task detail includes session and transcript-projection references.
The additive `/v1/work/search/page` route now returns bounded scrubbed excerpts, source digests, projection versions, and cursors.
Search pagination filters in SQLite before applying the offset, and advances by rows actually emitted when the byte cap ends a page, so non-matching or truncated projections cannot hide later matches.
The public session-page regression test now proves bounded paging, provider identifier kind, runner/model facts, distinct referenced, rotated, and unavailable log pointers across protocol-shaped sessions, and project/conversation/task/run filters.
The same public fixture now opens `/v1/work/session` and verifies the originating task/run, parent topology, transcript custody, projection status, and scrubbed indexed excerpt.
Session detail now applies the 32-row relationship and attachment cap in SQLite before decoding rows, so a large run cannot materialize its full parent or transcript archive before the API limit.
`the_session_explorer_survives_restart_with_two_harness_protocols` writes a file-backed record with Claude and ACP-shaped sessions, drops the first API state, reopens the same record, and verifies both protocol identifiers and runners through `/v1/work/sessions`.

The session explorer acceptance is complete: the public list, detail, search, filter, bounded relationship, and two-protocol restart regressions cover every contract item.

**Exclusions:** No automatic transcript copying, private harness-directory guessing, external embeddings service, or raw log viewer without explicit custody.

**Test seam/demo:** Use one reference session, one copied session, and one missing pointer; navigate from task to session to graph evidence and back.
