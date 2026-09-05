//! Telling a person something happened, when nobody is looking at the screen.
//!
//! `35 notification plane`. Every other surface farseer has is pull - `16 local
//! api surface` gives one SSE endpoint, `07 attach semantics` makes live and
//! replay the same call with a different cursor, and `28 operator surface` puts
//! a canvas on top of both. All three assume an operator watching. A run that
//! finishes or hangs at 3am reaches nobody.
//!
//! **This is not a gate, and must never become one.** Telling a person is not
//! asking one for permission: nothing here is answerable, and nothing waits on
//! it. What `35` refused is a bar that approves **shell commands** mid-run, on
//! `12 autonomy and deny list`'s argument that `deny read .env` is defeated by
//! `cat .env` - approving a command string is false assurance.
//!
//! That argument reaches command strings and stops there. `12` section 3 gates
//! a *declared* tool at a *declared* irreversibility level, which `post` is and
//! `cat` does not defeat, and this comment used to say `12` had refused mid-run
//! approval outright. It had not. See `38 the tool verb`.
//!
//! **Off unless `FARSEER_NOTIFY_URL` is set.** The URL is the seam: anything
//! that accepts an HTTP POST is a backend, so ntfy, Slack, Discord and an
//! operator's own bridge are one code path and a different address. It lives in
//! the environment rather than in `runners.toml` because ntfy's own
//! documentation is explicit that a topic *is* the password - which makes it a
//! credential, and `31 manager delegation reach` already settled where those
//! live.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use farseer_core::{EventKind, Liveness, RunId, Seq};

use crate::AppState;

/// Where to POST. Absent means the whole plane is off, which is the default.
pub(crate) const ENV_URL: &str = "FARSEER_NOTIFY_URL";

/// Slow on purpose. Nothing here is a control path, and a run that ended two
/// seconds ago is not more finished than one that ended ten seconds ago.
const POLL: Duration = Duration::from_secs(5);

/// How many events one pass will look at. The same bound the event stream uses,
/// for the same reason: a burst must not become an unbounded read.
const BATCH: usize = 256;

/// One thing worth waking a person for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Notification {
    pub title: String,
    pub body: String,
    /// ntfy's 1-5 scale, sent as a header any other backend is free to ignore.
    pub priority: u8,
}

/// Start the watcher, if the operator asked for one.
pub(crate) fn spawn(state: Arc<AppState>) {
    let Some(url) = std::env::var(ENV_URL).ok().filter(|u| !u.trim().is_empty()) else {
        return;
    };
    tokio::spawn(watch(state, url));
}

/// Poll two sources, because farseer holds these two facts in two places.
///
/// **The record** answers "did a run finish": `02 record scope` makes that an
/// append, so a cursor over the log sees every one exactly once, and a notifier
/// reading the log structurally cannot announce something that did not happen.
///
/// **The live handles** answer "is a run hung", because `16 local api surface`
/// keeps liveness **derived, never stored** - there is no `status_changed` event
/// to subscribe to, and this module does not add one. That rule is worth more
/// than the symmetry: a stored liveness is a second truth to keep in step with
/// the first.
async fn watch(state: Arc<AppState>, url: String) {
    // Live only. Starting at the end of the log means a restart does not replay
    // yesterday's finished runs into somebody's phone.
    let mut cursor = state.store().latest_seq().unwrap_or(0);
    let mut warned: HashSet<RunId> = HashSet::new();
    let client = reqwest::Client::new();
    let mut ticker = tokio::time::interval(POLL);

    loop {
        ticker.tick().await;
        for notification in poll(&state, &mut cursor, &mut warned) {
            deliver(&client, &url, &notification).await;
        }
    }
}

