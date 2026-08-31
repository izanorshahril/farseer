//! Where a cell is, and what purge destroys.
//!
//! `17 cell lifecycle` settled three verbs increasing in violence - archive
//! keeps the definition and the record, delete keeps the record, purge keeps
//! nothing - plus pause, which is a policy flag and never a process operation.
//!
//! Two rules from that ticket live in this file:
//!
//! - **Lifecycle is stored, not held in memory.** A deletion that lived only in
//!   the loaded registry would be undone by the next `reload`, which reads the
//!   same directory the deleted cell's file is still in. `16 local api surface`
//!   gave the API no path that edits a definition, and this does not become one:
//!   the file stays exactly as git has it.
//! - **Purge takes a scope.** A purge that can only destroy everything cannot
//!   serve a retention policy, and retention is the reason purge exists.

use farseer_core::CellId;

use crate::{Result, Store};

/// Where a cell is.
///
/// `Active` has no row. A fresh store and a fleet nobody has touched are then
/// the same thing, rather than something that needs seeding to be correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lifecycle {
    Active,
    /// The manager starts no new runs. In-flight runs continue: `17` refused to
    /// suspend a process, because suspending an agent mid-API-call corrupts the
    /// session it is holding, and cancelling is 300 microseconds and honest.
    Paused,
    /// Cannot be called. Definition and record intact, and it can come back.
    Archived,
    /// The running cell and the definition binding are gone; **the record
    /// stays**, per `02 record scope`. Reversible the way the file is: through
    /// git, which is where the definition lives.
    Deleted,
}

impl Lifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Archived => "archived",
            Self::Deleted => "deleted",
        }
    }

    /// Anything a store might hold, including a word written by a newer build.
    /// An unreadable state reads as `Active` rather than as a refusal to start:
    /// a fleet that will not load because one row is from the future is worse
    /// than a cell that is briefly callable again.
    pub fn parse(text: &str) -> Self {
        match text {
            "paused" => Self::Paused,
            "archived" => Self::Archived,
            "deleted" => Self::Deleted,
            _ => Self::Active,
        }
    }

    /// Whether a new run may start in this cell.
    pub fn accepts_work(self) -> bool {
        self == Self::Active
    }
}

/// What one purge destroyed. Every count is rows actually removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Purged {
    pub events: usize,
    pub runs: usize,
    pub memories: usize,
    /// The scope asked for, carried back so the tombstone and the API answer
    /// say the same thing without recomputing it.
    pub from_ts: Option<i64>,
    pub to_ts: Option<i64>,
}

