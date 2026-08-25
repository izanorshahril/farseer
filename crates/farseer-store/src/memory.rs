//! Memory: what agents write, as **claims** rather than observations.
//!
//! `25 memory lifecycle` settled the lifecycle by researching Hermes Agent, which has no tiers at
//! all: one flat store with a hard character cap, and a write that would exceed
//! it **returns an error rather than silently dropping anything**. Scarcity is
//! the feature - a wrong lesson cannot accumulate, because it competes for a
//! small fixed space with better ones and must justify itself on every write.
//!
//! Farseer keeps `02 record scope`'s three tiers and adds that cap per tier per cell. The
//! number is an implementation choice; the principle is what transfers.

use rusqlite::{OptionalExtension, params_from_iter};
use std::collections::BTreeSet;

use farseer_core::{CellDefinition, CellId, MemoryId, MemoryTier, RunId, scrub::scrub};

use crate::{Result, Store, StoreError};

/// How many characters a tier may hold, per cell.
///
/// `25 memory lifecycle`: **do not copy Hermes' number.** 2,200 characters is a single-agent
/// assistant's whole budget; farseer's cap is per tier per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryCaps {
    pub global: usize,
    pub cell_local: usize,
    pub run_local: usize,
}

impl Default for MemoryCaps {
    fn default() -> Self {
        Self {
            global: 8_000,
            cell_local: 4_000,
            run_local: 2_000,
        }
    }
}

impl MemoryCaps {
    pub fn for_tier(&self, tier: MemoryTier) -> usize {
        match tier {
            MemoryTier::Global => self.global,
            MemoryTier::CellLocal => self.cell_local,
            MemoryTier::RunLocal => self.run_local,
        }
    }
}

/// A claim, live in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryClaim {
    pub memory_id: MemoryId,
    pub tier: MemoryTier,
    pub cell_id: CellId,
    pub run_id: Option<RunId>,
    pub body: String,
    pub ts: i64,
}

/// A claim on its way in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMemory {
    /// `25 memory lifecycle`: **cell-local is the default write tier.** A run-local default would
    /// mean memory does nothing until promotion is built, and would give `11 analytics questions`'s
    /// fourth question an n of 1 forever.
    pub tier: MemoryTier,
    pub cell_id: CellId,
    pub run_id: Option<RunId>,
    pub body: String,
    /// The claims this one replaces. Consolidating several entries into a denser
    /// one is an append too, per `25 memory lifecycle`, never an edit in place.
    pub supersedes: Vec<MemoryId>,
    pub ts: i64,
}

impl NewMemory {
    /// The default: a lesson for the cell that learned it.
    pub fn cell_local(cell_id: CellId, body: impl Into<String>, ts: i64) -> Self {
        Self {
            tier: MemoryTier::CellLocal,
            cell_id,
            run_id: None,
            body: body.into(),
            supersedes: Vec::new(),
            ts,
        }
    }
}

/// Who authorised moving a claim up a tier.
///
/// `25 memory lifecycle` tiers promotion by blast radius: **the manager decides for `cell-local`,
/// the operator gates `global`.** Global is the only tier that crosses cells, so
/// it is the only one that needs a human. An agent that could promote its own
/// lesson to global would poison every cell from inside a single run, which is
/// the same shape as the reasoning that withheld raw event append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotion {
    ManagerDecided,
    OperatorApproved,
}

/// What a reader may see.
///
/// `02 record scope`: `global` is readable by every cell and needs no declaration; cross-cell
/// reads beyond it are opt-in via the **reader's** own definition, never blanket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryScope {
    pub cell_id: CellId,
    pub run_id: Option<RunId>,
    pub also_read: BTreeSet<CellId>,
}

impl MemoryScope {
    pub fn new(cell_id: CellId) -> Self {
        Self {
            cell_id,
            run_id: None,
            also_read: BTreeSet::new(),
        }
    }

    pub fn in_run(mut self, run_id: RunId) -> Self {
        self.run_id = Some(run_id);
        self
    }

    pub fn from_definition(definition: &CellDefinition, run_id: Option<RunId>) -> Self {
        Self {
            cell_id: definition.cell_id.clone(),
            run_id,
            also_read: definition.record_scope.also_read.clone(),
        }
    }

    /// Every cell whose `cell_local` tier this reader may see.
    fn readable_cells(&self) -> Vec<String> {
        let mut cells: Vec<String> = self.also_read.iter().map(|c| c.to_string()).collect();
        cells.push(self.cell_id.to_string());
        cells
    }
}