/// One pass over both sources. Split out from [`watch`] so the decisions - which
/// events, which runs, once each - are testable without a listener or a clock.
fn poll(state: &AppState, cursor: &mut Seq, warned: &mut HashSet<RunId>) -> Vec<Notification> {
    let mut out = Vec::new();

    let batch = state
        .store()
        .scan(*cursor, BATCH, &farseer_store::ScanFilter::default())
        .unwrap_or_default();
    for event in batch {
        *cursor = event.seq;
        let kind = event.kind.as_str();
        if kind != EventKind::RUN_FINISHED && kind != EventKind::MANAGER_ANSWERED {
            continue;
        }
        // A manager delegating six workers finishes seven runs. Six of them are
        // its own business, and a notifier that reports all seven is one the
        // operator mutes - after which it is worse than absent.
        if !is_root_run(state, event.run_id) {
            continue;
        }
        if kind == EventKind::MANAGER_ANSWERED {
            // The trigger an end-to-end run added, because the obvious one is
            // not enough on its own: `15 manager conversation` keeps a manager
            // **open** on live stdin after it answers, so `run_finished` does
            // not arrive until somebody cancels or closes the session. An
            // operator who walked away would have been told nothing at all.
            //
            // This is the same moment Claude Code raises its own `Notification`
            // hook on - the agent has said its piece and is waiting on a person.
            out.push(Notification {
                title: "farseer: answered".to_string(),
                body: format!("run {} is waiting for you", short(event.run_id)),
                priority: 3,
            });
            continue;
        }
        // A finished run also stops being hung, so anything held for it goes.
        warned.remove(&event.run_id);
        let outcome = event
            .payload
            .get("outcome")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("finished");
        out.push(Notification {
            title: format!("farseer: {outcome}"),
            body: format!("run {} {outcome}", short(event.run_id)),
            // Failure is the one an operator wants pushed through a quiet hour.
            priority: if outcome == "ok" { 3 } else { 4 },
        });
    }

    // `05 run state model` made this signal trustworthy, and that is what makes
    // it worth sending. The watchdog once keyed on progress events, which meant
    // a model reasoning for twenty minutes was flagged while working perfectly;
    // it now keys on **any bytes from the adapter**, so `likely-hung` is
    // mechanical silence. A notifier can only be built on the second one.
    let hung: Vec<RunId> = state
        .runs()
        .iter()
        .filter(|(_, handle)| handle.liveness.liveness() == Liveness::LikelyHung)
        .map(|(run_id, _)| *run_id)
        .collect();
    for run_id in &hung {
        // Edge-triggered: the run stays hung for as long as it stays silent,
        // and the operator is told once.
        if warned.insert(*run_id) {
            out.push(Notification {
                title: "farseer: likely hung".to_string(),
                body: format!(
                    "run {} has produced nothing for {}s",
                    short(*run_id),
                    state.thresholds.likely_hung_secs
                ),
                priority: 4,
            });
        }
    }
    // A run that recovered, or ended while farseer was not looking, may be told
    // about again if it hangs a second time.
    warned.retain(|run_id| hung.contains(run_id));

    out
}

/// Whether this run is the one an operator asked for, rather than one a manager
/// delegated. See [`farseer_store::Store::first_run_of_task`].
///
/// A run farseer cannot resolve is treated as a root, because what this guards
/// against is **noise**, and the worse failure is silence.
fn is_root_run(state: &AppState, run_id: RunId) -> bool {
    let store = state.store();
    let Ok(Some(row)) = store.run(run_id) else {
        return true;
    };
    match store.first_run_of_task(row.task_id) {
        Ok(Some(first)) => first == run_id,
        _ => true,
    }
}

/// The first segment of an id, which is what the operator's own tooling prints.
fn short(run_id: RunId) -> String {
    run_id.to_string()[..8].to_string()
}

/// Best-effort, and deliberately so.
///
/// A notifier that can fail a run is worse than no notifier, so a refused POST,
/// a dead host and a wrong URL all end here. Nothing retries: the next thing
/// worth saying will be along, and a queue of stale alerts is its own problem.
async fn deliver(client: &reqwest::Client, url: &str, notification: &Notification) {
    let _ = client
        .post(url)
        .header("Title", &notification.title)
        .header("Priority", notification.priority.to_string())
        .body(notification.body.clone())
        .timeout(Duration::from_secs(10))
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scoping rule, on the shape that motivated it: one manager, one
    /// delegated worker, one task, and **one** notification.
    #[test]
    fn a_delegated_worker_is_not_the_run_an_operator_asked_for() {
        let store = farseer_store::Store::open_in_memory().unwrap();
        let task = farseer_core::TaskId::new();
        let manager = RunId::new();
        let worker = RunId::new();
        for (run_id, started) in [(manager, 100), (worker, 200)] {
            store
                .upsert_run(&farseer_store::RunRow {
                    run_id,
                    task_id: task,
                    cell_id: farseer_core::CellId::new("zero"),
                    runner: "pi".into(),
                    model: String::new(),
                    outcome: None,
                    usd_micros: 0,
                    tokens: 0,
                    operator_touched: false,
                    started_ts: started,
                    finished_ts: None,
                })
                .unwrap();
        }
        assert_eq!(store.first_run_of_task(task).unwrap(), Some(manager));
        assert_ne!(store.first_run_of_task(task).unwrap(), Some(worker));
    }

    /// A hang is told once, not once per poll, and again only if it recurs.
    #[test]
    fn a_run_that_stays_hung_is_reported_once() {
        let mut warned = HashSet::new();
        let run = RunId::new();
        assert!(warned.insert(run), "the first sighting says so");
        assert!(!warned.insert(run), "the second says nothing");
        warned.retain(|r| *r != run);
        assert!(warned.insert(run), "a run that recovered may hang again");
    }
}
