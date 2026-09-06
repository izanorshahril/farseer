//! The record: **one physical append-only log, with cell-scoped visibility.**
//!
//! `02 record scope` settled that storage and visibility are not the same thing, which is
//! what dissolved the apparent conflict between `BRIEF.md` and `ARCHITECTURE.md`.
//! `09 store decision` then benched the substrate and chose SQLite outright.
//!
//! Two rules this crate enforces rather than documents:
//!
//! - **Agents never append events.** `02 record scope` section 8: an agent that can forge
//!   events can rewrite its own history. Agents write *memory*, which is marked
//!   as a claim, and that path is [`Store::write_memory`].
//! - **Scrub on the way in.** Never at read time, because that leaves the
//!   secrets on disk and one query bug away from exposure.

use rusqlite::{Connection, OptionalExtension, params_from_iter};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use farseer_core::{
    Actor, CellId, Event, EventId, EventKind, NewEvent, RunId, Seq, scrub::scrub_value,
};

mod analytics;
mod lifecycle;
pub mod maintenance;
mod memory;
mod quota;
mod resource;
mod roots;
mod schema;
mod ui_state;
mod work;

/// Storage format understood by this binary.
pub const STORE_FORMAT_VERSION: i64 = 1;
const BACKUP_FORMAT_VERSION: u32 = 1;
const BACKUP_DATABASE: &str = "record.sqlite3";
const BACKUP_MANIFEST: &str = "manifest.json";

