//! Bench for `09 Store: SQLite edge tables and CTEs, or an embedded graph engine?`
//!
//! Builds the record shape `02` and `11` fixed, at 10x the honest target scale,
//! then times the two workloads that actually matter:
//!   - the hot path: append, and `WHERE seq > X ORDER BY seq` cursor reads
//!   - the cold path: the four analytics queries from `11`, including a recursive CTE
//!
//! Usage: storebench [scale_multiplier]

use rusqlite::{params, Connection};
use std::time::{Duration, Instant};

/// Honest target from the ticket is "thousands of tasks, tens of thousands of events".
/// These are 10x that, so headroom is visible rather than assumed.
const TASKS: usize = 10_000;
const RUNS_PER_TASK: usize = 2;
const EVENTS_PER_RUN: usize = 10;
const MEMORIES: usize = 500;

fn schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;

        -- The append-only log. `seq` is INTEGER PRIMARY KEY, so it is the rowid:
        -- a range scan on it is a b-tree walk with no secondary index to maintain.
        CREATE TABLE events (
            seq       INTEGER PRIMARY KEY,
            event_id  BLOB NOT NULL,
            ts        INTEGER NOT NULL,
            cell_id   INTEGER NOT NULL,
            run_id    INTEGER NOT NULL,
            kind      TEXT NOT NULL,
            actor     TEXT NOT NULL,
            payload   TEXT NOT NULL
        );
        CREATE INDEX events_run ON events(run_id, seq);

        CREATE TABLE runs (
            run_id   INTEGER PRIMARY KEY,
            task_id  INTEGER NOT NULL,
            cell_id  INTEGER NOT NULL,
            seat     TEXT NOT NULL,
            model    TEXT NOT NULL,
            outcome  TEXT NOT NULL,
            cost_usd REAL NOT NULL,
            tokens   INTEGER NOT NULL,
            touched  INTEGER NOT NULL,
            ts       INTEGER NOT NULL
        );
        CREATE INDEX runs_task ON runs(task_id);

        CREATE TABLE memories (
            memory_id INTEGER PRIMARY KEY,
            tier      TEXT NOT NULL,
            cell_id   INTEGER
        );

        -- The two edge kinds `11` kept. Nothing else.
        CREATE TABLE consulted (run_id INTEGER NOT NULL, memory_id INTEGER NOT NULL);
        CREATE INDEX consulted_mem ON consulted(memory_id);
        CREATE TABLE rescoped_from (run_id INTEGER PRIMARY KEY, parent_run_id INTEGER NOT NULL);
        CREATE INDEX rescoped_parent ON rescoped_from(parent_run_id);
        "#,
    )
}

/// Cheap deterministic pseudo-random, so runs are comparable and there is no dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn upto(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn pct(v: &mut Vec<Duration>, p: usize) -> Duration {
    v.sort_unstable();
    v[(v.len() * p / 100).min(v.len() - 1)]
}

fn time_query(db: &Connection, sql: &str) -> (Duration, usize) {
    let t0 = Instant::now();
    let mut stmt = db.prepare(sql).unwrap();
    let rows: usize = stmt
        .query_map([], |_| Ok(()))
        .unwrap()
        .filter_map(Result::ok)
        .count();
    (t0.elapsed(), rows)
}

