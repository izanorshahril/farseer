//! The record: **one physical append-only log, with cell-scoped visibility.**
//!
//! `02` settled that storage and visibility are not the same thing, which is
//! what dissolved the apparent conflict between `BRIEF.md` and `ARCHITECTURE.md`.
//! `09` then benched the substrate and chose SQLite outright.
//!
//! Two rules this crate enforces rather than documents:
//!
//! - **Agents never append events.** `02` section 8: an agent that can forge
//!   events can rewrite its own history. Agents write *memory*, which is marked
//!   as a claim, and that path is [`Store::write_memory`].
//! - **Scrub on the way in.** Never at read time, because that leaves the
//!   secrets on disk and one query bug away from exposure.

use rusqlite::{Connection, OptionalExtension, params_from_iter};
use std::path::Path;

use farseer_core::{
    Actor, CellId, Event, EventId, EventKind, NewEvent, RunId, Seq, scrub::scrub_value,
};

mod analytics;
mod memory;
mod schema;
mod ui_state;

pub use analytics::{CostRow, InterventionRow, LessonRow, ReworkRow};
pub use farseer_core::MemoryId;
pub use memory::{MemoryCaps, MemoryClaim, MemoryScope, NewMemory, Promotion};
pub use ui_state::UI_STATE_CAP_BYTES;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(
        "memory tier `{tier}` for cell `{cell_id}` holds {used} of {cap} characters; \
         this write needs {wanted} more. Consolidate or retract first."
    )]
    MemoryCapExceeded {
        tier: &'static str,
        cell_id: String,
        used: usize,
        cap: usize,
        wanted: usize,
    },
    #[error("promoting to the global tier is gated on the operator, per `25`")]
    GlobalPromotionNeedsOperator,
    #[error("no memory claim with id {0}")]
    NoSuchMemory(MemoryId),
    #[error("ui state for `{key}` is {size} bytes, over the {cap} byte cap")]
    UiStateTooLarge {
        key: String,
        size: usize,
        cap: usize,
    },
    #[error("record holds an unreadable {field}: {value}")]
    Corrupt { field: &'static str, value: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Which slice of the log a reader wants. `16` chose one stream endpoint scoped
/// **server-side**, rather than a firehose every client reimplements filtering
/// for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanFilter {
    pub cell_id: Option<CellId>,
    pub run_id: Option<RunId>,
}

impl ScanFilter {
    pub fn cell(cell_id: CellId) -> Self {
        Self {
            cell_id: Some(cell_id),
            ..Self::default()
        }
    }

    pub fn run(run_id: RunId) -> Self {
        Self {
            run_id: Some(run_id),
            ..Self::default()
        }
    }
}

