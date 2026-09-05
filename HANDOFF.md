# Session handoff

## State

Branch: `feat/acp-runner`.

Follow-up commit: `Complete durable work explorer`, on top of `ec67ee6`.

The reviewed follow-up and this handoff are committed together.

The branch is two commits ahead of `origin/feat/acp-runner` and the working tree is clean.

Run `git status --short --branch` before doing anything else.

## Completed behavior

- Durable conversations group tasks, and instructions create one task plus one root manager run.
- Task transitions are validated and retain actor, reason, and timestamp.
- Rerun, rescope, continuation, delegation, and cell-call parent edges are persisted.
- Runs retain multiple provider-owned session identifiers and optional runner-reported transcript pointers.
- Transcript custody supports `reference`, `copy`, and `copy-plus-index`, with raw bytes outside SQLite and scrubbed rebuildable search/similarity projections.
- The version-eight canvas defaults to Conversation, Work, Fleet, and Capacity, shares conversation/task/run/project/manager context, and keeps Settings in a top-bar popover.
- Work includes global and project boards, conversations, completed work, full-canvas expansion, complete observed topology, transcript nodes, and separately styled similarity edges.

## Follow-up fixes currently in the worktree

- Fixed repeated transcript content replacing another run's attachment by changing the association key to `(digest, run_id)`.
- Added a transactional migration for the earlier digest-only schema, including the legacy index path and a regression test.
- Added `SessionInfo.log_pointer`, emitted it on `session_started`, and persisted it when an adapter reports one.
- Kept a frozen legacy decoder for immutable pre-ticket session events that lack `session_kind`.
- Removed the duplicated TypeScript task-transition matrix by returning `allowed_transitions` from the task API.
- Added project-board filtering, task-to-project graph edges, transcript graph nodes, derived similarity SVG edges, and removal of the 48-node truncation.
- Added decision citations for the hash-TF/cosine projection, fixed Markdown sentence wrapping, ignored `.vite/`, removed committed Vite cache files, and made the Rust CI job build `ui/dist` before Tauri compilation.

## Verification already completed

- `cargo fmt --all` completed.
- `cargo test --workspace` passed with 374 tests and 25 intentionally ignored live-runner tests.
- `cargo clippy --workspace --all-targets` completed silently.
- `bun run --cwd ui test` passed 6 tests with 17 assertions.
- `bun run --cwd ui check` passed.
- `bun run --cwd ui build` passed and compiled 3 of 3 authored widgets.
- A live isolated runtime returned 200 for conversations, tasks, and the work graph.
- Browser verification covered the four-widget default, conversation creation/selection, Settings popover, Work expansion, project-board selector, and a populated graph with two transcript nodes and one visible derived edge.
- Parallel standards and ticket-40 reviews found no remaining concrete blocker after the follow-up fixes.
- Temporary databases, screenshots, and isolated Cargo target directories were removed.

## External gate blocker

No-mistakes run `01M1Q9ZCHCV78YHA2FX5MB4TX3` failed before its own review because Claude Code reported: `Your organization has disabled Claude subscription access for Claude Code`.

No-mistakes v1.48.0 recorded preserved head `f273e3a9` but left the gate branch at `ec67ee6`, so both recovery commands refused with `blocked_recover_gate_diverged`.

The exact preserved commit was verified in the gate object store, the old gate ref was retained at `refs/no-mistakes/manual-recovery/01M1Q9ZCHCV78YHA2FX5MB4TX3/gate-before`, and the guarded `no-mistakes axi sync --recover --keep-local` then returned custody without touching the dirty worktree.

The inconsistency was reported through `xd://report_issue`.

Updating to no-mistakes v1.64.0 is currently blocked by unrelated active pipeline `01M07734GPSERZED3GZTQ5PV2X` on `fix/windows-herdr-socket`.

Do not force-update or disrupt that unrelated run.

The global no-mistakes configuration now sets `agent: codex`; `no-mistakes doctor` reports `codex is runnable`.

## Remaining work

1. Rerun the full no-mistakes pipeline with the original intent using Codex.
2. Drive any gate findings, push, PR, and CI to a terminal successful outcome.
3. After the unrelated pipeline ends, update no-mistakes from v1.48.0 without using `--force`.

## Original gate intent

`Iterate and complete the durable work and session explorer plus the approved operator UX safely. Implement durable conversations, validated task boards, explicit run topology, multiple provider-owned harness session identifiers, transcript custody with scrubbed rebuildable search and similarity projections, ordered cell-zero manager candidates, the version-eight Conversation/Work/Fleet/Capacity canvas, shared composer context, and a settings popover. Preserve top-manager-only routing, SQLite as truth, raw transcripts outside the event log, no plugin ABI, Windows-native behavior, and update decision records and documentation.`

State: implementation, local validation, gate custody recovery, Codex configuration, and the follow-up commit are complete; pipeline, push, PR, and CI remain.