/// A claim is live when nothing supersedes it and it is not itself a tombstone.
const LIVE: &str = "tombstone = 0
     AND NOT EXISTS (SELECT 1 FROM supersedes s WHERE s.old_id = memories.memory_id)";

impl Store {
    /// Write a claim, or refuse because the tier is full.
    ///
    /// Refusing is the mechanism, not a limitation: it forces the agent to
    /// consolidate or retract in the same turn.
    pub fn write_memory(&mut self, memory: &NewMemory) -> Result<MemoryId> {
        let body = scrub(&memory.body);
        let cap = self.caps.for_tier(memory.tier);
        // Superseded claims free their space in the same write, so consolidation
        // is always possible even from a full tier.
        let used = self.tier_usage(memory.tier, &memory.cell_id, &memory.supersedes)?;
        if used + body.chars().count() > cap {
            return Err(StoreError::MemoryCapExceeded {
                tier: memory.tier.as_str(),
                cell_id: memory.cell_id.to_string(),
                used,
                cap,
                wanted: body.chars().count(),
            });
        }

        let memory_id = MemoryId::new();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO memories (memory_id, tier, cell_id, run_id, body, tombstone, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
            rusqlite::params![
                &memory_id.as_bytes()[..],
                memory.tier.as_str(),
                memory.cell_id.as_str(),
                memory.run_id.map(|r| r.as_bytes().to_vec()),
                body,
                memory.ts,
            ],
        )?;
        for old in &memory.supersedes {
            tx.execute(
                "INSERT OR IGNORE INTO supersedes (new_id, old_id) VALUES (?1, ?2)",
                rusqlite::params![&memory_id.as_bytes()[..], &old.as_bytes()[..]],
            )?;
        }
        tx.commit()?;
        Ok(memory_id)
    }

    /// Retract a claim that turned out to be wrong.
    ///
    /// `25 memory lifecycle`: a superseding tombstone, never a removal. Farseer diverges from
    /// Hermes here and the divergence is forced - Hermes corrects with a
    /// destructive `replace` and `remove`, and `02 record scope` made farseer append-only.
    ///
    /// A merely wrong lesson is not a privacy problem, so this is not
    /// [`Store::purge_cell`].
    pub fn retract_memory(&mut self, memory_id: MemoryId, ts: i64) -> Result<MemoryId> {
        let existing = self
            .memory(memory_id)?
            .ok_or(StoreError::NoSuchMemory(memory_id))?;
        let tombstone_id = MemoryId::new();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO memories (memory_id, tier, cell_id, run_id, body, tombstone, ts)
             VALUES (?1, ?2, ?3, ?4, '', 1, ?5)",
            rusqlite::params![
                &tombstone_id.as_bytes()[..],
                existing.tier.as_str(),
                existing.cell_id.as_str(),
                existing.run_id.map(|r| r.as_bytes().to_vec()),
                ts,
            ],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO supersedes (new_id, old_id) VALUES (?1, ?2)",
            rusqlite::params![&tombstone_id.as_bytes()[..], &memory_id.as_bytes()[..]],
        )?;
        tx.commit()?;
        Ok(tombstone_id)
    }

    /// Move a claim to a wider tier, appending rather than editing.
    pub fn promote_memory(
        &mut self,
        memory_id: MemoryId,
        to: MemoryTier,
        by: Promotion,
        ts: i64,
    ) -> Result<MemoryId> {
        if to == MemoryTier::Global && by != Promotion::OperatorApproved {
            return Err(StoreError::GlobalPromotionNeedsOperator);
        }
        let existing = self
            .memory(memory_id)?
            .ok_or(StoreError::NoSuchMemory(memory_id))?;
        self.write_memory(&NewMemory {
            tier: to,
            cell_id: existing.cell_id,
            run_id: if to == MemoryTier::RunLocal {
                existing.run_id
            } else {
                None
            },
            body: existing.body,
            supersedes: vec![memory_id],
            ts,
        })
    }

    /// Every live claim this scope may read.
    pub fn read_memory(&self, scope: &MemoryScope) -> Result<Vec<MemoryClaim>> {
        let cells = scope.readable_cells();
        let placeholders: Vec<String> = (1..=cells.len()).map(|i| format!("?{i}")).collect();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = cells
            .into_iter()
            .map(|c| Box::new(c) as Box<dyn rusqlite::ToSql>)
            .collect();

        let mut sql = format!(
            "SELECT memory_id, tier, cell_id, run_id, body, ts FROM memories
             WHERE {LIVE} AND (
                 tier = 'global'
                 OR (tier = 'cell_local' AND cell_id IN ({}))",
            placeholders.join(", ")
        );
        match scope.run_id {
            Some(run_id) => {
                params.push(Box::new(run_id.as_bytes().to_vec()));
                sql.push_str(&format!(
                    " OR (tier = 'run_local' AND run_id = ?{})",
                    params.len()
                ));
            }
            None => sql.push_str(" OR 0"),
        }
        sql.push_str(") ORDER BY ts, memory_id");

        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(params.iter().map(|p| p.as_ref())), |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut claims = Vec::new();
        for row in rows {
            claims.push(claim_from(row?)?);
        }
        Ok(claims)
    }

    /// One claim by id, live or not.
    pub fn memory(&self, memory_id: MemoryId) -> Result<Option<MemoryClaim>> {
        let row = self
            .conn
            .query_row(
                "SELECT memory_id, tier, cell_id, run_id, body, ts FROM memories
                 WHERE memory_id = ?1",
                [&memory_id.as_bytes()[..]],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<Vec<u8>>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        row.map(claim_from).transpose()
    }

    /// `run -> consulted -> memory`, the edge `11 analytics questions`'s fourth question runs on.
    pub fn record_consulted(&self, run_id: RunId, memory_id: MemoryId, ts: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO consulted (run_id, memory_id, ts) VALUES (?1, ?2, ?3)",
            rusqlite::params![&run_id.as_bytes()[..], &memory_id.as_bytes()[..], ts],
        )?;
        Ok(())
    }

    /// Characters currently held in a tier, ignoring anything a pending write is
    /// about to supersede.
    fn tier_usage(
        &self,
        tier: MemoryTier,
        cell_id: &CellId,
        freed_by: &[MemoryId],
    ) -> Result<usize> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT memory_id, body FROM memories
             WHERE {LIVE} AND tier = ?1 AND cell_id = ?2"
        ))?;
        let rows = stmt.query_map(rusqlite::params![tier.as_str(), cell_id.as_str()], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?))
        })?;

        let freed: BTreeSet<Vec<u8>> = freed_by.iter().map(|m| m.as_bytes().to_vec()).collect();
        let mut used = 0;
        for row in rows {
            let (id, body) = row?;
            if !freed.contains(&id) {
                used += body.chars().count();
            }
        }
        Ok(used)
    }
}

