//! The schema `09 store decision` benched, plus the tables `02 record scope` and `24 ui state persistence` require.
//!
//! `09 store decision`'s verdict was "SQLite. Not close." - a cursor scan at 100x the target
//! scale runs at p99 425us, and a 10x increase in data moved that from 299us.
//! The hot path is effectively scale-invariant because `seq` is the rowid.

/// Every statement needed to bring an empty database up to date.
///
/// Written to be re-runnable, so opening an existing store is the same code
/// path as creating one.
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    seq       INTEGER PRIMARY KEY,   -- the cursor; rowid, so range scans are b-tree walks
    event_id  BLOB NOT NULL UNIQUE,  -- UUIDv7, the portable identity
    ts        INTEGER NOT NULL,
    cell_id   TEXT NOT NULL,
    run_id    BLOB NOT NULL,
    kind      TEXT NOT NULL,
    actor     TEXT NOT NULL,         -- manager / worker / operator / system
    payload   TEXT NOT NULL          -- JSON; SQLite can index into it if a query ever needs to
);
CREATE INDEX IF NOT EXISTS events_run ON events(run_id, seq);
CREATE INDEX IF NOT EXISTS events_cell ON events(cell_id, seq);

-- `11 analytics questions` cut the analytics surface to three entities and two edge kinds.
CREATE TABLE IF NOT EXISTS runs (
    run_id           BLOB PRIMARY KEY,
    task_id          BLOB NOT NULL,
    cell_id          TEXT NOT NULL,
    runner           TEXT NOT NULL,
    model            TEXT NOT NULL DEFAULT '',
    outcome          TEXT,           -- NULL while in flight
    usd_micros       INTEGER NOT NULL DEFAULT 0,
    tokens           INTEGER NOT NULL DEFAULT 0,
    operator_touched INTEGER NOT NULL DEFAULT 0,
    started_ts       INTEGER NOT NULL,
    finished_ts      INTEGER
);
CREATE INDEX IF NOT EXISTS runs_task ON runs(task_id);
CREATE INDEX IF NOT EXISTS runs_cell ON runs(cell_id);

