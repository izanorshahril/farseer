//! `11 analytics questions`'s four questions.
//!
//! `11 analytics questions` cut the analytics surface down to three entities, two edge kinds and
//! four queries, which is what let `09 store decision` reject a graph engine: **two edge kinds
//! is a join, not a graph problem.** The recursive walk below is the entire
//! graph workload, and `09 store decision` measured it at 790ms with 30,165 chains at 100x the
//! target scale. These are queries a human runs occasionally, not a hot loop.

use serde::Serialize;

use crate::{Result, Store};

/// Q1: cost per successful run, by runner and model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CostRow {
    pub runner: String,
    pub model: String,
    pub runs: i64,
    pub usd_micros: i64,
    pub tokens: i64,
    pub usd_micros_per_run: i64,
}

/// Q2: intervention rate, by cell.
///
/// `11 analytics questions` chose this as the headline metric, and `12 autonomy and deny list` noted the metric only means
/// something while the human is still the gate - an automated merge would make
/// the number look excellent while measuring nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterventionRow {
    pub cell_id: String,
    pub runs: i64,
    pub touched: i64,
}

/// Q3: how deep the rework chains go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReworkRow {
    pub root_run_id: String,
    pub depth: i64,
}

/// Q4: did a lesson help?
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LessonRow {
    pub memory_id: String,
    pub body: String,
    pub consulted_by: i64,
    pub ok_runs: i64,
    pub failed_runs: i64,
}