pub use analytics::{CostPage, CostRow, InterventionRow, LessonRow, ReworkRow};
pub use farseer_core::MemoryId;
pub use lifecycle::{Lifecycle, Purged};
pub use memory::{MemoryCaps, MemoryClaim, MemoryScope, NewMemory, Promotion};
pub use quota::WindowRow;
pub use resource::ResourceSample;
pub use ui_state::{UI_STATE_CAP_BYTES, UI_STATE_KEY_CAP_BYTES};
pub use work::{
    ArtifactRow, GraphEdge, GraphFilter, GraphNode, GraphPage, IndexedTranscript, RunParent,
    SessionRow, SimilarityEdge, TaskCursor, TaskFilter, TranscriptAttachment, TranscriptProjection,
};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(
        "record schema version {found} is newer than this binary supports (maximum {supported})"
    )]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
    #[error(
        "backup format version {found} is newer than this binary supports (maximum {supported})"
    )]
    UnsupportedBackupVersion { found: u32, supported: u32 },
    #[error("backup attachment `{digest}` is missing or changed")]
    InvalidBackupAttachment { digest: String },
    #[error("backup destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("backup manifest is missing `{0}`")]
    MissingBackupFile(&'static str),
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
    #[error("promoting to the global tier is gated on the operator, per `25 memory lifecycle`")]
    GlobalPromotionNeedsOperator,
    #[error("no memory claim with id {0}")]
    NoSuchMemory(MemoryId),
    #[error("no conversation with id {0}")]
    NoSuchConversation(farseer_core::ConversationId),
    #[error("no task with id {0}")]
    NoSuchTask(farseer_core::TaskId),
    #[error("task cannot transition from {from} to {to}")]
    InvalidTaskTransition {
        from: farseer_core::TaskState,
        to: farseer_core::TaskState,
    },
    #[error("ui state for `{key}` is {size} bytes, over the {cap} byte cap")]
    UiStateTooLarge {
        key: String,
        size: usize,
        cap: usize,
    },
    #[error("ui state key is {size} bytes, over the {cap} byte cap")]
    UiStateKeyTooLong { size: usize, cap: usize },
    #[error("record holds an unreadable {field}: {value}")]
    Corrupt { field: &'static str, value: String },
    #[error("transcript source changed before its projection was committed")]
    StaleTranscriptProjection,
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format_version: u32,
    pub schema_version: i64,
    pub attachments: Vec<BackupAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupAttachment {
    pub digest: String,
    pub run_id: String,
    pub source: String,
    pub path: Option<String>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

/// Which slice of the log a reader wants. `16 local api surface` chose one stream endpoint scoped
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
/// `09 store decision` asked whether single-writer holds under a realistic worker fleet and
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

    fn from_connection(mut conn: Connection) -> Result<Self> {
        let found: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if found > STORE_FORMAT_VERSION {
            return Err(StoreError::UnsupportedSchemaVersion {
                found,
                supported: STORE_FORMAT_VERSION,
            });
        }
        conn.execute_batch(schema::PRAGMAS)?;
        Self::migrate_schema(&mut conn, found)?;
        Ok(Self {
            conn,
            caps: MemoryCaps::default(),
        })
    }

    /// Apply ordered, recoverable schema steps as one transaction.
    ///
    /// `schema::SCHEMA` stays idempotent because older binaries already
    /// stamped databases with version one while later slices added tables;
    /// the explicit dispatch keeps the next incompatible change reviewable
    /// rather than hiding it in an ever-growing open path.
    fn migrate_schema(conn: &mut Connection, found: i64) -> Result<()> {
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        let result = (|| {
            conn.execute_batch(schema::SCHEMA)?;
            match found {
                0 | 1 => Self::migrate_transcript_attachments(conn)?,
                _ => unreachable!("future schema versions are rejected before migration"),
            }
            conn.execute_batch(&format!("PRAGMA user_version = {STORE_FORMAT_VERSION};"))?;
            Ok(())
        })();
        match result {
            Ok(()) => conn.execute_batch("COMMIT;").map_err(Into::into),
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(error)
            }
        }
    }

    /// Write a consistent SQLite snapshot and all copied transcript bytes to a new directory.
    /// The manifest is published last, so a failed snapshot cannot look complete to a restore.
    pub fn backup_to(&self, destination: &Path, attachment_root: &Path) -> Result<BackupManifest> {
        if destination.exists() {
            return Err(StoreError::DestinationExists(destination.to_path_buf()));
        }
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let staging = unique_path(parent, ".farseer-backup");
        fs::create_dir(&staging)?;
        let result: Result<BackupManifest> = (|| {
            // Read attachment rows from the copied database, not the live connection. This
            // keeps the manifest aligned with the exact SQLite snapshot produced by backup.
            let database = staging.join(BACKUP_DATABASE);
            backup_database(&self.conn, &database)?;
            let snapshot_store = Store::open(&database)?;
            let attachments = snapshot_store.transcript_attachments(None)?;
            let snapshot: Result<BackupManifest> = (|| {
                let mut manifest = BackupManifest {
                    format_version: BACKUP_FORMAT_VERSION,
                    schema_version: STORE_FORMAT_VERSION,
                    attachments: Vec::with_capacity(attachments.len()),
                };
                let attachment_dir = staging.join("attachments");
                fs::create_dir(&attachment_dir)?;
                for attachment in attachments {
                    let Some(stored_path) = attachment.stored_path else {
                        manifest.attachments.push(BackupAttachment {
                            digest: attachment.digest,
                            run_id: attachment.run_id.to_string(),
                            source: attachment.source,
                            path: None,
                            size: None,
                            sha256: None,
                        });
                        continue;
                    };
                    let source = PathBuf::from(&stored_path);
                    let source = if source.is_absolute() {
                        source
                    } else {
                        attachment_root.join(source)
                    };
                    let target_name = safe_attachment_name(&attachment.digest)?;
                    let target = attachment_dir.join(target_name);
                    let (size, digest) = copy_digest(&source, &target)?;
                    manifest.attachments.push(BackupAttachment {
                        digest: attachment.digest,
                        run_id: attachment.run_id.to_string(),
                        source: attachment.source,
                        path: Some(format!(
                            "attachments/{}",
                            target.file_name().unwrap().to_string_lossy()
                        )),
                        size: Some(size),
                        sha256: Some(digest),
                    });
                }
                let bytes = serde_json::to_vec_pretty(&manifest)?;
                let mut file = fs::File::create(staging.join(BACKUP_MANIFEST))?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                Ok(manifest)
            })();
            drop(snapshot_store);
            let manifest = snapshot?;
            // Windows may keep SQLite directory handles alive briefly after backup.  Publish
            // the files with the manifest last; an incomplete directory is never restorable.
            fs::create_dir(destination)?;
            fs::rename(&database, destination.join(BACKUP_DATABASE))?;
            fs::rename(staging.join("attachments"), destination.join("attachments"))?;
            fs::rename(
                staging.join(BACKUP_MANIFEST),
                destination.join(BACKUP_MANIFEST),
            )?;
            fs::remove_dir(&staging)?;
            Ok(manifest)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
            let _ = fs::remove_dir_all(destination);
        }
        result
    }

    /// Restore a backup into a new record and attachment directory, validating the recovered
    /// record through the normal open path before returning.
    pub fn restore_from(
        backup: &Path,
        record: &Path,
        attachment_root: &Path,
    ) -> Result<BackupManifest> {
        if record.exists() {
            return Err(StoreError::DestinationExists(record.to_path_buf()));
        }
        let manifest_path = backup.join(BACKUP_MANIFEST);
        let database = backup.join(BACKUP_DATABASE);
        if !manifest_path.is_file() {
            return Err(StoreError::MissingBackupFile(BACKUP_MANIFEST));
        }
        if !database.is_file() {
            return Err(StoreError::MissingBackupFile(BACKUP_DATABASE));
        }
        let manifest: BackupManifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
        if manifest.format_version > BACKUP_FORMAT_VERSION {
            return Err(StoreError::UnsupportedBackupVersion {
                found: manifest.format_version,
                supported: BACKUP_FORMAT_VERSION,
            });
        }
        if manifest.schema_version > STORE_FORMAT_VERSION {
            return Err(StoreError::UnsupportedSchemaVersion {
                found: manifest.schema_version,
                supported: STORE_FORMAT_VERSION,
            });
        }
        let parent = record.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let staged = unique_path(parent, ".farseer-restore");
        let attachment_parent = attachment_root.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(attachment_parent)?;
        let attachment_staging = unique_path(attachment_parent, ".farseer-restore-attachments");
        let mut published_attachments = Vec::new();
        let result = (|| {
            let source = Connection::open(&database)?;
            backup_database(&source, &staged)?;
            let store = Store::open(&staged)?;
            fs::create_dir(&attachment_staging)?;
            let mut attachment_targets = Vec::new();
            for attachment in &manifest.attachments {
                let Some(path) = &attachment.path else {
                    continue;
                };
                let relative = Path::new(path);
                if relative.is_absolute()
                    || relative.components().count() != 2
                    || relative.components().next().is_none_or(|component| {
                        component != std::path::Component::Normal("attachments".as_ref())
                    })
                {
                    return Err(StoreError::InvalidBackupAttachment {
                        digest: attachment.digest.clone(),
                    });
                }
                let source = backup.join(relative);
                let target = attachment_root.join(safe_attachment_name(&attachment.digest)?);
                let staged_target =
                    attachment_staging.join(safe_attachment_name(&attachment.digest)?);
                let (size, digest) = copy_digest(&source, &staged_target)?;
                if Some(size) != attachment.size
                    || Some(digest.as_str()) != attachment.sha256.as_deref()
                {
                    return Err(StoreError::InvalidBackupAttachment {
                        digest: attachment.digest.clone(),
                    });
                }
                store.set_attachment_path(&attachment.digest, &attachment.run_id, &target)?;
                attachment_targets.push((staged_target, target, size, digest));
            }
            drop(store);
            let restored = Store::open(&staged)?;
            if restored.transcript_attachments(None)?.len() != manifest.attachments.len() {
                return Err(StoreError::InvalidBackupAttachment {
                    digest: "manifest association count".into(),
                });
            }
            drop(restored);
            fs::create_dir_all(attachment_root)?;
            for (staged_target, target, size, digest) in attachment_targets {
                if target.exists() {
                    let existing = digest_file(&target)?;
                    if existing != (size, digest) {
                        return Err(StoreError::InvalidBackupAttachment {
                            digest: target
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or_default()
                                .to_owned(),
                        });
                    }
                    let _ = fs::remove_file(&staged_target);
                } else {
                    fs::rename(&staged_target, &target)?;
                    published_attachments.push(target);
                }
            }
            fs::remove_dir(&attachment_staging)?;
            fs::rename(&staged, record)?;
            Ok(manifest)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&staged);
            let _ = fs::remove_dir_all(&attachment_staging);
            for path in published_attachments.drain(..) {
                let _ = fs::remove_file(path);
            }
        }
        result
    }

    fn set_attachment_path(&self, digest: &str, run_id: &str, path: &Path) -> Result<()> {
        let run_id = run_id
            .parse::<RunId>()
            .map_err(|_| StoreError::InvalidBackupAttachment {
                digest: digest.into(),
            })?;
        let changed = self.conn.execute(
            "UPDATE transcript_attachments SET stored_path = ?3 WHERE digest = ?1 AND run_id = ?2",
            rusqlite::params![
                digest,
                &run_id.as_bytes()[..],
                path.to_string_lossy().as_ref()
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::InvalidBackupAttachment {
                digest: digest.into(),
            });
        }
        Ok(())
    }

    /// Upgrade the first unreleased `40 work model and session explorer` schema,
    /// where a content digest accidentally owned the run association.
    ///
    /// Content remains deduplicated on disk, while SQLite must retain one
    /// association per `(digest, run_id)`.
    fn migrate_transcript_attachments(conn: &Connection) -> Result<()> {
        let composite_key_columns: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('transcript_attachments')
         WHERE (name = 'digest' AND pk = 1) OR (name = 'run_id' AND pk = 2)",
            [],
            |row| row.get(0),
        )?;
        if composite_key_columns == 2 {
            return Ok(());
        }
        conn.execute_batch(
            "DROP INDEX IF EXISTS transcript_attachments_run;
         ALTER TABLE transcript_attachments RENAME TO transcript_attachments_legacy;
         CREATE TABLE transcript_attachments (
             digest       TEXT NOT NULL,
             run_id       BLOB NOT NULL,
             custody      TEXT NOT NULL,
             source       TEXT NOT NULL,
             stored_path  TEXT,
             created_ts   INTEGER NOT NULL,
             PRIMARY KEY (digest, run_id)
         );
         INSERT INTO transcript_attachments
             (digest, run_id, custody, source, stored_path, created_ts)
         SELECT digest, run_id, custody, source, stored_path, created_ts
         FROM transcript_attachments_legacy;
         DROP TABLE transcript_attachments_legacy;
         CREATE INDEX transcript_attachments_run ON transcript_attachments(run_id);",
        )?;
        Ok(())
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

    /// Append many in one transaction. `09 store decision` measured this at ~308k events/sec
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

    /// The cursor read `16 local api surface` and `07 attach semantics` both depend on: everything after `since`,
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

    /// The **last** `limit` events, oldest first.
    ///
    /// [`Self::scan`] reads forward from a cursor, which is what an attach
    /// wants: `07 attach semantics` made replay and live the same call with a
    /// different cursor, and a cursor always points at a beginning.
    ///
    /// A surface that opens cold has no cursor and does not want one. It wants
    /// what just happened - and reading forward from zero gives it the opposite,
    /// silently: with a limit of 200 and a log of 200, the canvas's conversation
    /// looked correct, and the day the log passed 200 it would have frozen on
    /// the oldest 200 events with no error anywhere.
    ///
    /// Ordered ascending on the way out so a caller folds it exactly like a
    /// `scan`. Selected descending, because "the last N" cannot be expressed as
    /// an offset over a log with holes in it, and `05 run state model`'s purge
    /// puts holes in it.
    pub fn scan_tail(&self, limit: usize, filter: &ScanFilter) -> Result<Vec<Event>> {
        let head = self.latest_seq()?;
        let mut sql = String::from("SELECT seq FROM events WHERE seq <= ?1");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(head)];
        if let Some(cell_id) = &filter.cell_id {
            params.push(Box::new(cell_id.as_str().to_string()));
            sql.push_str(&format!(" AND cell_id = ?{}", params.len()));
        }
        if let Some(run_id) = &filter.run_id {
            params.push(Box::new(run_id.as_bytes().to_vec()));
            sql.push_str(&format!(" AND run_id = ?{}", params.len()));
        }
        params.push(Box::new(limit as i64));
        sql.push_str(&format!(" ORDER BY seq DESC LIMIT ?{}", params.len()));

        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut seqs = stmt
            .query_map(params_from_iter(params.iter().map(|p| p.as_ref())), |row| {
                row.get::<_, Seq>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // The oldest of the tail, made exclusive, is the cursor that reads the
        // tail forwards - so the rows themselves come back through `scan` and
        // there is one place that turns a row into an `Event`.
        let Some(oldest) = seqs.pop() else {
            return Ok(Vec::new());
        };
        self.scan(oldest - 1, limit, filter)
    }

    /// The highest cursor position in the log, or 0 when it is empty.
    pub fn latest_seq(&self) -> Result<Seq> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?)
    }

    /// Record a run for `11 analytics questions`'s four questions. Deleting a cell does not delete
    /// this: `02 record scope` section 7 keeps the record when the cell goes, because a
    /// definition is a file in git and its history is not reversible.
    /// Close every run row still marked running, and say which they were.
    ///
    /// `17 cell lifecycle` chose **no orphan survival**: a run's process dies
    /// with the runtime, and nothing reattaches. The row it left behind did
    /// survive, though, and said `running` forever - so a restart accumulated
    /// permanent live-looking runs that no verb could reach, because
    /// `cancel` asks the in-memory handle and an orphan has none. Eight had
    /// piled up by 2026-08-27 and every fleet surface was reading them as work
    /// in flight.
    ///
    /// `Failed` rather than a new outcome: `05 run state model` defines it as
    /// *something broke and nobody chose it*, which is exactly what a runtime
    /// going away is. `Abandoned` would be wrong - it means a manager decided
    /// the run was unnecessary **before it started**, and these had started.
    /// The caller records the reason on the event so the record can tell a
    /// reaped run from one that failed on its own.
    ///
    /// Safe because farseer is one instance per machine - one `runtime.json`,
    /// one port. A second concurrent runtime would have its live runs reaped by
    /// the newcomer, which is a consequence of that assumption rather than of
    /// this sweep.
    pub fn reap_orphaned_runs(&self, finished_ts: i64) -> Result<Vec<RunRow>> {
        let orphans: Vec<RunRow> = self
            .recent_runs(5_000)?
            .into_iter()
            .filter(|row| row.outcome.is_none())
            .collect();
        for row in &orphans {
            self.conn.execute(
                "UPDATE runs SET outcome = 'failed', finished_ts = ?2 WHERE run_id = ?1",
                rusqlite::params![&row.run_id.as_bytes()[..], finished_ts],
            )?;
        }
        Ok(orphans)
    }

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

    /// `run -> re-scoped-from -> run`, one of `11 analytics questions`'s two edge kinds.
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

fn unique_path(parent: &Path, prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    parent.join(format!("{prefix}-{}-{stamp}", std::process::id()))
}

fn backup_database(source: &Connection, destination: &Path) -> Result<()> {
    let mut destination = Connection::open(destination)?;
    rusqlite::backup::Backup::new(source, &mut destination)?.run_to_completion(
        100,
        Duration::from_millis(10),
        None,
    )?;
    Ok(())
}

fn safe_attachment_name(digest: &str) -> Result<&str> {
    let path = Path::new(digest);
    if digest.is_empty()
        || path.file_name().and_then(|name| name.to_str()) != Some(digest)
        || path.components().count() != 1
    {
        return Err(StoreError::InvalidBackupAttachment {
            digest: digest.to_owned(),
        });
    }
    Ok(digest)
}

fn copy_digest(source: &Path, target: &Path) -> Result<(u64, String)> {
    let mut input = fs::File::open(source)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = fs::File::create(target)?;
    let mut hasher = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = input.read(&mut bytes)?;
        if read == 0 {
            break;
        }
        output.write_all(&bytes[..read])?;
        hasher.update(&bytes[..read]);
        size += read as u64;
    }
    output.sync_all()?;
    let mut digest = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut digest, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok((size, digest))
}

fn digest_file(path: &Path) -> Result<(u64, String)> {
    let mut input = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = input.read(&mut bytes)?;
        if read == 0 {
            break;
        }
        hasher.update(&bytes[..read]);
        size += read as u64;
    }
    let mut digest = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut digest, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok((size, digest))
}