-- Memory is written by agents, as claims, never as observations.
-- Retraction and consolidation are appends: `25 memory lifecycle` resolves latest-wins over a
-- superseding tombstone, because `02 record scope` made the record append-only.
CREATE TABLE IF NOT EXISTS memories (
    memory_id  BLOB PRIMARY KEY,
    tier       TEXT NOT NULL,        -- global / cell_local / run_local
    cell_id    TEXT NOT NULL,
    run_id     BLOB,                 -- set for run_local
    body       TEXT NOT NULL,
    tombstone  INTEGER NOT NULL DEFAULT 0,
    ts         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS memories_scope ON memories(tier, cell_id);
-- Supersession is how `25 memory lifecycle` retracts and consolidates: an append, never a
-- removal, so the history of what was believed and when survives.
CREATE TABLE IF NOT EXISTS supersedes (
    new_id BLOB NOT NULL,
    old_id BLOB NOT NULL,
    PRIMARY KEY (new_id, old_id)
);
CREATE INDEX IF NOT EXISTS supersedes_old ON supersedes(old_id);

-- Edge kind 1: run -> consulted -> memory.
CREATE TABLE IF NOT EXISTS consulted (
    run_id    BLOB NOT NULL,
    memory_id BLOB NOT NULL,
    ts        INTEGER NOT NULL,
    PRIMARY KEY (run_id, memory_id)
);

-- Edge kind 2: run -> re-scoped-from -> run.
CREATE TABLE IF NOT EXISTS rescoped_from (
    run_id  BLOB PRIMARY KEY,
    parent  BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS rescoped_parent ON rescoped_from(parent);

-- `40 work model and session explorer`: durable operator work.
CREATE TABLE IF NOT EXISTS conversations (
    conversation_id BLOB PRIMARY KEY,
    title            TEXT NOT NULL,
    project_path     TEXT,
    manager_runner   TEXT,
    created_ts       INTEGER NOT NULL,
    updated_ts       INTEGER NOT NULL,
    archived_ts      INTEGER
);
CREATE INDEX IF NOT EXISTS conversations_updated ON conversations(updated_ts DESC);

CREATE TABLE IF NOT EXISTS tasks (
    task_id         BLOB PRIMARY KEY,
    conversation_id BLOB NOT NULL,
    goal            TEXT NOT NULL,
    title           TEXT NOT NULL,
    project_path    TEXT,
    state           TEXT NOT NULL,
    priority        INTEGER NOT NULL DEFAULT 0,
    created_ts      INTEGER NOT NULL,
    updated_ts      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS tasks_conversation ON tasks(conversation_id, updated_ts DESC);
CREATE INDEX IF NOT EXISTS tasks_project ON tasks(project_path, state, updated_ts DESC);

CREATE TABLE IF NOT EXISTS task_transitions (
    seq      INTEGER PRIMARY KEY,
    task_id  BLOB NOT NULL,
    from_state TEXT NOT NULL,
    to_state TEXT NOT NULL,
    actor    TEXT NOT NULL,
    reason   TEXT NOT NULL,
    ts       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS task_transitions_task ON task_transitions(task_id, seq);

-- A run may have one supervised parent and several provider-owned sessions.
CREATE TABLE IF NOT EXISTS run_parents (
    run_id BLOB PRIMARY KEY,
    parent BLOB NOT NULL,
    kind   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS run_parents_parent ON run_parents(parent);

CREATE TABLE IF NOT EXISTS harness_sessions (
    run_id          BLOB NOT NULL,
    identifier_kind TEXT NOT NULL,
    identifier      TEXT NOT NULL,
    log_pointer     TEXT,
    observed_ts     INTEGER NOT NULL,
    PRIMARY KEY (run_id, identifier_kind, identifier)
);

-- Raw transcript bytes live at stored_path, never in SQLite or the event log.
CREATE TABLE IF NOT EXISTS transcript_attachments (
    digest       TEXT PRIMARY KEY,
    run_id       BLOB NOT NULL,
    custody      TEXT NOT NULL,
    source       TEXT NOT NULL,
    stored_path  TEXT,
    created_ts   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS transcript_attachments_run ON transcript_attachments(run_id);

-- Scrubbed rebuildable projections. Neither table is canonical evidence.
CREATE TABLE IF NOT EXISTS transcript_index (
    digest             TEXT PRIMARY KEY,
    body               TEXT NOT NULL,
    redaction_version  TEXT NOT NULL,
    projection_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS similarity_edges (
    left_digest        TEXT NOT NULL,
    right_digest       TEXT NOT NULL,
    score              REAL NOT NULL,
    embedding_model    TEXT NOT NULL,
    dimensions         INTEGER NOT NULL,
    distance_metric    TEXT NOT NULL,
    redaction_version  TEXT NOT NULL,
    projection_version TEXT NOT NULL,
    source_digest      TEXT NOT NULL,
    evidence           TEXT NOT NULL,
    PRIMARY KEY (left_digest, right_digest, projection_version)
);

-- `24 ui state persistence`: an opaque blob farseer never parses. Mutable, last-write-wins, no
-- `seq`, no scrub, and no event emitted on write - a cursor drag is not history.
-- `39 what an installed farseer points at`: the directories the operator has
-- authorized farseer to work inside. A **root**, not a project - projects are
-- the directories found inside a root, and are never registered, so this list
-- cannot drift from the disk. Farseer may create a project inside a root and
-- may never create a root: creating one is the act of granting access, and an
-- application that can widen its own authorization has none.
--
-- The path is stored canonicalized, because a check against the string a caller
-- sent is not a check.
CREATE TABLE IF NOT EXISTS roots (
    path  TEXT PRIMARY KEY,
    ts    INTEGER NOT NULL
);

-- `17 cell lifecycle`: where a cell is, when it is not simply active.
--
-- Kept here rather than in the definition file, because the file is git's and
-- `16 local api surface` gave the API no edit path to it. Pausing a cell is an
-- operator act on a running system, not a change to what the cell is - and a
-- deletion that lived only in memory would be undone by the next reload.
--
-- A row exists only for a cell that has moved. Absent means active, so a fresh
-- store and a fleet nobody has touched are the same thing.
CREATE TABLE IF NOT EXISTS cell_state (
    cell_id   TEXT PRIMARY KEY,
    lifecycle TEXT NOT NULL,        -- paused / archived / deleted
    ts        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ui_state (
    key   TEXT PRIMARY KEY,
    blob  BLOB NOT NULL,
    ts    INTEGER NOT NULL
);
"#;

/// Pragmas applied on every open.
///
/// WAL is what gives `09 store decision`'s measured result: one writer and many concurrent
/// readers, with a reader degrading roughly 13% at p99 while a writer appends
/// continuously. `16 local api surface` requires that a slow client never slows a worker, and this
/// is the inverse holding too.
pub const PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";