impl Store {
    /// Q1. Successful runs only, because cost on a failed run answers a
    /// different question.
    pub fn cost_by_runner_and_model(&self) -> Result<Vec<CostRow>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT runner, model, COUNT(*), SUM(usd_micros), SUM(tokens)
             FROM runs WHERE outcome = 'ok'
             GROUP BY runner, model
             ORDER BY SUM(usd_micros) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let runs: i64 = row.get(2)?;
            let usd_micros: i64 = row.get(3)?;
            Ok(CostRow {
                runner: row.get(0)?,
                model: row.get(1)?,
                runs,
                usd_micros,
                tokens: row.get(4)?,
                usd_micros_per_run: usd_micros / runs.max(1),
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(Into::into)
    }

    /// Q2.
    pub fn intervention_rate_by_cell(&self) -> Result<Vec<InterventionRow>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT cell_id, COUNT(*), SUM(operator_touched)
             FROM runs GROUP BY cell_id ORDER BY cell_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(InterventionRow {
                cell_id: row.get(0)?,
                runs: row.get(1)?,
                touched: row.get(2)?,
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(Into::into)
    }

    /// Q3. The recursive CTE `09 store decision` benched, walking up the `rescoped_from` chain.
    pub fn rework_depth(&self) -> Result<Vec<ReworkRow>> {
        let mut stmt = self.conn().prepare_cached(
            "WITH RECURSIVE chain(run_id, root, depth) AS (
                 SELECT r.run_id, r.run_id, 1
                 FROM runs r
                 WHERE NOT EXISTS (SELECT 1 FROM rescoped_from f WHERE f.run_id = r.run_id)
               UNION ALL
                 SELECT f.run_id, c.root, c.depth + 1
                 FROM rescoped_from f JOIN chain c ON f.parent = c.run_id
             )
             SELECT root, MAX(depth) FROM chain GROUP BY root ORDER BY MAX(depth) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ReworkRow {
                root_run_id: id_string(&row.get::<_, Vec<u8>>(0)?),
                depth: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(Into::into)
    }

    /// Q4. `11 analytics questions`'s fourth question, and the reason `02 record scope` added the
    /// `memory_consulted` event and the `consulted` edge at all.
    pub fn lessons_against_outcome(&self) -> Result<Vec<LessonRow>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT m.memory_id, m.body,
                    COUNT(r.run_id),
                    SUM(CASE WHEN r.outcome = 'ok' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN r.outcome = 'failed' THEN 1 ELSE 0 END)
             FROM memories m
             JOIN consulted c ON c.memory_id = m.memory_id
             JOIN runs r ON r.run_id = c.run_id
             GROUP BY m.memory_id, m.body
             ORDER BY COUNT(r.run_id) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LessonRow {
                memory_id: id_string(&row.get::<_, Vec<u8>>(0)?),
                body: row.get(1)?,
                consulted_by: row.get(2)?,
                ok_runs: row.get(3)?,
                failed_runs: row.get(4)?,
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(Into::into)
    }
}

/// Render a stored id the way every other surface renders one.
///
/// A run id printed here has to be the same string `GET /v1/runs/{id}` parses,
/// or a client cannot follow a chart back to the run it names.
fn id_string(bytes: &[u8]) -> String {
    match <[u8; 16]>::try_from(bytes) {
        Ok(bytes) => farseer_core::RunId::from_bytes(bytes).to_string(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewMemory, RunRow};
    use farseer_core::{CellId, RunId, TaskId};

    fn finished(
        store: &Store,
        cell: &str,
        runner: &str,
        outcome: &str,
        usd: u64,
        touched: bool,
    ) -> RunId {
        let run_id = RunId::new();
        store
            .upsert_run(&RunRow {
                run_id,
                task_id: TaskId::new(),
                cell_id: CellId::new(cell),
                runner: runner.into(),
                model: "opus-5".into(),
                outcome: Some(outcome.into()),
                usd_micros: usd,
                tokens: 1_000,
                operator_touched: touched,
                started_ts: 1,
                finished_ts: Some(2),
            })
            .unwrap();
        run_id
    }

    #[test]
    fn cost_groups_by_runner_and_model_over_successful_runs_only() {
        let s = Store::open_in_memory().unwrap();
        finished(&s, "zero", "claude-code", "ok", 300_000, false);
        finished(&s, "zero", "claude-code", "ok", 100_000, false);
        finished(&s, "zero", "claude-code", "failed", 900_000, false);
        finished(&s, "zero", "codex", "ok", 50_000, false);

        let rows = s.cost_by_runner_and_model().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].runner, "claude-code");
        assert_eq!(rows[0].runs, 2);
        assert_eq!(rows[0].usd_micros, 400_000);
        assert_eq!(rows[0].usd_micros_per_run, 200_000);
    }

    #[test]
    fn intervention_rate_counts_touched_runs_per_cell() {
        let s = Store::open_in_memory().unwrap();
        finished(&s, "zero", "claude-code", "ok", 1, true);
        finished(&s, "zero", "claude-code", "ok", 1, false);
        finished(&s, "social", "codex", "ok", 1, false);

        let rows = s.intervention_rate_by_cell().unwrap();
        assert_eq!(rows.len(), 2);
        let zero = rows.iter().find(|r| r.cell_id == "zero").unwrap();
        assert_eq!((zero.runs, zero.touched), (2, 1));
    }

    #[test]
    fn rework_depth_walks_the_rescope_chain() {
        let s = Store::open_in_memory().unwrap();
        let a = finished(&s, "zero", "claude-code", "failed", 1, false);
        let b = finished(&s, "zero", "claude-code", "failed", 1, false);
        let c = finished(&s, "zero", "claude-code", "ok", 1, false);
        let lone = finished(&s, "zero", "claude-code", "ok", 1, false);
        s.record_rescope(b, a).unwrap();
        s.record_rescope(c, b).unwrap();

        let rows = s.rework_depth().unwrap();
        assert_eq!(rows[0].depth, 3, "a -> b -> c is one chain three deep");
        assert_eq!(rows.len(), 2, "the unrelated run is its own chain");
        assert_eq!(rows[1].depth, 1);
        assert!(!rows.iter().any(|r| r.root_run_id.is_empty()));
        let _ = lone;
    }

    #[test]
    fn a_lesson_is_scored_against_the_outcomes_of_the_runs_that_read_it() {
        let mut s = Store::open_in_memory().unwrap();
        let lesson = s
            .write_memory(&NewMemory::cell_local(
                CellId::new("zero"),
                "resolve .cmd shims explicitly",
                1,
            ))
            .unwrap();
        let good = finished(&s, "zero", "claude-code", "ok", 1, false);
        let bad = finished(&s, "zero", "claude-code", "failed", 1, false);
        s.record_consulted(good, lesson, 2).unwrap();
        s.record_consulted(bad, lesson, 3).unwrap();

        let rows = s.lessons_against_outcome().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].consulted_by, rows[0].ok_runs, rows[0].failed_runs),
            (2, 1, 1)
        );
    }

    #[test]
    fn an_id_in_a_result_row_is_the_same_string_the_api_parses_back() {
        let s = Store::open_in_memory().unwrap();
        let run = finished(&s, "zero", "claude-code", "ok", 1, false);
        let rows = s.rework_depth().unwrap();
        assert_eq!(rows[0].root_run_id, run.to_string());
        assert_eq!(
            rows[0].root_run_id.parse::<RunId>().unwrap(),
            run,
            "a client must be able to follow the chart back to the run"
        );
    }

    #[test]
    fn every_query_answers_on_an_empty_record() {
        let s = Store::open_in_memory().unwrap();
        assert!(s.cost_by_runner_and_model().unwrap().is_empty());
        assert!(s.intervention_rate_by_cell().unwrap().is_empty());
        assert!(s.rework_depth().unwrap().is_empty());
        assert!(s.lessons_against_outcome().unwrap().is_empty());
    }
}