fn main() -> rusqlite::Result<()> {
    let mult: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let tasks = TASKS * mult;
    let runs = tasks * RUNS_PER_TASK;
    let events = runs * EVENTS_PER_RUN;

    let path = std::env::current_dir().unwrap().join("bench.db");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));

    let mut db = Connection::open(&path)?;
    schema(&db)?;

    println!("scale: {tasks} tasks, {runs} runs, {events} events, {MEMORIES} memories\n");

    let seats = ["acp:claude", "acp:codex", "native:claude-stream", "proc:ffmpeg"];
    let models = ["opus-5", "sonnet-5", "gpt-5.6", "n/a"];
    let outcomes = ["ok", "ok", "ok", "ok", "failed", "cancelled"];
    let kinds = [
        "tool_call",
        "tool_result",
        "status_change",
        "manager_steered",
        "operator_intervened",
        "context_compacted",
        "memory_consulted",
    ];

    // --- 1. populate, and time the append hot path ------------------------------------
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let t0 = Instant::now();
    {
        let tx = db.transaction()?;
        {
            let mut ins = tx.prepare("INSERT INTO memories VALUES (?,?,?)")?;
            for m in 0..MEMORIES {
                let tier = ["global", "cell", "run"][m % 3];
                ins.execute(params![m as i64, tier, (m % 4) as i64])?;
            }
        }
        {
            let mut ins = tx.prepare("INSERT INTO runs VALUES (?,?,?,?,?,?,?,?,?,?)")?;
            for r in 0..runs {
                let outcome = outcomes[rng.upto(outcomes.len())];
                ins.execute(params![
                    r as i64,
                    (r / RUNS_PER_TASK) as i64,
                    (r % 4) as i64,
                    seats[rng.upto(seats.len())],
                    models[rng.upto(models.len())],
                    outcome,
                    (rng.upto(500) as f64) / 100.0,
                    rng.upto(2_000_000) as i64,
                    (rng.upto(10) == 0) as i64,
                    1_700_000_000i64 + r as i64,
                ])?;
            }
        }
        {
            let mut ins = tx.prepare("INSERT INTO rescoped_from VALUES (?,?)")?;
            // Roughly 15% of runs are rework, chained to the previous run in the same task.
            for r in 0..runs {
                if r % RUNS_PER_TASK != 0 && rng.upto(100) < 30 {
                    ins.execute(params![r as i64, (r - 1) as i64])?;
                }
            }
        }
        {
            let mut ins = tx.prepare("INSERT INTO consulted VALUES (?,?)")?;
            for r in 0..runs {
                for _ in 0..rng.upto(3) {
                    ins.execute(params![r as i64, rng.upto(MEMORIES) as i64])?;
                }
            }
        }
        {
            let mut ins = tx.prepare("INSERT INTO events VALUES (?,?,?,?,?,?,?,?)")?;
            let mut seq: i64 = 0;
            let payload = r#"{"tool":"Bash","args":{"command":"cargo test"},"ok":true}"#;
            for r in 0..runs {
                for _ in 0..EVENTS_PER_RUN {
                    seq += 1;
                    let uuid: [u8; 16] = std::array::from_fn(|i| (rng.next() >> (i % 8 * 8)) as u8);
                    ins.execute(params![
                        seq,
                        &uuid[..],
                        1_700_000_000i64 + seq,
                        (r % 4) as i64,
                        r as i64,
                        kinds[rng.upto(kinds.len())],
                        "worker",
                        payload,
                    ])?;
                }
            }
        }
        tx.commit()?;
    }
    let load = t0.elapsed();
    println!(
        "append (batched in one txn): {events} events + {runs} runs in {:.2?}  =  {:.0} events/sec",
        load,
        events as f64 / load.as_secs_f64()
    );

    // Single-event append, the realistic live path: one row, one commit, WAL fsync.
    let mut singles = Vec::new();
    {
        let mut ins = db.prepare("INSERT INTO events VALUES (?,?,?,?,?,?,?,?)")?;
        for i in 0..200i64 {
            let uuid = [0u8; 16];
            let t = Instant::now();
            ins.execute(params![
                events as i64 + i + 1,
                &uuid[..],
                1_700_000_000i64,
                0,
                0,
                "tool_call",
                "worker",
                "{}"
            ])?;
            singles.push(t.elapsed());
        }
    }
    println!(
        "append (one event, one commit): p50 {:?}  p99 {:?}",
        pct(&mut singles, 50),
        pct(&mut singles, 99)
    );

    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!(
        "db size: {:.1} MB  ({:.0} bytes/event)\n",
        size as f64 / 1e6,
        size as f64 / events as f64
    );

    // --- 2. the hot read path: cursor range scans -------------------------------------
    let mut cursor = Vec::new();
    {
        let mut stmt =
            db.prepare("SELECT seq, event_id, ts, kind, actor, payload FROM events WHERE seq > ? ORDER BY seq LIMIT 500")?;
        for _ in 0..1000 {
            let start = rng.upto(events) as i64;
            let t = Instant::now();
            let n = stmt
                .query_map([start], |_| Ok(()))?
                .filter_map(Result::ok)
                .count();
            assert!(n > 0 || start as usize >= events - 1);
            cursor.push(t.elapsed());
        }
    }
    println!(
        "cursor scan  WHERE seq > ? ORDER BY seq LIMIT 500  over {} runs:",
        cursor.len()
    );
    println!(
        "  p50 {:?}   p95 {:?}   p99 {:?}   max {:?}\n",
        pct(&mut cursor, 50),
        pct(&mut cursor, 95),
        pct(&mut cursor, 99),
        pct(&mut cursor, 100)
    );

    // --- 3. the four analytics queries from `11` --------------------------------------
    println!("analytics, cold path (one run each):");

    let (d, n) = time_query(
        &db,
        "SELECT seat, model, COUNT(*) AS runs, SUM(cost_usd) AS cost, SUM(cost_usd)/COUNT(*) AS per_ok
         FROM runs WHERE outcome = 'ok' GROUP BY seat, model ORDER BY cost DESC",
    );
    println!("  Q1 cost per successful run, by seat and model : {d:>10.2?}  ({n} rows)");

    let (d, n) = time_query(
        &db,
        "SELECT cell_id,
                SUM(touched) * 1.0 / COUNT(*) AS intervention_rate,
                COUNT(*) AS runs
         FROM runs GROUP BY cell_id",
    );
    println!("  Q2 intervention rate, by cell                 : {d:>10.2?}  ({n} rows)");

    // Recursive CTE: the full rework chain per task, which is the query a graph
    // engine would supposedly be needed for.
    let (d, n) = time_query(
        &db,
        "WITH RECURSIVE chain(root, run_id, depth) AS (
             SELECT r.run_id, r.run_id, 0 FROM runs r
             WHERE NOT EXISTS (SELECT 1 FROM rescoped_from f WHERE f.run_id = r.run_id)
           UNION ALL
             SELECT c.root, f.run_id, c.depth + 1
             FROM rescoped_from f JOIN chain c ON f.parent_run_id = c.run_id
         )
         SELECT root, MAX(depth) AS rework_depth FROM chain GROUP BY root HAVING rework_depth > 0",
    );
    println!("  Q3 rework depth per chain (RECURSIVE CTE)     : {d:>10.2?}  ({n} rows)");

    let (d, n) = time_query(
        &db,
        "SELECT m.memory_id, m.tier,
                COUNT(*) AS uses,
                SUM(CASE WHEN r.outcome = 'ok' THEN 1 ELSE 0 END) * 1.0 / COUNT(*) AS ok_rate
         FROM consulted c
         JOIN runs r ON r.run_id = c.run_id
         JOIN memories m ON m.memory_id = c.memory_id
         GROUP BY m.memory_id HAVING uses > 5 ORDER BY ok_rate DESC",
    );
    println!("  Q4 lessons vs outcome (2 joins + group by)    : {d:>10.2?}  ({n} rows)");

    let (d, n) = time_query(
        &db,
        "SELECT kind, COUNT(*) FROM events GROUP BY kind ORDER BY 2 DESC",
    );
    println!("  (full table scan for reference)               : {d:>10.2?}  ({n} rows)");

    contention(&path, events)?;

    Ok(())
}