fn uuid_bytes(raw: &[u8], field: &'static str) -> Result<[u8; 16]> {
    raw.try_into().map_err(|_| StoreError::Corrupt {
        field,
        value: format!("{} bytes", raw.len()),
    })
}

/// A run, as `11 analytics questions` needs it: `cost`, `tokens`, `runner`, `model`, `cell_id`,
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
    /// Set permanently once a human touched the run, per `07 attach semantics`. Provenance, not
    /// a reason to restrict what happens next.
    pub operator_touched: bool,
    pub started_ts: i64,
    pub finished_ts: Option<i64>,
}

/// Convenience for reading a row back, used by tests and by the API.
impl Store {
    /// The most recent runs, newest first.
    ///
    /// Ordered by `started_ts` rather than by insertion, because a run that
    /// started earlier and finished later is still the older run - and the
    /// fleet view reads top-down.
    pub fn recent_runs(&self, limit: usize) -> Result<Vec<RunRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT run_id, task_id, cell_id, runner, model, outcome, usd_micros, tokens,
                    operator_touched, started_ts, finished_ts
             FROM runs ORDER BY started_ts DESC, rowid DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|r| {
                Ok(RunRow {
                    run_id: RunId::from_bytes(uuid_bytes(&r.0, "run_id")?),
                    task_id: farseer_core::TaskId::from_bytes(uuid_bytes(&r.1, "task_id")?),
                    cell_id: CellId::new(r.2),
                    runner: r.3,
                    model: r.4,
                    outcome: r.5,
                    usd_micros: r.6 as u64,
                    tokens: r.7 as u64,
                    operator_touched: r.8 != 0,
                    started_ts: r.9,
                    finished_ts: r.10,
                })
            })
            .collect()
    }

    /// The first run of a task, which is the one an operator asked for.
    ///
    /// Every run a manager delegates carries its manager's `task_id`, so a task
    /// is a tree and this is its root. `35 notification plane` is the caller:
    /// one manager delegating six workers must send **one** notification, not
    /// seven, and a notifier nobody trusts is one people mute.
    ///
    /// Earliest `started_ts`, with the id as the tie-break so two runs started
    /// inside the same millisecond still give a stable answer rather than
    /// whichever the planner happened to reach first.
    pub fn first_run_of_task(&self, task_id: farseer_core::TaskId) -> Result<Option<RunId>> {
        let row = self
            .conn
            .query_row(
                "SELECT run_id FROM runs WHERE task_id = ?1
                 ORDER BY started_ts, run_id LIMIT 1",
                [&task_id.as_bytes()[..]],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        Ok(row
            .and_then(|bytes| <[u8; 16]>::try_from(bytes.as_slice()).ok())
            .map(RunId::from_bytes))
    }

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
    /// `38`-adjacent, found while reviewing the canvas: the two conversation
    /// widgets replayed with `limit`, which reads **forward from zero**. With a
    /// log shorter than the limit that is indistinguishable from a tail, and
    /// the day it grows past one the surface freezes on ancient history with no
    /// error anywhere.
    #[test]
    fn a_tail_reads_the_end_of_the_log_and_a_scan_reads_the_start() {
        let store = Store::open_in_memory().unwrap();
        let run = RunId::new();
        for n in 0..10 {
            store
                .append(&event("zero", run, &format!("kind-{n}"), n))
                .unwrap();
        }
        let filter = ScanFilter::default();
        let head: Vec<_> = store
            .scan(0, 3, &filter)
            .unwrap()
            .iter()
            .map(|e| e.kind.as_str().to_string())
            .collect();
        let tail: Vec<_> = store
            .scan_tail(3, &filter)
            .unwrap()
            .iter()
            .map(|e| e.kind.as_str().to_string())
            .collect();
        assert_eq!(head, ["kind-0", "kind-1", "kind-2"]);
        // Oldest first, so a caller folds a tail exactly like a scan.
        assert_eq!(tail, ["kind-7", "kind-8", "kind-9"]);
        // A tail longer than the log is the whole log, not an error.
        assert_eq!(store.scan_tail(100, &filter).unwrap().len(), 10);
        assert!(
            Store::open_in_memory()
                .unwrap()
                .scan_tail(5, &filter)
                .unwrap()
                .is_empty()
        );
    }

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

        store
            .purge_cell(&CellId::new("social"), None, None)
            .unwrap();

        assert_eq!(store.run(run).unwrap(), None);
        assert!(
            store.cost_by_runner_and_model().unwrap().is_empty(),
            "analytics still reports spend on what was destroyed"
        );
        assert!(store.intervention_rate_by_cell().unwrap().is_empty());
    }

    /// `17 cell lifecycle`: purge takes a scope, because a purge that can only
    /// destroy everything cannot serve a retention policy - and retention is
    /// the reason purge exists.
    #[test]
    fn a_purge_destroys_the_range_it_was_given_and_nothing_either_side_of_it() {
        let mut store = Store::open_in_memory().unwrap();
        let run = RunId::new();
        for ts in [10, 20, 30, 40] {
            store
                .append(&event("zero", run, &format!("at-{ts}"), ts))
                .unwrap();
        }

        let purged = store
            .purge_cell(&CellId::new("zero"), Some(20), Some(30))
            .unwrap();
        assert_eq!(purged.events, 2, "both ends of the range are inclusive");

        let left = store.scan(0, 100, &ScanFilter::default()).unwrap();
        assert_eq!(
            left.iter().map(|e| e.ts).collect::<Vec<_>>(),
            vec![10, 40],
            "a scoped purge must not reach outside its range"
        );
    }

    /// Active has no row, so a fleet nobody has touched needs no seeding to be
    /// correct - and coming back to active removes the row rather than writing
    /// the word, so "not moved" has one representation.
    #[test]
    fn a_cell_that_was_never_moved_is_active_and_coming_back_leaves_no_trace() {
        let store = Store::open_in_memory().unwrap();
        let zero = CellId::new("zero");
        assert_eq!(store.cell_state(&zero).unwrap(), Lifecycle::Active);
        assert!(store.cell_states().unwrap().is_empty());

        store.set_cell_state(&zero, Lifecycle::Paused, 1).unwrap();
        assert_eq!(store.cell_state(&zero).unwrap(), Lifecycle::Paused);
        assert!(!Lifecycle::Paused.accepts_work());

        store.set_cell_state(&zero, Lifecycle::Archived, 2).unwrap();
        assert_eq!(store.cell_state(&zero).unwrap(), Lifecycle::Archived);

        store.set_cell_state(&zero, Lifecycle::Active, 3).unwrap();
        assert!(store.cell_states().unwrap().is_empty());
    }

    #[test]
    fn a_purge_leaves_permanent_holes_that_a_cursor_read_walks_over() {
        let mut store = Store::open_in_memory().unwrap();
        let run = RunId::new();
        store.append(&event("zero", run, "a", 1)).unwrap();
        store.append(&event("social", run, "b", 2)).unwrap();
        store.append(&event("zero", run, "c", 3)).unwrap();

        assert_eq!(
            store
                .purge_cell(&CellId::new("zero"), None, None)
                .unwrap()
                .events,
            2
        );

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
    #[test]
    fn digest_only_transcript_schema_migrates_without_losing_associations() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE transcript_attachments (
                 digest TEXT PRIMARY KEY,
                 run_id BLOB NOT NULL,
                 custody TEXT NOT NULL,
                 source TEXT NOT NULL,
                 stored_path TEXT,
                 created_ts INTEGER NOT NULL
             );
             CREATE INDEX transcript_attachments_run
                 ON transcript_attachments(run_id);",
        )
        .unwrap();
        let first = RunId::new();
        conn.execute(
            "INSERT INTO transcript_attachments
                 (digest, run_id, custody, source, stored_path, created_ts)
             VALUES ('same-content', ?1, 'copy', 'first.jsonl', NULL, 1)",
            [&first.as_bytes()[..]],
        )
        .unwrap();

        let store = Store::from_connection(conn).unwrap();
        let second = RunId::new();
        store
            .record_transcript_attachment(&TranscriptAttachment {
                digest: "same-content".into(),
                run_id: second,
                custody: farseer_core::TranscriptCustody::Copy,
                source: "second.jsonl".into(),
                stored_path: None,
                created_ts: 2,
            })
            .unwrap();

        assert_eq!(store.transcript_attachments(Some(first)).unwrap().len(), 1);
        assert_eq!(store.transcript_attachments(Some(second)).unwrap().len(), 1);
    }

    #[test]
    fn a_migrated_store_reopens_without_losing_work_lineage_or_associations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.sqlite3");
        let first_run = RunId::new();
        let attachment = "same-content";
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE transcript_attachments (
                     digest TEXT PRIMARY KEY,
                     run_id BLOB NOT NULL,
                     custody TEXT NOT NULL,
                     source TEXT NOT NULL,
                     stored_path TEXT,
                     created_ts INTEGER NOT NULL
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transcript_attachments
                     (digest, run_id, custody, source, stored_path, created_ts)
                 VALUES (?1, ?2, 'copy', 'legacy.jsonl', NULL, 1)",
                rusqlite::params![attachment, &first_run.as_bytes()[..]],
            )
            .unwrap();
        }

        let task_id = TaskId::new();
        let conversation_id = farseer_core::ConversationId::new();
        let second_run = RunId::new();
        {
            let store = Store::open(&path).unwrap();
            store
                .create_conversation(&farseer_core::Conversation {
                    conversation_id,
                    title: "migrated".into(),
                    project_path: None,
                    manager_runner: Some("goose".into()),
                    created_ts: 1,
                    updated_ts: 1,
                    archived_ts: None,
                })
                .unwrap();
            store
                .create_task(&farseer_core::Task {
                    task_id,
                    conversation_id,
                    goal: "preserve lineage".into(),
                    title: "preserve lineage".into(),
                    project_path: None,
                    state: farseer_core::TaskState::Inbox,
                    priority: 0,
                    created_ts: 1,
                    updated_ts: 1,
                })
                .unwrap();
            store
                .upsert_run(&RunRow {
                    run_id: second_run,
                    task_id,
                    cell_id: CellId::new("zero"),
                    runner: "goose".into(),
                    model: "model".into(),
                    outcome: Some("ok".into()),
                    usd_micros: 1,
                    tokens: 2,
                    operator_touched: false,
                    started_ts: 1,
                    finished_ts: Some(2),
                })
                .unwrap();
            store
                .record_transcript_attachment(&TranscriptAttachment {
                    digest: attachment.into(),
                    run_id: second_run,
                    custody: farseer_core::TranscriptCustody::Copy,
                    source: "second.jsonl".into(),
                    stored_path: None,
                    created_ts: 2,
                })
                .unwrap();
            store
                .append(&event("zero", second_run, "lineage", 2))
                .unwrap();
        }

        let reopened = Store::open(&path).unwrap();
        assert_eq!(
            reopened.task(task_id).unwrap().unwrap().conversation_id,
            conversation_id
        );
        assert_eq!(reopened.run(second_run).unwrap().unwrap().task_id, task_id);
        assert_eq!(reopened.transcript_attachments(None).unwrap().len(), 2);
        assert_eq!(reopened.latest_seq().unwrap(), 1);
        let version: i64 = reopened
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, STORE_FORMAT_VERSION);
    }

    #[test]
    fn a_failed_migration_rolls_back_to_the_original_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.sqlite3");
        let run = RunId::new();
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE transcript_attachments (
                     digest TEXT NOT NULL,
                     run_id BLOB NOT NULL,
                     custody TEXT NOT NULL,
                     source TEXT NOT NULL,
                     stored_path TEXT,
                     created_ts INTEGER NOT NULL
                 );",
            )
            .unwrap();
            for source in ["one.jsonl", "two.jsonl"] {
                conn.execute(
                    "INSERT INTO transcript_attachments
                         (digest, run_id, custody, source, stored_path, created_ts)
                     VALUES ('duplicate', ?1, 'copy', ?2, NULL, 1)",
                    rusqlite::params![&run.as_bytes()[..], source],
                )
                .unwrap();
            }
        }

        assert!(matches!(Store::open(&path), Err(StoreError::Sqlite(_))));
        let conn = Connection::open(&path).unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcript_attachments", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 2);
        let legacy: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'transcript_attachments_legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy, 0);
        let events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'events'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            events, 0,
            "failed migration must not publish new schema tables"
        );
    }

    #[test]
    fn a_newer_schema_is_refused_before_opening_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        drop(conn);

        assert!(matches!(
            Store::open(&path),
            Err(StoreError::UnsupportedSchemaVersion {
                found: 99,
                supported: STORE_FORMAT_VERSION
            })
        ));
    }

    #[test]
    fn a_backup_restores_record_and_copied_attachment() {
        let source = tempfile::tempdir().unwrap();
        let record = source.path().join("record.sqlite3");
        let transcripts = source.path().join("transcripts");
        fs::create_dir(&transcripts).unwrap();
        let bytes_path = transcripts.join("attachment");
        fs::write(&bytes_path, b"recover me").unwrap();
        let (size, digest) = copy_digest(&bytes_path, &source.path().join("unused")).unwrap();
        fs::remove_file(source.path().join("unused")).unwrap();
        let run_id = RunId::new();
        {
            let store = Store::open(&record).unwrap();
            store.append(&event("zero", run_id, "observed", 1)).unwrap();
            store
                .record_transcript_attachment(&TranscriptAttachment {
                    digest: digest.clone(),
                    run_id,
                    custody: farseer_core::TranscriptCustody::Copy,
                    source: "input.jsonl".into(),
                    stored_path: Some(bytes_path.to_string_lossy().into_owned()),
                    created_ts: 1,
                })
                .unwrap();
        }
        assert_eq!(size, 10);

        let backup = source.path().join("backup");
        Store::open(&record)
            .unwrap()
            .backup_to(&backup, &transcripts)
            .unwrap();
        let mismatched_record = source.path().join("mismatched.sqlite3");
        let mismatched_transcripts = source.path().join("mismatched-transcripts");
        fs::create_dir(&mismatched_transcripts).unwrap();
        fs::write(mismatched_transcripts.join(&digest), b"do not overwrite").unwrap();
        assert!(matches!(
            Store::restore_from(&backup, &mismatched_record, &mismatched_transcripts),
            Err(StoreError::InvalidBackupAttachment { .. })
        ));
        assert_eq!(
            fs::read(mismatched_transcripts.join(&digest)).unwrap(),
            b"do not overwrite"
        );
        assert!(!mismatched_record.exists());
        let restored = source.path().join("restored.sqlite3");
        let restored_transcripts = source.path().join("restored-transcripts");
        Store::restore_from(&backup, &restored, &restored_transcripts).unwrap();
        let store = Store::open(&restored).unwrap();
        assert_eq!(store.latest_seq().unwrap(), 1);
        let attachments = store.transcript_attachments(None).unwrap();
        assert_eq!(attachments.len(), 1);
        let path = attachments[0].stored_path.as_ref().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"recover me");
    }
}
