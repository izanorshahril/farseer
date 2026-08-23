//! The schema `09` benched, plus the tables `02` and `24` require.
//!
//! `09`'s verdict was "SQLite. Not close." - a cursor scan at 100x the target
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

-- `11` cut the analytics surface to three entities and two edge kinds.
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
-- Retraction and consolidation are appends: `25` resolves latest-wins over a
-- superseding tombstone, because `02` made the record append-only.
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
-- Supersession is how `25` retracts and consolidates: an append, never a
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

-- `24`: an opaque blob farseer never parses. Mutable, last-write-wins, no
-- `seq`, no scrub, and no event emitted on write - a cursor drag is not history.
CREATE TABLE IF NOT EXISTS ui_state (
    key   TEXT PRIMARY KEY,
    blob  BLOB NOT NULL,
    ts    INTEGER NOT NULL
);
"#;

/// Pragmas applied on every open.
///
/// WAL is what gives `09`'s measured result: one writer and many concurrent
/// readers, with a reader degrading roughly 13% at p99 while a writer appends
/// continuously. `16` requires that a slow client never slows a worker, and this
/// is the inverse holding too.
pub const PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";
