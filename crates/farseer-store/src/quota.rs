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
/// **Farseer never computes a percentage here, and `used_percent` is not one.**
/// `27 quota accounting` section 2 refused a percentage derived from farseer's
/// own spend, because the same window is drained by sessions farseer cannot see
/// (wrong in a way the operator could not detect, and most wrong exactly near
/// exhaustion). That refusal stands. `used_percent` is present only when the
/// **provider states it**, which `30 codex app server` found `codex app-server`
/// doing after every turn, and is absent for every runner that does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowRow {
    pub account: String,
    /// `allowed`, `exhausted_until` or `unknown`.
    pub status: String,
    pub resets_at: Option<i64>,
    pub rate_limit_type: String,
    pub is_using_overage: bool,
    /// The provider's own reading, never farseer's arithmetic.
    ///
    /// Skipped entirely when absent rather than serialized as `null`, so a
    /// window nobody reported a percentage for carries **no percentage on the
    /// wire at all** - which is what `27 quota accounting`'s own test greps for,
    /// and what keeps its refusal literally true for every runner that states
    /// nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_duration_mins: Option<i64>,
    /// When farseer **first saw** this window, which is what its own spend is
    /// counted from.
    ///
    /// Not when the provider opened it: nothing farseer observes says that, and
    /// deriving it from `rate_limit_type` would mean inventing a window length
    /// `10 runner inventory` never measured.
    pub since_ts: i64,
    /// Farseer's own spend since `since_ts`. A **lower bound** twice over - the
    /// window is drained by sessions farseer cannot see, and a run already in
    /// flight when the window was first seen is not counted. Still the only
    /// honest number available, and a lower bound that says so beats a
    /// percentage that does not.
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
        if let Some(previous) = self.latest_observation(observation.window_key())?
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

    /// The last thing said about **this** window.
    ///
    /// Keyed by account *and* limit, per `30 codex app server`: one account can
    /// be running several windows at once, and keying by account alone makes two
    /// of them look like one flapping between two states - so every observation
    /// would differ from the last and the on-change discipline would collapse
    /// into recording everything.
    fn latest_observation(&self, key: (&str, &str)) -> Result<Option<WindowObservation>> {
        let (account, rate_limit_type) = key;
        let row: Option<String> = self
            .conn()
            .query_row(
                "SELECT payload FROM events
                 WHERE kind = ?1
                   AND json_extract(payload, '$.account') = ?2
                   AND COALESCE(json_extract(payload, '$.rate_limit_type'), '') = ?3
                 ORDER BY seq DESC LIMIT 1",
                rusqlite::params![EventKind::RATE_LIMIT, account, rate_limit_type],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|payload| Ok(serde_json::from_str(&payload)?))
            .transpose()
    }

    /// Every window's current state, newest transition first.
    ///
    /// One row per **window**, not per account: `30 codex app server` found one
    /// account can be running two at once.
    ///
    /// The spend figures are farseer's own runs on that account's runners since
    /// the transition, which is the number `27 quota accounting` found actually
    /// answers the fleet question: not "how much of my window is left", which is
    /// unanswerable, but "what has the fleet spent".
    pub fn windows(&self, runners_on: impl Fn(&str) -> Vec<String>) -> Result<Vec<WindowRow>> {
        let mut stmt = self.conn().prepare(
            "SELECT payload, MAX(ts) FROM events
             WHERE kind = ?1
             GROUP BY json_extract(payload, '$.account'),
                      COALESCE(json_extract(payload, '$.rate_limit_type'), '')
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
                    used_percent: observation.used_percent,
                    window_duration_mins: observation.window_duration_mins,
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
            used_percent: None,
            window_duration_mins: None,
        }
    }

    fn observe(store: &Store, observation: &WindowObservation, ts: i64) -> bool {
        store
            .observe_window(&CellId::new("zero"), RunId::new(), observation, ts)
            .unwrap()
    }

    /// `30 codex app server`: Codex reports a five-hour and a weekly window in
    /// the same notification, and they alternate in the log.
    ///
    /// Keyed by account alone, each would differ from the one before it forever,
    /// and "append on change" would silently become "append everything" - the
    /// exact noise `27 quota accounting` section 4 built this to avoid.
    #[test]
    fn two_windows_on_one_account_do_not_flap_against_each_other() {
        let store = Store::open_in_memory().unwrap();
        let five_hour = {
            let mut o = observation(Availability::Allowed {
                resets_at: Some(1787710593),
            });
            o.runner = "codex-app-server".into();
            o.used_percent = Some(12);
            o
        };
        let weekly = {
            let mut o = five_hour.clone();
            o.rate_limit_type = "weekly".into();
            o.availability = Availability::Allowed {
                resets_at: Some(1788273509),
            };
            o.used_percent = Some(3);
            o
        };

        assert!(observe(&store, &five_hour, 100));
        assert!(observe(&store, &weekly, 101));
        // Both seen again, unchanged: neither is a transition.
        assert!(!observe(&store, &five_hour, 102));
        assert!(!observe(&store, &weekly, 103));

        let rows = store.windows(|_| vec!["codex-app-server".into()]).unwrap();
        assert_eq!(
            rows.len(),
            2,
            "one row per window, not per account: {rows:?}"
        );
        let weekly_row = rows
            .iter()
            .find(|r| r.rate_limit_type == "weekly")
            .expect("the weekly window has its own row");
        assert_eq!(weekly_row.used_percent, Some(3));
    }

    #[test]
    fn a_window_filling_up_is_a_transition_the_operator_can_see_coming() {
        let store = Store::open_in_memory().unwrap();
        let mut window = observation(Availability::Allowed { resets_at: Some(1) });
        window.runner = "codex-app-server".into();
        window.used_percent = Some(10);
        assert!(observe(&store, &window, 100));
        assert!(!observe(&store, &window, 101));

        window.used_percent = Some(85);
        assert!(
            observe(&store, &window, 102),
            "the status is still `allowed`, and that is the point"
        );
        let rows = store.windows(|_| vec![]).unwrap();
        assert_eq!(rows[0].status, "allowed");
        assert_eq!(rows[0].used_percent, Some(85));
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

    /// The other half of the one above, added by `30 codex app server`.
    ///
    /// The refusal was never "no number shaped like a percentage may exist" - it
    /// was "farseer must not compute one from its own spend". A provider that
    /// states its own is an observation, and `10 runner inventory`'s rule admits
    /// it for the same reason it admits `resetsAt`.
    #[test]
    fn a_percentage_the_provider_stated_is_reported_as_it_was_stated() {
        let store = Store::open_in_memory().unwrap();
        let mut window = observation(Availability::Allowed {
            resets_at: Some(1_787_710_593),
        });
        window.runner = "codex-app-server".into();
        window.used_percent = Some(41);
        window.window_duration_mins = Some(300);
        observe(&store, &window, 10);

        // Spend on this account exists and is deliberately unrelated to the
        // number reported: nothing here turns one into the other.
        store
            .upsert_run(&RunRow {
                run_id: RunId::new(),
                task_id: TaskId::new(),
                cell_id: CellId::new("zero"),
                runner: "codex-app-server".into(),
                model: String::new(),
                outcome: None,
                usd_micros: 999_000,
                tokens: 5_000,
                started_ts: 11,
                finished_ts: None,
                operator_touched: false,
            })
            .unwrap();

        let windows = store.windows(|_| vec!["codex-app-server".into()]).unwrap();
        assert_eq!(windows[0].used_percent, Some(41));
        assert_eq!(windows[0].window_duration_mins, Some(300));
        assert_eq!(
            windows[0].farseer_usd_micros, 999_000,
            "farseer's own spend is still reported as itself, beside the              provider's reading rather than instead of it"
        );
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
        // The fixture is a runner that states nothing, which is every runner but
        // `codex-app-server`. Nothing may appear from farseer's own arithmetic.
        for absent in ["percent", "used_", "remaining", "quota_left"] {
            assert!(
                !wire.contains(absent),
                "`{absent}` would present a lower bound as a measurement"
            );
        }
    }
}