/// The one owning writer.
///
/// `09` asked whether single-writer holds under a realistic worker fleet and
/// found it **holds by construction**: workers emit events to the runtime, and
/// the runtime writes. Many producers into one process, one process into one
/// writer, which is exactly what SQLite WAL wants.
pub struct Store {
    conn: Connection,
    caps: MemoryCaps,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    /// For tests and for a runtime asked to keep nothing.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch(schema::PRAGMAS)?;
        conn.execute_batch(schema::SCHEMA)?;
        Ok(Self {
            conn,
            caps: MemoryCaps::default(),
        })
    }

    pub fn with_memory_caps(mut self, caps: MemoryCaps) -> Self {
        self.caps = caps;
        self
    }

    pub fn memory_caps(&self) -> &MemoryCaps {
        &self.caps
    }

    /// Append one observed event and return its cursor position.
    ///
    /// The payload is scrubbed here, on the way in.
    pub fn append(&self, event: &NewEvent) -> Result<Seq> {
        let payload = serde_json::to_string(&scrub_value(&event.payload))?;
        self.conn.execute(
            "INSERT INTO events (event_id, ts, cell_id, run_id, kind, actor, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                &event.event_id.as_bytes()[..],
                event.ts,
                event.cell_id.as_str(),
                &event.run_id.as_bytes()[..],
                event.kind.as_str(),
                event.actor.as_str(),
                payload,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Append many in one transaction. `09` measured this at ~308k events/sec
    /// at 2M rows, against ~23us p50 for one event one commit.
    pub fn append_batch(&mut self, events: &[NewEvent]) -> Result<Vec<Seq>> {
        let tx = self.conn.transaction()?;
        let mut seqs = Vec::with_capacity(events.len());
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO events (event_id, ts, cell_id, run_id, kind, actor, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for event in events {
                let payload = serde_json::to_string(&scrub_value(&event.payload))?;
                stmt.execute(rusqlite::params![
                    &event.event_id.as_bytes()[..],
                    event.ts,
                    event.cell_id.as_str(),
                    &event.run_id.as_bytes()[..],
                    event.kind.as_str(),
                    event.actor.as_str(),
                    payload,
                ])?;
                seqs.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(seqs)
    }

    /// The cursor read `16` and `07` both depend on: everything after `since`,
    /// in order.
    ///
    /// `since` is exclusive, so a client passing back the last `seq` it saw gets
    /// no gap and no duplicate. That is what makes "attach to a running worker"
    /// and "replay a dead session" the same call with a different cursor.
    pub fn scan(&self, since: Seq, limit: usize, filter: &ScanFilter) -> Result<Vec<Event>> {
        let mut sql = String::from(
            "SELECT seq, event_id, ts, cell_id, run_id, kind, actor, payload
             FROM events WHERE seq > ?1",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(since)];
        if let Some(cell_id) = &filter.cell_id {
            params.push(Box::new(cell_id.as_str().to_string()));
            sql.push_str(&format!(" AND cell_id = ?{}", params.len()));
        }
        if let Some(run_id) = &filter.run_id {
            params.push(Box::new(run_id.as_bytes().to_vec()));
            sql.push_str(&format!(" AND run_id = ?{}", params.len()));
        }
        params.push(Box::new(limit as i64));
        sql.push_str(&format!(" ORDER BY seq LIMIT ?{}", params.len()));

        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(params.iter().map(|p| p.as_ref())), |row| {
            Ok((
                row.get::<_, Seq>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;

        let mut events = Vec::new();
        for row in rows {
            let (seq, event_id, ts, cell_id, run_id, kind, actor, payload) = row?;
            events.push(Event {
                seq,
                event_id: EventId::from_bytes(uuid_bytes(&event_id, "event_id")?),
                ts,
                cell_id: CellId::new(cell_id),
                run_id: RunId::from_bytes(uuid_bytes(&run_id, "run_id")?),
                kind: EventKind::new(kind),
                actor: actor.parse::<Actor>().map_err(|e| StoreError::Corrupt {
                    field: "actor",
                    value: e.0,
                })?,
                payload: serde_json::from_str(&payload)?,
            });
        }
        Ok(events)
    }

    /// The highest cursor position in the log, or 0 when it is empty.
    pub fn latest_seq(&self) -> Result<Seq> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?)
    }

    /// Destroy a cell's history.
    ///
    /// `12` made this **operator-only**: never a manager, never a worker. An
    /// agent that can destroy its own history makes the record worthless as
    /// evidence, and forge and destroy are two halves of one threat. `12` also
    /// classes purge as `irreversible`, so the gate on it is not lowerable.
    ///
    /// Leaves permanent holes in `seq`. Per `09`, cursor reads tolerate gaps and
    /// nothing may infer a count from a delta.
    pub fn purge_cell(&mut self, cell_id: &CellId) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute("DELETE FROM events WHERE cell_id = ?1", [cell_id.as_str()])?;
        // Edges go before the rows they point at, so nothing dangles at any
        // point inside the transaction.
        tx.execute(
            "DELETE FROM supersedes
             WHERE new_id IN (SELECT memory_id FROM memories WHERE cell_id = ?1)
                OR old_id IN (SELECT memory_id FROM memories WHERE cell_id = ?1)",
            [cell_id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM consulted
             WHERE memory_id IN (SELECT memory_id FROM memories WHERE cell_id = ?1)
                OR run_id IN (SELECT run_id FROM runs WHERE cell_id = ?1)",
            [cell_id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM rescoped_from
             WHERE run_id IN (SELECT run_id FROM runs WHERE cell_id = ?1)
                OR parent IN (SELECT run_id FROM runs WHERE cell_id = ?1)",
            [cell_id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM memories WHERE cell_id = ?1",
            [cell_id.as_str()],
        )?;
        // The runs go too. Purge is not delete: `02` section 7 keeps the record
        // when a *cell* is deleted, but this verb exists for content that must
        // not exist, and leaving the cost and intervention rows behind would
        // have the analytics still reporting on what was supposedly destroyed.
        tx.execute("DELETE FROM runs WHERE cell_id = ?1", [cell_id.as_str()])?;
        tx.commit()?;
        Ok(removed)
    }

    /// Record a run for `11`'s four questions. Deleting a cell does not delete
    /// this: `02` section 7 keeps the record when the cell goes, because a
    /// definition is a file in git and its history is not reversible.
    pub fn upsert_run(&self, run: &RunRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO runs
               (run_id, task_id, cell_id, runner, model, outcome, usd_micros, tokens,
                operator_touched, started_ts, finished_ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(run_id) DO UPDATE SET
               outcome = excluded.outcome,
               model = excluded.model,
               usd_micros = excluded.usd_micros,
               tokens = excluded.tokens,
               operator_touched = excluded.operator_touched,
               finished_ts = excluded.finished_ts",
            rusqlite::params![
                &run.run_id.as_bytes()[..],
                &run.task_id.as_bytes()[..],
                run.cell_id.as_str(),
                run.runner,
                run.model,
                run.outcome,
                run.usd_micros as i64,
                run.tokens as i64,
                run.operator_touched as i64,
                run.started_ts,
                run.finished_ts,
            ],
        )?;
        Ok(())
    }

    /// `run -> re-scoped-from -> run`, one of `11`'s two edge kinds.
    pub fn record_rescope(&self, run_id: RunId, parent: RunId) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO rescoped_from (run_id, parent) VALUES (?1, ?2)",
            rusqlite::params![&run_id.as_bytes()[..], &parent.as_bytes()[..]],
        )?;
        Ok(())
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

fn uuid_bytes(raw: &[u8], field: &'static str) -> Result<[u8; 16]> {
    raw.try_into().map_err(|_| StoreError::Corrupt {
        field,
        value: format!("{} bytes", raw.len()),
    })
}

/// A run, as `11` needs it: `cost`, `tokens`, `runner`, `model`, `cell_id`,
/// `outcome`, `ts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub run_id: RunId,
    pub task_id: farseer_core::TaskId,
    pub cell_id: CellId,
    pub runner: String,
    pub model: String,
    /// `None` while in flight.
    pub outcome: Option<String>,
    pub usd_micros: u64,
    pub tokens: u64,
    /// Set permanently once a human touched the run, per `07`. Provenance, not
    /// a reason to restrict what happens next.
    pub operator_touched: bool,
    pub started_ts: i64,
    pub finished_ts: Option<i64>,
}

/// Convenience for reading a row back, used by tests and by the API.
impl Store {
    pub fn run(&self, run_id: RunId) -> Result<Option<RunRow>> {
        let row = self
            .conn
            .query_row(
                "SELECT task_id, cell_id, runner, model, outcome, usd_micros, tokens,
                        operator_touched, started_ts, finished_ts
                 FROM runs WHERE run_id = ?1",
                [&run_id.as_bytes()[..]],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, Option<i64>>(9)?,
                    ))
                },
            )
            .optional()?;
        let Some(r) = row else { return Ok(None) };
        Ok(Some(RunRow {
            run_id,
            task_id: farseer_core::TaskId::from_bytes(uuid_bytes(&r.0, "task_id")?),
            cell_id: CellId::new(r.1),
            runner: r.2,
            model: r.3,
            outcome: r.4,
            usd_micros: r.5 as u64,
            tokens: r.6 as u64,
            operator_touched: r.7 != 0,
            started_ts: r.8,
            finished_ts: r.9,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use farseer_core::TaskId;
    use serde_json::json;

    fn event(store_cell: &str, run: RunId, kind: &str, ts: i64) -> NewEvent {
        NewEvent::new(
            CellId::new(store_cell),
            run,
            kind,
            Actor::Worker,
            ts,
            json!({"note": "ok"}),
        )
    }

    #[test]
    fn a_cursor_scan_returns_everything_after_the_cursor_in_order() {
        let store = Store::open_in_memory().unwrap();
        let run = RunId::new();
        for i in 0..10 {
            store.append(&event("zero", run, "tool_result", i)).unwrap();
        }
        let page = store.scan(0, 4, &ScanFilter::default()).unwrap();
        assert_eq!(page.len(), 4);
        assert_eq!(page[0].seq, 1);

        let next = store
            .scan(page[3].seq, 100, &ScanFilter::default())
            .unwrap();
        assert_eq!(next.len(), 6);
        assert_eq!(
            next[0].seq, 5,
            "no gap and no duplicate across the boundary"
        );
    }

    #[test]
    fn a_scan_is_scoped_server_side_by_cell_and_by_run() {
        let store = Store::open_in_memory().unwrap();
        let mine = RunId::new();
        let theirs = RunId::new();
        store
            .append(&event("zero", mine, "tool_result", 1))
            .unwrap();
        store
            .append(&event("social", theirs, "tool_result", 2))
            .unwrap();

        let zero = store
            .scan(0, 100, &ScanFilter::cell(CellId::new("zero")))
            .unwrap();
        assert_eq!(zero.len(), 1);
        assert_eq!(zero[0].cell_id.as_str(), "zero");

        let by_run = store.scan(0, 100, &ScanFilter::run(theirs)).unwrap();
        assert_eq!(by_run.len(), 1);
        assert_eq!(by_run[0].run_id, theirs);
    }

    #[test]
    fn an_event_survives_a_round_trip_with_every_field_intact() {
        let store = Store::open_in_memory().unwrap();
        let mut written = event("zero", RunId::new(), EventKind::CONTEXT_COMPACTED, 1_700);
        written.actor = Actor::System;
        written.payload = json!({"trigger": "auto", "tokens_before": 180_000});
        let seq = store.append(&written).unwrap();

        let read = store.scan(seq - 1, 1, &ScanFilter::default()).unwrap();
        let read = &read[0];
        assert_eq!(read.event_id, written.event_id);
        assert_eq!(read.ts, 1_700);
        assert_eq!(read.actor, Actor::System);
        assert_eq!(read.kind.as_str(), EventKind::CONTEXT_COMPACTED);
        assert_eq!(read.payload["tokens_before"], 180_000);
    }

    #[test]
    fn a_secret_in_a_payload_never_reaches_the_disk() {
        let store = Store::open_in_memory().unwrap();
        let mut e = event("zero", RunId::new(), "tool_call_started", 1);
        e.payload = json!({"command": "gh auth login --with-token ghp_ZzAa0011223344556677889900"});
        store.append(&e).unwrap();

        let raw: String = store
            .conn()
            .query_row("SELECT payload FROM events", [], |r| r.get(0))
            .unwrap();
        assert!(
            !raw.contains("ghp_"),
            "raw row still holds the token: {raw}"
        );
    }

    #[test]
    fn a_purge_takes_the_cells_runs_and_edges_with_it() {
        let mut store = Store::open_in_memory().unwrap();
        let run = RunId::new();
        store
            .upsert_run(&RunRow {
                run_id: run,
                task_id: TaskId::new(),
                cell_id: CellId::new("social"),
                runner: "codex".into(),
                model: "gpt".into(),
                outcome: Some("ok".into()),
                usd_micros: 500_000,
                tokens: 10,
                operator_touched: false,
                started_ts: 1,
                finished_ts: Some(2),
            })
            .unwrap();

        store.purge_cell(&CellId::new("social")).unwrap();

        assert_eq!(store.run(run).unwrap(), None);
        assert!(
            store.cost_by_runner_and_model().unwrap().is_empty(),
            "analytics still reports spend on what was destroyed"
        );
        assert!(store.intervention_rate_by_cell().unwrap().is_empty());
    }

    #[test]
    fn a_purge_leaves_permanent_holes_that_a_cursor_read_walks_over() {
        let mut store = Store::open_in_memory().unwrap();
        let run = RunId::new();
        store.append(&event("zero", run, "a", 1)).unwrap();
        store.append(&event("social", run, "b", 2)).unwrap();
        store.append(&event("zero", run, "c", 3)).unwrap();

        assert_eq!(store.purge_cell(&CellId::new("zero")).unwrap(), 2);

        let all = store.scan(0, 100, &ScanFilter::default()).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].seq, 2, "seq is not contiguous after a purge");
        assert_eq!(store.latest_seq().unwrap(), 2);
    }

    #[test]
    fn a_batch_append_assigns_consecutive_cursor_positions() {
        let mut store = Store::open_in_memory().unwrap();
        let run = RunId::new();
        let batch: Vec<_> = (0..100)
            .map(|i| event("zero", run, "tool_result", i))
            .collect();
        let seqs = store.append_batch(&batch).unwrap();
        assert_eq!(seqs.first(), Some(&1));
        assert_eq!(seqs.last(), Some(&100));
    }

    #[test]
    fn a_run_row_round_trips_and_updates_in_place() {
        let store = Store::open_in_memory().unwrap();
        let run_id = RunId::new();
        let mut row = RunRow {
            run_id,
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            runner: "claude-code".into(),
            model: "opus-5".into(),
            outcome: None,
            usd_micros: 0,
            tokens: 0,
            operator_touched: false,
            started_ts: 100,
            finished_ts: None,
        };
        store.upsert_run(&row).unwrap();
        row.outcome = Some("ok".into());
        row.usd_micros = 320_000;
        row.operator_touched = true;
        row.finished_ts = Some(200);
        store.upsert_run(&row).unwrap();

        assert_eq!(store.run(run_id).unwrap().as_ref(), Some(&row));
    }

    #[test]
    fn a_store_reopens_onto_the_same_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.sqlite3");
        let run = RunId::new();
        {
            let store = Store::open(&path).unwrap();
            store.append(&event("zero", run, "a", 1)).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.latest_seq().unwrap(), 1);
    }
}