/// Reader latency while a writer is hot.
///
/// This is the risk to `16`'s event stream: a client tailing the cursor while the
/// runtime appends. SQLite in WAL mode allows one writer concurrent with many
/// readers, and `02` gives farseer exactly one owning writer, so the question is
/// not whether it deadlocks but whether tail latency degrades.
fn contention(path: &std::path::Path, events: usize) -> rusqlite::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let stop = Arc::new(AtomicBool::new(false));
    let stop_w = stop.clone();
    let p = path.to_path_buf();

    let writer = std::thread::spawn(move || {
        let db = Connection::open(&p).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .unwrap();
        let mut ins = db
            .prepare("INSERT INTO events VALUES (?,?,?,?,?,?,?,?)")
            .unwrap();
        let mut seq = events as i64 + 10_000;
        let mut n = 0u64;
        while !stop_w.load(Ordering::Relaxed) {
            seq += 1;
            n += 1;
            let uuid = [0u8; 16];
            ins.execute(params![
                seq,
                &uuid[..],
                1_700_000_000i64,
                0,
                0,
                "tool_call",
                "worker",
                "{}"
            ])
            .unwrap();
        }
        n
    });

    let db = Connection::open(path)?;
    let mut rng = Rng(42);
    let mut lat = Vec::new();
    {
        let mut stmt = db.prepare(
            "SELECT seq, event_id, ts, kind, actor, payload FROM events WHERE seq > ? ORDER BY seq LIMIT 500",
        )?;
        for _ in 0..1000 {
            let start = rng.upto(events) as i64;
            let t = Instant::now();
            let _ = stmt
                .query_map([start], |_| Ok(()))?
                .filter_map(Result::ok)
                .count();
            lat.push(t.elapsed());
        }
    }
    stop.store(true, Ordering::Relaxed);
    let written = writer.join().unwrap();

    println!(
        "\ncursor scan WHILE a writer appends continuously ({written} events written during the test):"
    );
    println!(
        "  p50 {:?}   p95 {:?}   p99 {:?}   max {:?}",
        pct(&mut lat, 50),
        pct(&mut lat, 95),
        pct(&mut lat, 99),
        pct(&mut lat, 100)
    );
    Ok(())
}