impl Store {
    /// Every cell that is not simply active.
    pub fn cell_states(&self) -> Result<Vec<(CellId, Lifecycle)>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT cell_id, lifecycle FROM cell_state ORDER BY cell_id")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                CellId::new(r.get::<_, String>(0)?),
                Lifecycle::parse(&r.get::<_, String>(1)?),
            ))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn cell_state(&self, cell_id: &CellId) -> Result<Lifecycle> {
        let conn = self.conn();
        let found = conn
            .query_row(
                "SELECT lifecycle FROM cell_state WHERE cell_id = ?1",
                [cell_id.as_str()],
                |r| r.get::<_, String>(0),
            )
            .ok();
        Ok(found.map_or(Lifecycle::Active, |text| Lifecycle::parse(&text)))
    }

    /// Move a cell. Returning to `Active` removes the row rather than writing
    /// the word, so "not moved" has one representation instead of two.
    pub fn set_cell_state(&self, cell_id: &CellId, lifecycle: Lifecycle, ts: i64) -> Result<()> {
        let conn = self.conn();
        if lifecycle == Lifecycle::Active {
            conn.execute(
                "DELETE FROM cell_state WHERE cell_id = ?1",
                [cell_id.as_str()],
            )?;
            return Ok(());
        }
        conn.execute(
            "INSERT INTO cell_state (cell_id, lifecycle, ts) VALUES (?1, ?2, ?3)
             ON CONFLICT(cell_id) DO UPDATE SET lifecycle = ?2, ts = ?3",
            rusqlite::params![cell_id.as_str(), lifecycle.as_str(), ts],
        )?;
        Ok(())
    }

    /// Destroy part or all of a cell's record.
    ///
    /// `12 autonomy and deny list` made this **operator-only**: never a manager,
    /// never a worker. An agent that can destroy its own history makes the
    /// record worthless as evidence, and forge and destroy are two halves of one
    /// threat. `12` also classes purge as `irreversible`, so the gate on it is
    /// not lowerable.
    ///
    /// `from_ts`/`to_ts` bound the scope, inclusive of both ends, and `None`
    /// means unbounded on that side. Rows are selected by the timestamp each
    /// table already keeps - an event's `ts`, a run's `started_ts`, a memory's
    /// `ts` - rather than by a range over `seq`, because the operator asking is
    /// asking about dates and `seq` is farseer's cursor rather than a clock.
    ///
    /// Leaves permanent holes in `seq`. Per `09 store decision`, cursor reads
    /// tolerate gaps and nothing may infer a count from a delta; per `17`, a
    /// reader is told which holes are permanent by the `void` kind and by the
    /// tombstone the caller appends after this returns.
    pub fn purge_cell(
        &mut self,
        cell_id: &CellId,
        from_ts: Option<i64>,
        to_ts: Option<i64>,
    ) -> Result<Purged> {
        // `-∞` and `+∞` as SQL bounds, so one statement serves the scoped and
        // the unscoped purge rather than two spellings of the same deletion
        // drifting apart.
        let from = from_ts.unwrap_or(i64::MIN);
        let to = to_ts.unwrap_or(i64::MAX);
        let tx = self.conn.transaction()?;
        let events = tx.execute(
            "DELETE FROM events WHERE cell_id = ?1 AND ts BETWEEN ?2 AND ?3",
            rusqlite::params![cell_id.as_str(), from, to],
        )?;
        // Edges go before the rows they point at, so nothing dangles at any
        // point inside the transaction.
        tx.execute(
            "DELETE FROM supersedes
             WHERE new_id IN (SELECT memory_id FROM memories WHERE cell_id = ?1 AND ts BETWEEN ?2 AND ?3)
                OR old_id IN (SELECT memory_id FROM memories WHERE cell_id = ?1 AND ts BETWEEN ?2 AND ?3)",
            rusqlite::params![cell_id.as_str(), from, to],
        )?;
        tx.execute(
            "DELETE FROM consulted
             WHERE memory_id IN (SELECT memory_id FROM memories WHERE cell_id = ?1 AND ts BETWEEN ?2 AND ?3)
                OR run_id IN (SELECT run_id FROM runs WHERE cell_id = ?1 AND started_ts BETWEEN ?2 AND ?3)",
            rusqlite::params![cell_id.as_str(), from, to],
        )?;
        tx.execute(
            "DELETE FROM rescoped_from
             WHERE run_id IN (SELECT run_id FROM runs WHERE cell_id = ?1 AND started_ts BETWEEN ?2 AND ?3)
                OR parent IN (SELECT run_id FROM runs WHERE cell_id = ?1 AND started_ts BETWEEN ?2 AND ?3)",
            rusqlite::params![cell_id.as_str(), from, to],
        )?;
        let memories = tx.execute(
            "DELETE FROM memories WHERE cell_id = ?1 AND ts BETWEEN ?2 AND ?3",
            rusqlite::params![cell_id.as_str(), from, to],
        )?;
        // The runs go too. Purge is not delete: `02 record scope` section 7
        // keeps the record when a *cell* is deleted, but this verb exists for
        // content that must not exist, and leaving the cost and intervention
        // rows behind would have the analytics still reporting on what was
        // supposedly destroyed.
        let runs = tx.execute(
            "DELETE FROM runs WHERE cell_id = ?1 AND started_ts BETWEEN ?2 AND ?3",
            rusqlite::params![cell_id.as_str(), from, to],
        )?;
        tx.commit()?;
        Ok(Purged {
            events,
            runs,
            memories,
            from_ts,
            to_ts,
        })
    }
}
