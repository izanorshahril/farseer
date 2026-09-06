use std::process::Command;

use farseer_store::Store;
use farseer_store::maintenance::{RuntimeIdentity, artifact_digest, write_runtime_identity};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_farseer"))
}

fn write_fixture_runtime(path: &std::path::Path, version: &str, bytes: &[u8]) -> RuntimeIdentity {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join("runtime.bin"), bytes).unwrap();
    let identity = RuntimeIdentity {
        version: version.into(),
        artifact_digest: artifact_digest(path).unwrap(),
        schema_version: 1,
    };
    write_runtime_identity(path, &identity).unwrap();
    identity
}

#[test]
fn cli_failed_candidate_startup_restores_the_previous_fixture() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installation");
    let active = root.join("active");
    let candidate = fixture.path().join("candidate");
    let backup = fixture.path().join("backup");
    let attachments = fixture.path().join("transcripts");
    std::fs::create_dir_all(&attachments).unwrap();

    let previous = write_fixture_runtime(&active, "previous", b"known-good");
    let candidate_identity = write_fixture_runtime(&candidate, "candidate", b"new-build");

    let record = fixture.path().join("record.sqlite3");
    let store = Store::open(&record).unwrap();
    store.backup_to(&backup, &attachments).unwrap();
    drop(store);

    #[cfg(windows)]
    let startup = ["cmd".to_owned(), "/C".to_owned(), "exit 1".to_owned()];
    #[cfg(not(windows))]
    let startup = ["sh".to_owned(), "-c".to_owned(), "exit 1".to_owned()];

    let output = cli()
        .args([
            "promote-fixture",
            "--root",
            root.to_str().unwrap(),
            "--candidate",
            candidate.to_str().unwrap(),
            "--backup",
            backup.to_str().unwrap(),
            "--health",
            "--smoke",
            "--startup-program",
            &startup[0],
            "--startup-arg",
            &startup[1],
            "--startup-arg",
            &startup[2],
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "a failed candidate must refuse promotion"
    );

    let restored = farseer_store::maintenance::read_runtime_identity(&active).unwrap();
    assert_eq!(restored, previous);
    assert_eq!(
        std::fs::read(active.join("runtime.bin")).unwrap(),
        b"known-good"
    );
    assert_eq!(
        farseer_store::maintenance::read_runtime_identity(
            &root.join("failed").join(&candidate_identity.version)
        )
        .unwrap(),
        candidate_identity
    );

    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("promotion.json")).unwrap()).unwrap();
    assert_eq!(journal["phase"], "rolled_back");
    assert!(journal["startup"]["status"] == 1, "{journal}");
    assert!(
        journal["recovery"]
            .as_str()
            .unwrap()
            .contains("restore the previous runtime")
    );
}