type MemoryRow = (Vec<u8>, String, String, Option<Vec<u8>>, String, i64);

fn claim_from(row: MemoryRow) -> Result<MemoryClaim> {
    let (id, tier, cell_id, run_id, body, ts) = row;
    Ok(MemoryClaim {
        memory_id: MemoryId::from_bytes(crate::uuid_bytes(&id, "memory_id")?),
        tier: tier.parse().map_err(|_| StoreError::Corrupt {
            field: "tier",
            value: tier,
        })?,
        cell_id: CellId::new(cell_id),
        run_id: run_id
            .map(|r| crate::uuid_bytes(&r, "run_id").map(RunId::from_bytes))
            .transpose()?,
        body,
        ts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn zero() -> CellId {
        CellId::new("zero")
    }

    #[test]
    fn a_claim_written_cell_local_is_read_back_by_its_own_cell() {
        let mut s = store();
        s.write_memory(&NewMemory::cell_local(zero(), "prefer MSVC", 1))
            .unwrap();
        let mine = s.read_memory(&MemoryScope::new(zero())).unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].body, "prefer MSVC");
        assert_eq!(mine[0].tier, MemoryTier::CellLocal);
    }

    #[test]
    fn cell_local_does_not_leak_across_cells_but_global_reaches_everyone() {
        let mut s = store();
        s.write_memory(&NewMemory::cell_local(zero(), "brand voice is dry", 1))
            .unwrap();
        s.write_memory(&NewMemory {
            tier: MemoryTier::Global,
            ..NewMemory::cell_local(zero(), "this .cmd shim needs explicit resolution", 2)
        })
        .unwrap();

        let other = s
            .read_memory(&MemoryScope::new(CellId::new("social")))
            .unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].tier, MemoryTier::Global);
    }

    #[test]
    fn a_reader_sees_another_cells_lessons_only_when_its_own_definition_opts_in() {
        let mut s = store();
        s.write_memory(&NewMemory::cell_local(zero(), "prefer MSVC", 1))
            .unwrap();

        let mut scope = MemoryScope::new(CellId::new("social"));
        assert!(s.read_memory(&scope).unwrap().is_empty());

        scope.also_read.insert(zero());
        assert_eq!(s.read_memory(&scope).unwrap().len(), 1);
    }

    #[test]
    fn run_local_is_visible_only_inside_its_own_run() {
        let mut s = store();
        let run = RunId::new();
        s.write_memory(&NewMemory {
            tier: MemoryTier::RunLocal,
            run_id: Some(run),
            ..NewMemory::cell_local(zero(), "scratch", 1)
        })
        .unwrap();

        assert!(s.read_memory(&MemoryScope::new(zero())).unwrap().is_empty());
        assert_eq!(
            s.read_memory(&MemoryScope::new(zero()).in_run(run))
                .unwrap()
                .len(),
            1
        );
        assert!(
            s.read_memory(&MemoryScope::new(zero()).in_run(RunId::new()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_full_tier_errors_rather_than_dropping_anything() {
        let mut s = store().with_memory_caps(MemoryCaps {
            cell_local: 20,
            ..MemoryCaps::default()
        });
        s.write_memory(&NewMemory::cell_local(zero(), "0123456789", 1))
            .unwrap();
        let err = s
            .write_memory(&NewMemory::cell_local(zero(), "0123456789abc", 2))
            .unwrap_err();
        assert!(matches!(
            err,
            StoreError::MemoryCapExceeded {
                used: 10,
                cap: 20,
                ..
            }
        ));
        assert_eq!(
            s.read_memory(&MemoryScope::new(zero())).unwrap().len(),
            1,
            "the refused write left the tier exactly as it was"
        );
    }

    #[test]
    fn consolidating_under_a_full_cap_succeeds_because_the_superseded_space_is_freed() {
        let mut s = store().with_memory_caps(MemoryCaps {
            cell_local: 20,
            ..MemoryCaps::default()
        });
        let a = s
            .write_memory(&NewMemory::cell_local(zero(), "0123456789", 1))
            .unwrap();
        let b = s
            .write_memory(&NewMemory::cell_local(zero(), "abcdefghij", 2))
            .unwrap();

        let merged = s
            .write_memory(&NewMemory {
                supersedes: vec![a, b],
                ..NewMemory::cell_local(zero(), "0123456789abcdefghij", 3)
            })
            .unwrap();

        let live = s.read_memory(&MemoryScope::new(zero())).unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].memory_id, merged);
    }

    #[test]
    fn retraction_hides_a_claim_without_removing_its_history() {
        let mut s = store();
        let id = s
            .write_memory(&NewMemory::cell_local(zero(), "wrong lesson", 1))
            .unwrap();
        s.retract_memory(id, 2).unwrap();

        assert!(s.read_memory(&MemoryScope::new(zero())).unwrap().is_empty());
        assert_eq!(
            s.memory(id).unwrap().unwrap().body,
            "wrong lesson",
            "what was believed and when still resolves by id"
        );
    }

    #[test]
    fn promoting_to_global_needs_the_operator_and_the_manager_cannot_do_it() {
        let mut s = store();
        let id = s
            .write_memory(&NewMemory::cell_local(zero(), "windows path gotcha", 1))
            .unwrap();

        assert!(matches!(
            s.promote_memory(id, MemoryTier::Global, Promotion::ManagerDecided, 2),
            Err(StoreError::GlobalPromotionNeedsOperator)
        ));

        let promoted = s
            .promote_memory(id, MemoryTier::Global, Promotion::OperatorApproved, 3)
            .unwrap();
        let seen_elsewhere = s
            .read_memory(&MemoryScope::new(CellId::new("social")))
            .unwrap();
        assert_eq!(seen_elsewhere.len(), 1);
        assert_eq!(seen_elsewhere[0].memory_id, promoted);
    }

    #[test]
    fn a_secret_in_a_lesson_never_reaches_the_disk() {
        let mut s = store();
        s.write_memory(&NewMemory::cell_local(
            zero(),
            "deploy with token ghp_ZzAa0011223344556677889900",
            1,
        ))
        .unwrap();
        let body = &s.read_memory(&MemoryScope::new(zero())).unwrap()[0].body;
        assert!(!body.contains("ghp_"));
    }

    #[test]
    fn retracting_a_claim_that_does_not_exist_says_so() {
        let mut s = store();
        assert!(matches!(
            s.retract_memory(MemoryId::new(), 1),
            Err(StoreError::NoSuchMemory(_))
        ));
    }
}
