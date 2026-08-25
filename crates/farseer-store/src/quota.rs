//! Window observations in the record: append on change, derive current.
//!
//! `27 quota accounting` section 4 chose this shape over a mutable current-state
//! table, and both consequences are improvements rather than compromises.
//!
//! - **The log records window transitions.** `10 runner inventory` measured
//!   `rate_limit_event` arriving on every successful run, and every concurrent
//!   run on one account reports the same window identically, so appending each
//!   one would bury the transitions in repetition. "How often did this account
//!   exhaust, and for how long" is a scan of a handful of rows.
//! - **Current state derives from the latest event**, exactly the trick
//!   `05 run state model` used for liveness. No mutable runtime state, so
//!   `24 ui state persistence`'s ruling that the opaque blob is the only
//!   non-record store stays true.
//!
//! The event carries `actor: system`, and `02 record scope` never lets the MCP
//! face append a raw event, so an agent cannot forge a window state to escape
//! routing.

use farseer_core::{Actor, CellId, EventKind, NewEvent, RunId, WindowObservation};
use rusqlite::OptionalExtension;
use serde::Serialize;

use crate::{Result, Store};

/// One account's window as it stands now, plus what farseer itself spent inside
/// it.
///
/// **There is no percentage here and there never will be.** `27 quota
/// accounting` section 2: farseer's own consumption is a lower bound on window
/// usage, because the same window is drained by sessions farseer cannot see, so
/// a percentage would be wrong in a way the operator could not detect - and most
/// wrong exactly near exhaustion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowRow {
    pub account: String,
    /// `allowed`, `exhausted_until` or `unknown`.
    pub status: String,
    pub resets_at: Option<i64>,
    pub rate_limit_type: String,
    pub is_using_overage: bool,
    /// When farseer first saw this window state, which is what its own spend is
    /// counted from.
    pub since_ts: i64,
    /// Farseer's own spend since `since_ts`. A **lower bound** on the window,
    /// and the only honest number available.
    pub farseer_usd_micros: i64,
    pub farseer_tokens: i64,
}

impl Store {
    /// Append this observation only if it differs from the account's last one.
    ///
    /// Returns whether a transition was recorded, which is what a caller wants
    /// for logging and what the tests assert on.
    pub fn observe_window(
        &self,
        cell_id: &CellId,
        run_id: RunId,
        observation: &WindowObservation,
        ts: i64,
    ) -> Result<bool> {
        if let Some(previous) = self.latest_observation(&observation.account)?
            && !observation.differs_from(&previous)
        {
            return Ok(false);
        }
        self.append(&NewEvent::new(
            cell_id.clone(),
            run_id,
            EventKind::RATE_LIMIT,
            Actor::System,
            ts,
            serde_json::to_value(observation)?,
        ))?;
        Ok(true)
    }

    fn latest_observation(&self, account: &str) -> Result<Option<WindowObservation>> {
        let row: Option<String> = self
            .conn()
            .query_row(
                "SELECT payload FROM events
                 WHERE kind = ?1 AND json_extract(payload, '$.account') = ?2
                 ORDER BY seq DESC LIMIT 1",
                rusqlite::params![EventKind::RATE_LIMIT, account],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|payload| Ok(serde_json::from_str(&payload)?))
            .transpose()
    }

