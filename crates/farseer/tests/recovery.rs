use std::process::Command;

use farseer_core::{Actor, CellId, NewEvent, RunId, TranscriptCustody};
use farseer_store::{RunRow, STORE_FORMAT_VERSION, Store, TranscriptAttachment};
use rusqlite::Connection;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_farseer"))
}

#[test]
fn cli_backup_and_restore_round_trip_committed_wal_data() {
    let fixture = tempfile::tempdir().unwrap();
    let record = fixture.path().join("record.sqlite3");
    let transcripts = fixture.path().join("transcripts");
    std::fs::create_dir(&transcripts).unwrap();
    let source = transcripts.join("source.jsonl");
    std::fs::write(&source, b"committed transcript").unwrap();
    let run_id = RunId::new();
    let task_id = farseer_core::TaskId::new();

    let store = Store::open(&record).unwrap();
    store
        .upsert_run(&RunRow {
            run_id,
            task_id,
            cell_id: CellId::new("zero"),
            runner: "goose".into(),
            model: "model".into(),
            outcome: Some("ok".into()),
            usd_micros: 2,
            tokens: 3,
            operator_touched: false,
            started_ts: 1,
            finished_ts: Some(2),
        })
        .unwrap();
    store
        .append(&NewEvent::new(
            CellId::new("zero"),
            run_id,
            "committed",
            Actor::Worker,
            2,
            serde_json::json!({"task_id": task_id}),
        ))
        .unwrap();
    store
        .record_transcript_attachment(&TranscriptAttachment {
            digest: "digest-1".into(),
            run_id,
            custody: TranscriptCustody::Copy,
            source: source.display().to_string(),
            stored_path: Some(source.display().to_string()),
            created_ts: 2,
        })
        .unwrap();

    let backup = fixture.path().join("backup");
    let output = cli()
        .args([
            "--record",
            record.to_str().unwrap(),
            "backup",
            backup.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "backup failed: {:?}", output);
    assert!(backup.join("manifest.json").is_file());
    assert!(backup.join("record.sqlite3").is_file());
    drop(store);

    let restored = fixture.path().join("restored.sqlite3");
    let output = cli()
        .args([
            "--record",
            restored.to_str().unwrap(),
            "restore",
            backup.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "restore failed: {:?}", output);

    let restored_store = Store::open(&restored).unwrap();
    assert_eq!(restored_store.latest_seq().unwrap(), 1);
    assert_eq!(
        restored_store.run(run_id).unwrap().unwrap().task_id,
        task_id
    );
    let attachments = restored_store.transcript_attachments(Some(run_id)).unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(
        std::fs::read(attachments[0].stored_path.as_ref().unwrap()).unwrap(),
        b"committed transcript"
    );
}

#[test]
fn cli_backup_refuses_a_future_schema_before_admitting_work() {
    let fixture = tempfile::tempdir().unwrap();
    let record = fixture.path().join("future.sqlite3");
    let conn = Connection::open(&record).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA user_version = {};",
        STORE_FORMAT_VERSION + 1
    ))
    .unwrap();
    drop(conn);

    let output = cli()
        .args([
            "--record",
            record.to_str().unwrap(),
            "backup",
            fixture.path().join("backup").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("newer than this binary supports"),
        "{stderr}"
    );
    assert!(!fixture.path().join("backup").exists());
}