    /// Every account's current window, newest transition first.
    ///
    /// The spend figures are farseer's own runs on that account's runners since
    /// the transition, which is the number `27 quota accounting` found actually
    /// answers the fleet question: not "how much of my window is left", which is
    /// unanswerable, but "what has the fleet spent".
    pub fn windows(&self, runners_on: impl Fn(&str) -> Vec<String>) -> Result<Vec<WindowRow>> {
        let mut stmt = self.conn().prepare(
            "SELECT payload, MAX(ts) FROM events
             WHERE kind = ?1
             GROUP BY json_extract(payload, '$.account')
             ORDER BY MAX(seq) DESC",
        )?;
        let latest = stmt
            .query_map([EventKind::RATE_LIMIT], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        latest
            .into_iter()
            .map(|(payload, since_ts)| {
                let observation: WindowObservation = serde_json::from_str(&payload)?;
                let (usd, tokens) =
                    self.spend_since(&runners_on(&observation.account), since_ts)?;
                Ok(WindowRow {
                    status: observation.availability.as_str().to_string(),
                    resets_at: observation.availability.resets_at(),
                    account: observation.account,
                    rate_limit_type: observation.rate_limit_type,
                    is_using_overage: observation.is_using_overage,
                    since_ts,
                    farseer_usd_micros: usd,
                    farseer_tokens: tokens,
                })
            })
            .collect()
    }

    /// What farseer's own runs on these runners cost since a moment.
    fn spend_since(&self, runners: &[String], since_ts: i64) -> Result<(i64, i64)> {
        if runners.is_empty() {
            return Ok((0, 0));
        }
        let placeholders = (2..2 + runners.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(since_ts)];
        params.extend(
            runners
                .iter()
                .map(|r| Box::new(r.clone()) as Box<dyn rusqlite::ToSql>),
        );
        let sql = format!(
            "SELECT COALESCE(SUM(usd_micros), 0), COALESCE(SUM(tokens), 0) FROM runs
             WHERE started_ts >= ?1 AND runner IN ({placeholders})"
        );
        Ok(self.conn().query_row(
            &sql,
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RunRow, ScanFilter};
    use farseer_core::{Availability, TaskId};

    fn observation(availability: Availability) -> WindowObservation {
        WindowObservation {
            account: "anthropic-max".into(),
            runner: "claude-code".into(),
            availability,
            rate_limit_type: "five_hour".into(),
            is_using_overage: false,
        }
    }

    fn observe(store: &Store, observation: &WindowObservation, ts: i64) -> bool {
        store
            .observe_window(&CellId::new("zero"), RunId::new(), observation, ts)
            .unwrap()
    }

    #[test]
    fn a_repeated_window_appends_nothing_and_a_transition_appends_once() {
        let store = Store::open_in_memory().unwrap();
        let allowed = observation(Availability::Allowed {
            resets_at: Some(1_787_003_600),
        });

        assert!(observe(&store, &allowed, 10), "the first sighting is news");
        assert!(!observe(&store, &allowed, 20));
        assert!(!observe(&store, &allowed, 30));
        assert!(observe(
            &store,
            &observation(Availability::ExhaustedUntil {
                resets_at: 1_787_000_000
            }),
            40
        ));

        let events = store.scan(0, 100, &ScanFilter::default()).unwrap();
        assert_eq!(
            events.len(),
            2,
            "every successful run reports the window; only transitions are history"
        );
        assert!(events.iter().all(|e| e.actor == Actor::System));
    }

    #[test]
    fn two_accounts_keep_two_windows() {
        let store = Store::open_in_memory().unwrap();
        observe(
            &store,
            &observation(Availability::Allowed {
                resets_at: Some(1_787_003_600),
            }),
            10,
        );
        let mut other = observation(Availability::Unknown);
        other.account = "openai-plus".into();
        other.runner = "codex".into();
        observe(&store, &other, 20);

        let windows = store.windows(|account| vec![account.to_string()]).unwrap();
        assert_eq!(windows.len(), 2);
        assert!(windows.iter().any(|w| w.account == "anthropic-max"));
        assert!(windows.iter().any(|w| w.status == "unknown"));
    }

    #[test]
    fn a_windows_spend_counts_every_runner_on_the_account_since_the_transition() {
        let store = Store::open_in_memory().unwrap();
        let run = |runner: &str, started_ts, usd, tokens| RunRow {
            run_id: RunId::new(),
            task_id: TaskId::new(),
            cell_id: CellId::new("zero"),
            runner: runner.into(),
            model: String::new(),
            outcome: Some("ok".into()),
            usd_micros: usd,
            tokens,
            operator_touched: false,
            started_ts,
            finished_ts: Some(started_ts + 1),
        };
        // Before the window opened, so outside it.
        store
            .upsert_run(&run("claude-code", 5, 900_000, 900))
            .unwrap();
        observe(
            &store,
            &observation(Availability::Allowed {
                resets_at: Some(1_787_003_600),
            }),
            10,
        );
        store
            .upsert_run(&run("claude-code", 11, 400_000, 400))
            .unwrap();
        // `27 quota accounting` section 3: a second runner on the same login
        // drains the same window.
        store
            .upsert_run(&run("claude-acp", 12, 100_000, 100))
            .unwrap();
        // A different account entirely.
        store.upsert_run(&run("codex", 13, 700_000, 700)).unwrap();

        let windows = store
            .windows(|account| match account {
                "anthropic-max" => vec!["claude-code".into(), "claude-acp".into()],
                other => vec![other.to_string()],
            })
            .unwrap();

        let window = windows
            .iter()
            .find(|w| w.account == "anthropic-max")
            .unwrap();
        assert_eq!(window.farseer_usd_micros, 500_000);
        assert_eq!(window.farseer_tokens, 500);
        assert_eq!(window.since_ts, 10);
    }

    #[test]
    fn a_window_row_carries_no_percentage_of_the_providers_window() {
        let store = Store::open_in_memory().unwrap();
        observe(
            &store,
            &observation(Availability::Allowed {
                resets_at: Some(1_787_003_600),
            }),
            10,
        );
        let windows = store.windows(|a| vec![a.to_string()]).unwrap();
        let wire = serde_json::to_string(&windows).unwrap();
        for absent in ["percent", "used_", "remaining", "quota_left"] {
            assert!(
                !wire.contains(absent),
                "`{absent}` would present a lower bound as a measurement"
            );
        }
    }
}
